use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use fleet_hub::*;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Response {
    code: u16,
    body: String,
    delay: Duration,
    location: Option<String>,
    wire: Option<u8>,
}
impl Response {
    fn json(body: Value) -> Self {
        Self {
            code: 200,
            body: body.to_string(),
            delay: Duration::ZERO,
            location: None,
            wire: None,
        }
    }
    fn delayed(mut self) -> Self {
        self.delay = Duration::from_millis(250);
        self
    }
}
#[derive(Clone, Debug)]
struct Request {
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    connection_port: u16,
}
struct Hub {
    url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Hub {
    fn new(responses: Vec<Response>) -> Self {
        Self::with_keepalive(responses, false)
    }
    fn with_keepalive(responses: Vec<Response>, keepalive: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let handle = thread::spawn(move || {
            let mut responses = VecDeque::from(responses);
            let mut workers = Vec::new();
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let requests = captured.clone();
                        let response = responses.pop_front().unwrap_or(Response {
                            code: 500,
                            body: "unexpected request".into(),
                            delay: Duration::ZERO,
                            location: None,
                            wire: None,
                        });
                        workers.push(thread::spawn(move || {
                            for _ in 0..if keepalive { 2 } else { 1 } {
                            let Ok(request) = read_request(&mut stream) else { break; };
                            requests.lock().unwrap().push(request);
                            thread::sleep(response.delay);
                            let location = response.location.as_ref().map(|url| format!("Location: {url}\r\n")).unwrap_or_default();
                            let wire_header = response.wire.map(|v| format!("X-ANet-Wire: {v}\r\n")).unwrap_or_default();
                            let wire = format!("HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {}\r\n{}{}\r\n{}", response.code, response.body.len(), if keepalive { "keep-alive" } else { "close" }, location, wire_header, response.body);
                            let _ = stream.write_all(wire.as_bytes());
                            }
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(handle),
        }
    }
    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
    fn wait_for_request(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.requests().is_empty() {
            assert!(Instant::now() < deadline, "client never reached fixture");
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn client(&self, journal: &tempfile::TempDir) -> HubClient {
        HubClient::new(&self.url, journal.path(), Duration::from_millis(100)).unwrap()
    }
}
impl Drop for Hub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}
fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte)?;
        head.push(byte[0]);
        assert!(head.len() <= 16 * 1024);
    }
    let text = String::from_utf8(head).unwrap();
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap();
    assert!(first.starts_with("POST "));
    let path = first.split_whitespace().nth(1).unwrap().into();
    let headers: BTreeMap<_, _> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.to_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers["content-length"].parse().unwrap();
    let mut body = vec![0; length];
    stream.read_exact(&mut body)?;
    Ok(Request {
        path,
        headers,
        body,
        connection_port: stream.peer_addr()?.port(),
    })
}
fn encoded(len: usize) -> String {
    URL_SAFE_NO_PAD.encode(vec![7; len])
}
fn task() -> TaskDelivery {
    TaskDelivery {
        correlation_id: "operation-1".into(),
        task_id: "task-1".into(),
        envelope: vec![0xa1, 7, 0x41, 1],
        delivery_handle: encoded(16),
    }
}
fn capability() -> SendAuth {
    SendAuth {
        access: URL_SAFE_NO_PAD.encode([1]),
        home: None,
        proof: AccessProof::Capability,
    }
}
fn queued(status: &str) -> Response {
    Response::json(json!({"status":status,"rid":encoded(16),"recipient_quiet":false}))
}
fn observation(receipt: &DeliveryReceipt) -> &RelayObservation {
    match &receipt.state {
        DeliveryState::Tracked { observation, .. } => observation,
        state => panic!("not tracked: {state:?}"),
    }
}
fn assert_single_send(hub: &Hub, status_count: usize) {
    let requests = hub.requests();
    assert_eq!(
        requests.iter().filter(|r| r.path == "/relay/send").count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path == "/relay/status")
            .count(),
        status_count
    );
    assert_eq!(requests.len(), status_count + 1);
}

#[test]
fn duplicate_response_restarts_as_anonymous_correlated_readback() {
    let hub = Hub::new(vec![
        queued("duplicate"),
        Response::json(json!({"items":[{"state":"done","dack":encoded(80)}]})),
    ]);
    let journal = tempfile::tempdir().unwrap();
    let first = hub
        .client(&journal)
        .dispatch(&task(), &capability())
        .unwrap();
    assert_eq!(observation(&first), &RelayObservation::Queued);
    let recovered = hub.client(&journal).reconcile("operation-1").unwrap();
    assert_eq!(first.association, recovered.association);
    assert_eq!(recovered.association.correlation_id, "operation-1");
    assert_eq!(recovered.association.task_id, "task-1");
    assert_eq!(
        observation(&recovered),
        &RelayObservation::DoneUnverified { dack: encoded(80) }
    );
    assert_single_send(&hub, 1);
    let requests = hub.requests();
    assert_eq!(requests[0].body, task().envelope);
    assert_eq!(
        requests[0].headers["content-type"],
        "application/vnd.anet.envelope+cbor"
    );
    assert_eq!(requests[0].headers["x-anet-access"], capability().access);
    assert_eq!(requests[1].headers["x-anet-wire"], "3");
    for request in &requests {
        assert_eq!(request.headers["x-anet-wire"], "3");
        for forbidden in [
            "authorization",
            "x-anet-aid",
            "x-anet-seq",
            "x-anet-sig",
            "x-anet-session",
            "x-anet-mac",
            "x-anet-ts",
        ] {
            assert!(!request.headers.contains_key(forbidden));
        }
    }
    assert!(!requests[1].headers.contains_key("x-anet-access"));
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
        json!({"items":[{"rid":encoded(16),"handle":encoded(16)}]})
    );
}

#[test]
fn first_contact_headers_are_isolated_from_status() {
    let mut answer = queued("queued");
    answer.body = json!({"status":"queued","rid":encoded(16),"via_hub":"hub-aid"}).to_string();
    let hub = Hub::new(vec![
        answer,
        Response::json(json!({"items":[{"state":"queued"}]})),
    ]);
    let journal = tempfile::tempdir().unwrap();
    let auth = SendAuth {
        access: capability().access,
        home: Some("home-aid".into()),
        proof: AccessProof::Session {
            session_id: encoded(16),
            timestamp_ms: 1234,
            mac: encoded(32),
        },
    };
    let client = hub.client(&journal);
    client.dispatch(&task(), &auth).unwrap();
    client.reconcile("operation-1").unwrap();
    let requests = hub.requests();
    assert_eq!(requests[0].headers["x-anet-session"], encoded(16));
    assert_eq!(requests[0].headers["x-anet-ts"], "1234");
    assert_eq!(requests[0].headers["x-anet-mac"], encoded(32));
    assert_eq!(requests[0].headers["x-anet-home"], "home-aid");
    assert_eq!(
        requests[1]
            .headers
            .keys()
            .filter(|k| k.starts_with("x-anet-"))
            .collect::<Vec<_>>(),
        vec!["x-anet-wire"]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[1].body).unwrap()["items"][0]["via_hub"],
        "hub-aid"
    );
    for entry in std::fs::read_dir(journal.path()).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let content = std::fs::read_to_string(path).unwrap();
        assert!(!content.contains(&encoded(32)), "MAC persisted");
        assert!(!content.contains("session_id"));
        assert!(!content.contains("access\""));
    }
}

#[test]
fn auth_rejections_remain_one_attempt_after_restart() {
    for code in [401, 403] {
        let hub = Hub::new(vec![Response {
            code,
            body: "denied".into(),
            delay: Duration::ZERO,
            location: None,
            wire: None,
        }]);
        let journal = tempfile::tempdir().unwrap();
        let receipt = hub
            .client(&journal)
            .dispatch(&task(), &capability())
            .unwrap();
        assert_eq!(receipt.state, DeliveryState::Rejected { http_status: code });
        assert_eq!(
            hub.client(&journal)
                .dispatch(&task(), &capability())
                .unwrap(),
            receipt
        );
        assert_eq!(
            hub.client(&journal).reconcile("operation-1").unwrap(),
            receipt
        );
        assert_single_send(&hub, 0);
    }
}

#[test]
fn send_timeout_with_lost_rid_never_invents_a_status_lookup_or_resends() {
    let hub = Hub::new(vec![queued("queued").delayed()]);
    let journal = tempfile::tempdir().unwrap();
    let receipt = hub
        .client(&journal)
        .dispatch(&task(), &capability())
        .unwrap();
    assert_eq!(receipt.state, DeliveryState::UnknownNoRid);
    assert_eq!(
        hub.client(&journal)
            .dispatch(&task(), &capability())
            .unwrap(),
        receipt
    );
    assert_eq!(
        hub.client(&journal).reconcile("operation-1").unwrap(),
        receipt
    );
    assert_single_send(&hub, 0);
}

#[test]
fn status_timeout_and_unknown_only_allow_further_readback() {
    let hub = Hub::new(vec![
        queued("queued"),
        Response::json(json!({"items":[{"state":"done","dack":encoded(80)}]})).delayed(),
        Response::json(json!({"items":[{"state":"unknown"}]})),
        Response::json(json!({"items":[{"state":"queued"}]})),
    ]);
    let journal = tempfile::tempdir().unwrap();
    let first = hub
        .client(&journal)
        .dispatch(&task(), &capability())
        .unwrap();
    let timed_out = hub.client(&journal).reconcile("operation-1").unwrap();
    assert_eq!(
        observation(&timed_out),
        &RelayObservation::ReadbackUnavailable
    );
    assert_eq!(
        observation(
            &hub.client(&journal)
                .dispatch(&task(), &capability())
                .unwrap()
        ),
        &RelayObservation::Unknown
    );
    let recovered = hub.client(&journal).reconcile("operation-1").unwrap();
    assert_eq!(observation(&recovered), &RelayObservation::Queued);
    assert_eq!(first.association, recovered.association);
    assert_single_send(&hub, 3);
}

#[test]
fn association_conflicts_make_no_second_network_call() {
    let hub = Hub::new(vec![queued("queued")]);
    let journal = tempfile::tempdir().unwrap();
    hub.client(&journal)
        .dispatch(&task(), &capability())
        .unwrap();
    for field in ["envelope", "task", "handle", "auth", "hub"] {
        let mut altered = task();
        let mut auth = capability();
        match field {
            "envelope" => altered.envelope.push(1),
            "task" => altered.task_id = "other-task".into(),
            "handle" => altered.delivery_handle = URL_SAFE_NO_PAD.encode([8; 16]),
            "auth" => auth.access = URL_SAFE_NO_PAD.encode([2]),
            _ => (),
        }
        let client = if field == "hub" {
            HubClient::new(
                "https://other.invalid",
                journal.path(),
                Duration::from_millis(100),
            )
            .unwrap()
        } else {
            hub.client(&journal)
        };
        assert!(
            matches!(
                client.dispatch(&altered, &auth),
                Err(Error::AssociationConflict)
            ),
            "{field}"
        );
    }
    assert_single_send(&hub, 0);
}

#[test]
fn concurrent_clients_claim_before_the_first_network_write() {
    let hub = Hub::new(vec![queued("queued").delayed()]);
    let journal = tempfile::tempdir().unwrap();
    let first = hub.client(&journal);
    let worker = thread::spawn(move || first.dispatch(&task(), &capability()).unwrap());
    hub.wait_for_request();
    assert!(matches!(
        hub.client(&journal).dispatch(&task(), &capability()),
        Err(Error::Busy)
    ));
    assert_eq!(worker.join().unwrap().state, DeliveryState::UnknownNoRid);
    assert_eq!(
        hub.client(&journal).reconcile("operation-1").unwrap().state,
        DeliveryState::UnknownNoRid
    );
    assert_single_send(&hub, 0);
}

#[test]
fn malformed_send_responses_never_restore_send_eligibility() {
    for body in [
        json!({"status":"queued","rid":"bad"}),
        json!({"status":"forwarded","rid":encoded(16)}),
        json!({"status":"queued"}),
    ] {
        let hub = Hub::new(vec![Response::json(body)]);
        let journal = tempfile::tempdir().unwrap();
        assert_eq!(
            hub.client(&journal)
                .dispatch(&task(), &capability())
                .unwrap()
                .state,
            DeliveryState::UnknownNoRid
        );
        assert_eq!(
            hub.client(&journal).reconcile("operation-1").unwrap().state,
            DeliveryState::UnknownNoRid
        );
        assert_single_send(&hub, 0);
    }
}

#[test]
fn malformed_status_count_or_token_cannot_claim_delivery() {
    for body in [
        json!({"items":[]}),
        json!({"items":[{"state":"queued"},{"state":"queued"}]}),
        json!({"items":[{"state":"done","dack":"bad"}]}),
        json!({"items":[{"state":"completed"}]}),
    ] {
        let hub = Hub::new(vec![queued("queued"), Response::json(body)]);
        let journal = tempfile::tempdir().unwrap();
        hub.client(&journal)
            .dispatch(&task(), &capability())
            .unwrap();
        assert_eq!(
            observation(&hub.client(&journal).reconcile("operation-1").unwrap()),
            &RelayObservation::ReadbackUnavailable
        );
        assert_single_send(&hub, 1);
    }
}

#[test]
fn redirect_is_not_followed() {
    let target = Hub::new(vec![]);
    let hub = Hub::new(vec![Response {
        code: 307,
        body: "".into(),
        delay: Duration::ZERO,
        location: Some(format!("{}/relay/send", target.url)),
        wire: None,
    }]);
    let journal = tempfile::tempdir().unwrap();
    assert_eq!(
        hub.client(&journal)
            .dispatch(&task(), &capability())
            .unwrap()
            .state,
        DeliveryState::UnknownNoRid
    );
    assert_single_send(&hub, 0);
    assert!(target.requests().is_empty());
}

#[test]
fn incomplete_claim_is_fail_closed_after_restart() {
    let hub = Hub::new(vec![queued("queued")]);
    let journal = tempfile::tempdir().unwrap();
    hub.client(&journal)
        .dispatch(&task(), &capability())
        .unwrap();
    let claim = std::fs::read_dir(journal.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with("claim.json"))
        .unwrap();
    std::fs::write(claim, "{").unwrap();
    assert!(matches!(
        hub.client(&journal).dispatch(&task(), &capability()),
        Err(Error::CorruptJournal)
    ));
    assert_single_send(&hub, 0);
}

#[test]
fn session_and_capability_sends_use_separate_connections() {
    let hub = Hub::with_keepalive(vec![queued("queued"), queued("queued")], true);
    let journal = tempfile::tempdir().unwrap();
    let client = hub.client(&journal);
    let auth = SendAuth {
        access: capability().access,
        home: None,
        proof: AccessProof::Session {
            session_id: encoded(16),
            timestamp_ms: 1234,
            mac: encoded(32),
        },
    };
    client.dispatch(&task(), &auth).unwrap();
    let mut second = task();
    second.correlation_id = "operation-2".into();
    second.task_id = "task-2".into();
    client.dispatch(&second, &capability()).unwrap();
    let requests = hub.requests();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].connection_port, requests[1].connection_port);
}

#[test]
fn unverified_done_can_be_read_again_for_core_verification() {
    let hub = Hub::new(vec![
        queued("queued"),
        Response::json(json!({"items":[{"state":"done","dack":encoded(80)}]})),
        Response::json(json!({"items":[{"state":"done","dack":URL_SAFE_NO_PAD.encode([9;80])}]})),
    ]);
    let journal = tempfile::tempdir().unwrap();
    let client = hub.client(&journal);
    client.dispatch(&task(), &capability()).unwrap();
    client.reconcile("operation-1").unwrap();
    let receipt = client.reconcile("operation-1").unwrap();
    assert_eq!(
        observation(&receipt),
        &RelayObservation::DoneUnverified {
            dack: URL_SAFE_NO_PAD.encode([9; 80])
        }
    );
    assert_single_send(&hub, 2);
}

#[test]
fn conflicting_response_wire_version_is_rejected() {
    let mut wrong_send = queued("queued");
    wrong_send.wire = Some(2);
    let hub = Hub::new(vec![wrong_send]);
    let journal = tempfile::tempdir().unwrap();
    assert_eq!(
        hub.client(&journal)
            .dispatch(&task(), &capability())
            .unwrap()
            .state,
        DeliveryState::UnknownNoRid
    );
    assert_single_send(&hub, 0);
    let mut wrong_status = Response::json(json!({"items":[{"state":"done","dack":encoded(80)}]}));
    wrong_status.wire = Some(2);
    let hub = Hub::new(vec![queued("queued"), wrong_status]);
    let journal = tempfile::tempdir().unwrap();
    hub.client(&journal)
        .dispatch(&task(), &capability())
        .unwrap();
    assert_eq!(
        observation(&hub.client(&journal).reconcile("operation-1").unwrap()),
        &RelayObservation::ReadbackUnavailable
    );
    assert_single_send(&hub, 1);
}

#[test]
fn smoke_restart_readback_preserves_task_without_claiming_completion() {
    let hub = Hub::new(vec![
        queued("queued"),
        Response::json(json!({"items":[{"state":"done","dack":encoded(80)}]})),
    ]);
    let journal = tempfile::tempdir().unwrap();
    let submitted = hub
        .client(&journal)
        .dispatch(&task(), &capability())
        .unwrap();
    assert_eq!(submitted.tier(), fleet_proto::DeliveryTier::Accepted);

    // Restart with only the durable correlation; no send credentials or envelope needed.
    let recovered = hub.client(&journal).reconcile("operation-1").unwrap();
    assert_eq!(recovered.association, submitted.association);
    assert_eq!(recovered.tier(), fleet_proto::DeliveryTier::Unknown);
    assert_eq!(
        observation(&recovered),
        &RelayObservation::DoneUnverified { dack: encoded(80) }
    );
    let stored = std::fs::read_dir(journal.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with("receipt.json"))
        .unwrap();
    let persisted: DeliveryReceipt =
        serde_json::from_slice(&std::fs::read(stored).unwrap()).unwrap();
    assert_eq!(persisted, recovered);
    assert_single_send(&hub, 1);
    let requests = hub.requests();
    assert_eq!(
        requests[1]
            .headers
            .keys()
            .filter(|key| key.starts_with("x-anet-"))
            .collect::<Vec<_>>(),
        vec!["x-anet-wire"]
    );
}
