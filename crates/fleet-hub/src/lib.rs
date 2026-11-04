//! ANet Hub relay transport for caller-prepared, sealed envelopes.
//!
//! ANetCore owns envelope sealing, signature generation and delivery-token verification.
//! This crate never reads identity files or treats a Hub transport receipt as task completion.
//! The journal directory must remain stable across client restarts. Losing or replacing it
//! loses the at-most-once submission guarantee. Desktop delivery must share this claim before
//! a future dispatcher can offer a cross-channel guarantee.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use fleet_proto::DeliveryTier;
use fs2::FileExt;
use reqwest::{blocking::Client, header::HeaderValue, redirect::Policy, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

/// Access proof prepared by ANetCore for the exact envelope and Hub.
/// Intentionally does not implement Debug or Serialize.
pub struct SendAuth {
    pub access: String,
    pub home: Option<String>,
    pub proof: AccessProof,
}

pub enum AccessProof {
    Capability,
    Session {
        session_id: String,
        timestamp_ms: u64,
        mac: String,
    },
}

/// The caller supplies both task metadata and the sealed envelope. Their cryptographic
/// relationship is the caller's responsibility; the Hub cannot inspect the sealed task.
pub struct TaskDelivery {
    pub correlation_id: String,
    pub task_id: String,
    pub envelope: Vec<u8>,
    pub delivery_handle: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Association {
    pub correlation_id: String,
    pub task_id: String,
    pub hub: String,
    pub auth_binding_sha256: String,
    pub envelope_sha256: String,
    pub delivery_handle: String,
    pub channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Channel {
    Hub,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryReceipt {
    pub association: Association,
    pub state: DeliveryState,
}

impl DeliveryReceipt {
    /// A transport acknowledgment never proves execution or Desktop visibility.
    pub fn tier(&self) -> DeliveryTier {
        match &self.state {
            DeliveryState::Rejected { .. } => DeliveryTier::Failed,
            DeliveryState::Tracked {
                observation: RelayObservation::Queued,
                ..
            } => DeliveryTier::Accepted,
            _ => DeliveryTier::Unknown,
        }
    }
}

impl<'de> Deserialize<'de> for DeliveryReceipt {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct StoredReceipt {
            association: Association,
            state: DeliveryState,
            tier: Option<DeliveryTier>,
        }
        let stored = StoredReceipt::deserialize(deserializer)?;
        let receipt = Self {
            association: stored.association,
            state: stored.state,
        };
        if stored.tier.is_some_and(|tier| tier != receipt.tier()) {
            return Err(serde::de::Error::custom(
                "delivery tier is not supported by transport evidence",
            ));
        }
        Ok(receipt)
    }
}

impl Serialize for DeliveryReceipt {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("DeliveryReceipt", 3)?;
        state.serialize_field("association", &self.association)?;
        state.serialize_field("state", &self.state)?;
        state.serialize_field("tier", &self.tier())?;
        state.end()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum DeliveryState {
    /// Send may have reached the Hub. No RID was received, so Hub status cannot be queried.
    UnknownNoRid,
    Rejected {
        http_status: u16,
    },
    Tracked {
        relay: RelayReceipt,
        observation: RelayObservation,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RelayReceipt {
    pub rid: String,
    pub via_hub: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum RelayObservation {
    Queued,
    Unknown,
    /// ANetCore must verify this token before any delivery claim is promoted.
    DoneUnverified {
        dack: String,
    },
    ReadbackUnavailable,
}

#[derive(Debug)]
pub enum Error {
    InvalidInput(&'static str),
    Journal(std::io::Error),
    CorruptJournal,
    AssociationConflict,
    Busy,
    ClientConfiguration,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(reason) => write!(f, "invalid input: {reason}"),
            Self::Journal(_) => write!(f, "journal I/O failed"),
            Self::CorruptJournal => {
                write!(f, "journal is incomplete or invalid; refusing submission")
            }
            Self::AssociationConflict => {
                write!(f, "correlation ID is already bound to another delivery")
            }
            Self::Busy => write!(f, "delivery is being observed by another client"),
            Self::ClientConfiguration => write!(f, "HTTP client configuration failed"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Journal(value)
    }
}

pub struct HubClient {
    hub: Url,
    journal: PathBuf,
    timeout: Duration,
}

impl HubClient {
    /// HTTPS is required except for a literal loopback fixture address.
    pub fn new(hub: &str, journal: impl Into<PathBuf>, timeout: Duration) -> Result<Self, Error> {
        let mut hub = Url::parse(hub).map_err(|_| Error::InvalidInput("Hub URL"))?;
        let loopback = hub
            .host_str()
            .and_then(|s| s.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if (hub.scheme() != "https" && !(hub.scheme() == "http" && loopback))
            || !hub.username().is_empty()
            || hub.password().is_some()
            || hub.query().is_some()
            || hub.fragment().is_some()
            || hub.path() != "/"
            || timeout.is_zero()
        {
            return Err(Error::InvalidInput(
                "Hub must be an HTTPS origin and timeout must be positive",
            ));
        }
        hub.set_path("/");
        let journal = journal.into();
        fs::create_dir_all(&journal)?;
        Ok(Self {
            hub,
            journal,
            timeout,
        })
    }

    /// Claims one correlation ID durably before sending. Existing claims only read back.
    /// A failed or unknown submission never invokes another transport or another send.
    pub fn dispatch(&self, task: &TaskDelivery, auth: &SendAuth) -> Result<DeliveryReceipt, Error> {
        let association = self.association(task, auth)?;
        let stem = self.journal.join(digest(task.correlation_id.as_bytes()));
        let _lock = lock(&stem.with_extension("lock"))?;
        let claim_path = stem.with_extension("claim.json");
        let receipt_path = stem.with_extension("receipt.json");
        let fresh = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&claim_path)
        {
            Ok(mut file) => {
                file.write_all(
                    &serde_json::to_vec(&association).map_err(|_| Error::CorruptJournal)?,
                )?;
                file.sync_all()?;
                File::open(&self.journal)?.sync_all()?;
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let claimed: Association = read_json(&claim_path)?;
                if claimed != association {
                    return Err(Error::AssociationConflict);
                }
                false
            }
            Err(error) => return Err(error.into()),
        };
        let receipt = if fresh {
            DeliveryReceipt {
                association,
                state: self.send(task, auth),
            }
        } else {
            self.recover(&receipt_path, association)?
        };
        save_receipt(&receipt_path, &receipt)?;
        Ok(receipt)
    }

    /// Recovers the original correlation without requiring expired send credentials or payload.
    pub fn reconcile(&self, correlation_id: &str) -> Result<DeliveryReceipt, Error> {
        if correlation_id.is_empty() {
            return Err(Error::InvalidInput("empty correlation ID"));
        }
        let stem = self.journal.join(digest(correlation_id.as_bytes()));
        let _lock = lock(&stem.with_extension("lock"))?;
        let association: Association = read_json(&stem.with_extension("claim.json"))?;
        if association.correlation_id != correlation_id || association.hub != self.hub.to_string() {
            return Err(Error::AssociationConflict);
        }
        let path = stem.with_extension("receipt.json");
        let receipt = self.recover(&path, association)?;
        save_receipt(&path, &receipt)?;
        Ok(receipt)
    }

    fn recover(&self, path: &Path, association: Association) -> Result<DeliveryReceipt, Error> {
        let mut receipt = if path.exists() {
            let receipt: DeliveryReceipt = read_json(path)?;
            if receipt.association != association {
                return Err(Error::AssociationConflict);
            }
            receipt
        } else {
            DeliveryReceipt {
                association,
                state: DeliveryState::UnknownNoRid,
            }
        };
        if let DeliveryState::Tracked { relay, observation } = &mut receipt.state {
            *observation = self.readback(relay, &receipt.association.delivery_handle);
        }
        Ok(receipt)
    }

    fn association(&self, task: &TaskDelivery, auth: &SendAuth) -> Result<Association, Error> {
        if task.correlation_id.is_empty() || task.task_id.is_empty() || task.envelope.is_empty() {
            return Err(Error::InvalidInput("empty task or envelope"));
        }
        if !binary(&task.delivery_handle, 16) {
            return Err(Error::InvalidInput("delivery handle"));
        }
        if !valid_base64(&auth.access) {
            return Err(Error::InvalidInput("access"));
        }
        if auth.home.as_ref().is_some_and(|v| v.is_empty()) {
            return Err(Error::InvalidInput("home"));
        }
        for value in std::iter::once(&auth.access).chain(auth.home.iter()) {
            HeaderValue::from_str(value).map_err(|_| Error::InvalidInput("access header"))?;
        }
        let proof_binding = match &auth.proof {
            AccessProof::Capability => serde_json::json!(["capability"]),
            AccessProof::Session {
                session_id, mac, ..
            } => {
                if !binary(session_id, 16) || !binary(mac, 32) {
                    return Err(Error::InvalidInput("session proof"));
                }
                serde_json::json!(["session", session_id])
            }
        };
        let binding =
            serde_json::to_vec(&serde_json::json!([auth.access, auth.home, proof_binding]))
                .map_err(|_| Error::InvalidInput("access binding"))?;
        Ok(Association {
            correlation_id: task.correlation_id.clone(),
            task_id: task.task_id.clone(),
            hub: self.hub.to_string(),
            auth_binding_sha256: digest(&binding),
            envelope_sha256: digest(&task.envelope),
            delivery_handle: task.delivery_handle.clone(),
            channel: Channel::Hub,
        })
    }

    fn send(&self, task: &TaskDelivery, auth: &SendAuth) -> DeliveryState {
        let Ok(client) = http_client(self.timeout) else {
            return DeliveryState::UnknownNoRid;
        };
        let mut request = client
            .post(self.hub.join("relay/send").expect("fixed path"))
            .header("Content-Type", "application/vnd.anet.envelope+cbor")
            .header("X-ANet-Wire", "3")
            .header("X-ANet-Access", &auth.access);
        if let Some(home) = &auth.home {
            request = request.header("X-ANet-Home", home);
        }
        if let AccessProof::Session {
            session_id,
            timestamp_ms,
            mac,
        } = &auth.proof
        {
            request = request
                .header("X-ANet-Session", session_id)
                .header("X-ANet-TS", timestamp_ms.to_string())
                .header("X-ANet-MAC", mac);
        }
        let result = request.body(task.envelope.clone()).send();
        let Ok(response) = result else {
            return DeliveryState::UnknownNoRid;
        };
        let status = response.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return DeliveryState::Rejected {
                http_status: status.as_u16(),
            };
        }
        if status.as_u16() != 200 {
            return DeliveryState::UnknownNoRid;
        }
        let Ok(response) = decode::<SendResponse>(response) else {
            return DeliveryState::UnknownNoRid;
        };
        if !matches!(response.status.as_str(), "queued" | "duplicate")
            || !binary(&response.rid, 16)
            || response.via_hub.as_ref().is_some_and(|v| v.is_empty())
        {
            return DeliveryState::UnknownNoRid;
        }
        DeliveryState::Tracked {
            relay: RelayReceipt {
                rid: response.rid,
                via_hub: response.via_hub,
            },
            observation: RelayObservation::Queued,
        }
    }

    fn readback(&self, relay: &RelayReceipt, handle: &str) -> RelayObservation {
        if !binary(&relay.rid, 16) || !binary(handle, 16) {
            return RelayObservation::ReadbackUnavailable;
        }
        // Status capabilities must not share the signed sender's connection or headers.
        let Ok(client) = http_client(self.timeout) else {
            return RelayObservation::ReadbackUnavailable;
        };
        let mut item = serde_json::json!({"rid": relay.rid, "handle": handle});
        if let Some(via_hub) = &relay.via_hub {
            item["via_hub"] = via_hub.clone().into();
        }
        let result = client
            .post(self.hub.join("relay/status").expect("fixed path"))
            .header("X-ANet-Wire", "3")
            .json(&serde_json::json!({"items": [item]}))
            .send();
        let Ok(response) = result else {
            return RelayObservation::ReadbackUnavailable;
        };
        if response.status().as_u16() != 200 {
            return RelayObservation::ReadbackUnavailable;
        }
        let Ok(mut response) = decode::<StatusResponse>(response) else {
            return RelayObservation::ReadbackUnavailable;
        };
        if response.items.len() != 1 {
            return RelayObservation::ReadbackUnavailable;
        }
        let item = response.items.remove(0);
        match (item.state.as_str(), item.dack) {
            ("queued", None) => RelayObservation::Queued,
            ("unknown", None) => RelayObservation::Unknown,
            ("done", Some(dack)) if binary(&dack, 80) => RelayObservation::DoneUnverified { dack },
            _ => RelayObservation::ReadbackUnavailable,
        }
    }
}

fn http_client(timeout: Duration) -> Result<Client, Error> {
    Client::builder()
        .timeout(timeout)
        .redirect(Policy::none())
        .no_proxy()
        .retry(reqwest::retry::never())
        .build()
        .map_err(|_| Error::ClientConfiguration)
}
fn lock(path: &Path) -> Result<File, Error> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    file.try_lock_exclusive().map_err(|e| {
        if e.kind() == std::io::ErrorKind::WouldBlock {
            Error::Busy
        } else {
            e.into()
        }
    })?;
    Ok(file)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_base64(value: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| !bytes.is_empty() && URL_SAFE_NO_PAD.encode(bytes) == value)
}
fn binary(value: &str, len: usize) -> bool {
    URL_SAFE_NO_PAD
        .decode(value)
        .is_ok_and(|bytes| bytes.len() == len && URL_SAFE_NO_PAD.encode(bytes) == value)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Error> {
    serde_json::from_slice(&fs::read(path)?).map_err(|_| Error::CorruptJournal)
}
fn save_receipt(path: &Path, receipt: &DeliveryReceipt) -> Result<(), Error> {
    let parent = path.parent().ok_or(Error::CorruptJournal)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, receipt).map_err(|_| Error::CorruptJournal)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| Error::Journal(e.error))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
fn decode<T: serde::de::DeserializeOwned>(response: reqwest::blocking::Response) -> Result<T, ()> {
    if response
        .headers()
        .get("X-ANet-Wire")
        .is_some_and(|value| value != "3")
    {
        return Err(());
    }
    let mut bytes = Vec::new();
    response
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() > 16 * 1024 {
        return Err(());
    }
    serde_json::from_slice(&bytes).map_err(|_| ())
}
#[derive(Deserialize)]
struct SendResponse {
    status: String,
    rid: String,
    via_hub: Option<String>,
}
#[derive(Deserialize)]
struct StatusResponse {
    items: Vec<StatusItem>,
}
#[derive(Deserialize)]
struct StatusItem {
    state: String,
    dack: Option<String>,
}

#[cfg(test)]
mod delivery_tier_tests {
    use super::*;
    fn receipt(state: DeliveryState) -> DeliveryReceipt {
        DeliveryReceipt {
            association: Association {
                correlation_id: "correlation".into(),
                task_id: "task".into(),
                hub: "https://anet.sonori.dev/".into(),
                auth_binding_sha256: "binding".into(),
                envelope_sha256: "envelope".into(),
                delivery_handle: "handle".into(),
                channel: Channel::Hub,
            },
            state,
        }
    }
    #[test]
    fn delivery_tier_transport_done_never_proves_task_completed_or_thread_visible() {
        for (observation, expected) in [
            (RelayObservation::Queued, DeliveryTier::Accepted),
            (RelayObservation::Unknown, DeliveryTier::Unknown),
            (
                RelayObservation::DoneUnverified {
                    dack: "token".into(),
                },
                DeliveryTier::Unknown,
            ),
            (RelayObservation::ReadbackUnavailable, DeliveryTier::Unknown),
        ] {
            let r = receipt(DeliveryState::Tracked {
                relay: RelayReceipt {
                    rid: "rid".into(),
                    via_hub: None,
                },
                observation,
            });
            assert_eq!(r.tier(), expected);
            let value = serde_json::to_value(&r).unwrap();
            assert_eq!(
                serde_json::from_value::<DeliveryReceipt>(value.clone()).unwrap(),
                r
            );
            for unsupported in ["thread-visible", "running", "completed"] {
                let mut forged = value.clone();
                forged["tier"] = serde_json::json!(unsupported);
                assert!(serde_json::from_value::<DeliveryReceipt>(forged).is_err());
            }
        }
        assert_eq!(
            receipt(DeliveryState::UnknownNoRid).tier(),
            DeliveryTier::Unknown
        );
        assert_eq!(
            receipt(DeliveryState::Rejected { http_status: 401 }).tier(),
            DeliveryTier::Failed
        );
    }
}
