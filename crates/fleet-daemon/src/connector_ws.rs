use crate::{
    build_configured_host_adapter, observer::project_host_timelines, FleetLedgerStore,
    HostAdapterError, HostRuntimeOptions, SandboxFsError, SandboxFsHandler,
};
use fleet_proto::{
    timeline::{CanonicalSessionId, TimelineItem},
    workbench::{
        ConnectorRpcFrame, EffectiveCapabilities, IngestBatchResult, IngestRejection,
        InteractionStatus, MAX_CONNECTOR_WS_FRAME_BYTES,
    },
    CreateSessionRequest, DeliverTurnRequest, DesktopHostKind, GROKBOT_APP_DISPLAY_NAME,
    GROKBOT_BUNDLE_ID,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    env,
    ffi::{OsStr, OsString},
    fmt,
    io::{self, BufRead, BufReader, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{
    client::IntoClientRequest,
    http::HeaderValue,
    protocol::WebSocketConfig,
    Message, WebSocket,
};

pub const STANDARD_SEMAPHORE_CAPACITY: usize = 8;
pub const CRITICAL_SEMAPHORE_CAPACITY: usize = 2;

#[derive(Debug)]
pub enum ConnectorError {
    InvalidConfig(String),
    Transport(String),
    HttpStatus { status: u16, body: String },
    Protocol(String),
    StaleGeneration { active: u64, received: u64 },
    HostAdapter(HostAdapterError),
    Sandbox(SandboxFsError),
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(msg) => write!(f, "invalid connector configuration: {msg}"),
            Self::Transport(msg) => write!(f, "connector transport error: {msg}"),
            Self::HttpStatus { status, body } => {
                write!(f, "connector ingest HTTP {status}: {body}")
            }
            Self::Protocol(msg) => write!(f, "connector protocol error: {msg}"),
            Self::StaleGeneration { active, received } => write!(
                f,
                "stale_generation_fenced: active generation is {active}, received {received}"
            ),
            Self::HostAdapter(err) => write!(f, "connector host adapter error: {err}"),
            Self::Sandbox(err) => write!(f, "connector sandbox error: {err}"),
        }
    }
}

impl std::error::Error for ConnectorError {}

impl From<HostAdapterError> for ConnectorError {
    fn from(value: HostAdapterError) -> Self {
        Self::HostAdapter(value)
    }
}

impl From<SandboxFsError> for ConnectorError {
    fn from(value: SandboxFsError) -> Self {
        Self::Sandbox(value)
    }
}

/// Counting semaphore built on `std::sync::{Mutex, Condvar}` with zero external runtime dependencies.
#[derive(Debug)]
pub struct CountingSemaphore {
    capacity: usize,
    in_flight: Mutex<usize>,
    cvar: Condvar,
}

pub struct SemaphorePermit<'a> {
    sem: &'a CountingSemaphore,
}

impl Drop for SemaphorePermit<'_> {
    fn drop(&mut self) {
        let mut guard = self.sem.in_flight.lock().expect("semaphore mutex poisoned");
        *guard = guard.saturating_sub(1);
        self.sem.cvar.notify_one();
    }
}

impl CountingSemaphore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            in_flight: Mutex::new(0),
            cvar: Condvar::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn in_flight(&self) -> usize {
        *self.in_flight.lock().expect("semaphore mutex poisoned")
    }

    pub fn acquire(&self) -> SemaphorePermit<'_> {
        let mut guard = self.in_flight.lock().expect("semaphore mutex poisoned");
        while *guard >= self.capacity {
            guard = self.cvar.wait(guard).expect("semaphore condvar poisoned");
        }
        *guard += 1;
        SemaphorePermit { sem: self }
    }

    pub fn try_acquire(&self) -> Option<SemaphorePermit<'_>> {
        let mut guard = self.in_flight.lock().expect("semaphore mutex poisoned");
        if *guard < self.capacity {
            *guard += 1;
            Some(SemaphorePermit { sem: self })
        } else {
            None
        }
    }
}

/// Dual-lane concurrency gate: 8 standard slots (`runtime.*`, `session.*`, `workspace.*`)
/// and 2 critical slots (`interaction.respond`, `session.interrupt`).
#[derive(Debug)]
pub struct ConnectorConcurrencyGate {
    standard: CountingSemaphore,
    critical: CountingSemaphore,
}

impl Default for ConnectorConcurrencyGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectorConcurrencyGate {
    pub fn new() -> Self {
        Self {
            standard: CountingSemaphore::new(STANDARD_SEMAPHORE_CAPACITY),
            critical: CountingSemaphore::new(CRITICAL_SEMAPHORE_CAPACITY),
        }
    }

    pub fn standard(&self) -> &CountingSemaphore {
        &self.standard
    }

    pub fn critical(&self) -> &CountingSemaphore {
        &self.critical
    }

    pub fn acquire_for_frame(&self, frame: &ConnectorRpcFrame) -> SemaphorePermit<'_> {
        if frame.is_critical_lane() {
            self.critical.acquire()
        } else {
            self.standard.acquire()
        }
    }
}

/// Monotonic lease generation fence preventing split-brain RPC execution across reconnects.
#[derive(Clone, Debug, Default)]
pub struct GenerationFence {
    active_generation: Arc<AtomicU64>,
}

impl GenerationFence {
    pub fn new(initial: u64) -> Self {
        Self {
            active_generation: Arc::new(AtomicU64::new(initial)),
        }
    }

    pub fn current(&self) -> u64 {
        self.active_generation.load(Ordering::SeqCst)
    }

    pub fn advance(&self, next: u64) -> Result<u64, ConnectorError> {
        let mut cur = self.active_generation.load(Ordering::SeqCst);
        loop {
            if next < cur {
                return Err(ConnectorError::StaleGeneration {
                    active: cur,
                    received: next,
                });
            }
            match self.active_generation.compare_exchange(
                cur,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Ok(next),
                Err(actual) => cur = actual,
            }
        }
    }

    pub fn verify(&self, frame_generation: u64) -> Result<(), ConnectorError> {
        let active = self.current();
        if frame_generation < active {
            return Err(ConnectorError::StaleGeneration {
                active,
                received: frame_generation,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeProviderDescriptor {
    pub provider_id: String,
    pub display_name: String,
    pub bundle_id: String,
    pub online: bool,
    pub capabilities: EffectiveCapabilities,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineEmitTransport {
    WebSocket,
    HttpIngestSpillover,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimelineEmitOutcome {
    pub transport: TimelineEmitTransport,
    pub wire_bytes: usize,
    pub accepted: usize,
    pub rejected: Vec<IngestRejection>,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointScheme {
    Plain,
    Tls,
}

impl EndpointScheme {
    pub fn is_tls(self) -> bool {
        matches!(self, Self::Tls)
    }

    pub fn ws_scheme(self) -> &'static str {
        match self {
            Self::Plain => "ws",
            Self::Tls => "wss",
        }
    }

    pub fn http_scheme(self) -> &'static str {
        match self {
            Self::Plain => "http",
            Self::Tls => "https",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Plain => 80,
            Self::Tls => 443,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkbenchEndpoint {
    scheme: EndpointScheme,
    host: String,
    port: u16,
    explicit_port: bool,
    ws_path: String,
    http_base_path: String,
}

impl WorkbenchEndpoint {
    pub fn parse(raw_url: &str) -> Result<Self, ConnectorError> {
        let trimmed = raw_url.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(ConnectorError::InvalidConfig(
                "server_url must not be empty".into(),
            ));
        }
        let (scheme, is_ws_input, rest) = if let Some(r) = trimmed.strip_prefix("wss://") {
            (EndpointScheme::Tls, true, r)
        } else if let Some(r) = trimmed.strip_prefix("https://") {
            (EndpointScheme::Tls, false, r)
        } else if let Some(r) = trimmed.strip_prefix("ws://") {
            (EndpointScheme::Plain, true, r)
        } else if let Some(r) = trimmed.strip_prefix("http://") {
            (EndpointScheme::Plain, false, r)
        } else {
            return Err(ConnectorError::InvalidConfig(format!(
                "unsupported server_url scheme: {trimmed}"
            )));
        };

        let (authority, raw_path) = match rest.split_once('/') {
            Some((a, p)) => (a, format!("/{}", p.trim_matches('/'))),
            None => (rest, String::new()),
        };
        if authority.trim().is_empty() {
            return Err(ConnectorError::InvalidConfig(format!(
                "missing host in server_url: {trimmed}"
            )));
        }

        let (host, port, explicit_port) = if let Some((h, port_str)) = authority.rsplit_once(':') {
            if h.trim().is_empty() {
                return Err(ConnectorError::InvalidConfig(format!(
                    "missing host in server_url: {trimmed}"
                )));
            }
            let p = port_str.parse::<u16>().map_err(|_| {
                ConnectorError::InvalidConfig(format!("invalid port in {authority}"))
            })?;
            (h.to_string(), p, true)
        } else {
            (authority.to_string(), scheme.default_port(), false)
        };

        let normalized_path = if raw_path == "/" {
            String::new()
        } else {
            raw_path
        };

        let (http_base_path, ws_path) =
            if let Some(prefix) = normalized_path.strip_suffix("/api/v2/connector/ws") {
                (prefix.to_string(), normalized_path)
            } else if let Some(prefix) = normalized_path.strip_suffix("/ws/v2/connector") {
                (prefix.to_string(), normalized_path)
            } else if is_ws_input && !normalized_path.is_empty() {
                (String::new(), normalized_path)
            } else {
                (
                    normalized_path.clone(),
                    format!("{normalized_path}/api/v2/connector/ws"),
                )
            };

        Ok(Self {
            scheme,
            host,
            port,
            explicit_port,
            ws_path,
            http_base_path,
        })
    }

    pub fn scheme(&self) -> EndpointScheme {
        self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn host_header(&self) -> String {
        if self.explicit_port || self.port != self.scheme.default_port() {
            format!("{}:{}", self.host, self.port)
        } else {
            self.host.clone()
        }
    }

    pub fn ws_url(&self) -> String {
        format!(
            "{}://{}{}",
            self.scheme.ws_scheme(),
            self.host_header(),
            self.ws_path
        )
    }

    pub fn http_url(&self, path: &str) -> String {
        let normalized = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        format!(
            "{}://{}{}{}",
            self.scheme.http_scheme(),
            self.host_header(),
            self.http_base_path,
            normalized
        )
    }

    pub fn resolve_socket_addr(&self) -> Result<SocketAddr, ConnectorError> {
        let addr_str = format!("{}:{}", self.host, self.port);
        addr_str
            .to_socket_addrs()
            .map_err(|e| {
                ConnectorError::Transport(format!("cannot resolve {addr_str}: {e}"))
            })?
            .next()
            .ok_or_else(|| {
                ConnectorError::Transport(format!("no socket addresses resolved for {addr_str}"))
            })
    }
}

struct TlsBridgeGuard {
    child: Child,
}

impl Drop for TlsBridgeGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct ConnectorStream {
    inner: TcpStream,
    _tls_guard: Option<TlsBridgeGuard>,
}

impl ConnectorStream {
    pub fn connect(
        endpoint: &WorkbenchEndpoint,
        timeout: Duration,
    ) -> Result<Self, ConnectorError> {
        let sock_addr = endpoint.resolve_socket_addr()?;
        match endpoint.scheme() {
            EndpointScheme::Plain => {
                let stream = TcpStream::connect_timeout(&sock_addr, timeout).map_err(|e| {
                    ConnectorError::Transport(format!(
                        "TCP connect {}:{}: {e}",
                        endpoint.host(),
                        endpoint.port()
                    ))
                })?;
                stream
                    .set_nodelay(true)
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?;
                stream
                    .set_read_timeout(Some(timeout))
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?;
                stream
                    .set_write_timeout(Some(timeout))
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?;
                Ok(Self {
                    inner: stream,
                    _tls_guard: None,
                })
            }
            EndpointScheme::Tls => {
                let probe = TcpStream::connect_timeout(&sock_addr, timeout).map_err(|e| {
                    ConnectorError::Transport(format!(
                        "TLS target TCP connect {}:{}: {e}",
                        endpoint.host(),
                        endpoint.port()
                    ))
                })?;
                drop(probe);

                let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| {
                    ConnectorError::Transport(format!("bind TLS bridge listener: {e}"))
                })?;
                let local_addr = listener.local_addr().map_err(|e| {
                    ConnectorError::Transport(format!("TLS bridge local_addr: {e}"))
                })?;

                let connect_target = format!("{}:{}", endpoint.host(), endpoint.port());
                let mut cmd = Command::new("openssl");
                cmd.args([
                    "s_client",
                    "-quiet",
                    "-no_ign_eof",
                    "-connect",
                    &connect_target,
                    "-servername",
                    endpoint.host(),
                ]);
                if let Ok(ca_file) = env::var("FLEET_TLS_CA_CERT") {
                    if !ca_file.trim().is_empty() {
                        cmd.args(["-CAfile", ca_file.trim()]);
                    }
                }
                let mut child = cmd
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| {
                        ConnectorError::Transport(format!("spawn openssl s_client: {e}"))
                    })?;

                let mut child_stdin = child.stdin.take().ok_or_else(|| {
                    ConnectorError::Transport("openssl s_client stdin unavailable".into())
                })?;
                let mut child_stdout = child.stdout.take().ok_or_else(|| {
                    ConnectorError::Transport("openssl s_client stdout unavailable".into())
                })?;

                let client_stream =
                    TcpStream::connect_timeout(&local_addr, timeout).map_err(|e| {
                        ConnectorError::Transport(format!("connect local TLS bridge: {e}"))
                    })?;
                let (bridge_peer, _) = listener.accept().map_err(|e| {
                    ConnectorError::Transport(format!("accept local TLS bridge: {e}"))
                })?;
                let _ = client_stream.set_nodelay(true);
                let _ = bridge_peer.set_nodelay(true);
                client_stream
                    .set_read_timeout(Some(timeout))
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?;
                client_stream
                    .set_write_timeout(Some(timeout))
                    .map_err(|e| ConnectorError::Transport(e.to_string()))?;

                let mut peer_read = bridge_peer.try_clone().map_err(|e| {
                    ConnectorError::Transport(format!("clone TLS bridge socket: {e}"))
                })?;
                let mut peer_write = bridge_peer;

                thread::spawn(move || {
                    let mut buf = [0u8; 16 * 1024];
                    while let Ok(n) = peer_read.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        if child_stdin.write_all(&buf[..n]).is_err()
                            || child_stdin.flush().is_err()
                        {
                            break;
                        }
                    }
                    let _ = child_stdin.flush();
                });

                thread::spawn(move || {
                    let mut buf = [0u8; 16 * 1024];
                    while let Ok(n) = child_stdout.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        if peer_write.write_all(&buf[..n]).is_err()
                            || peer_write.flush().is_err()
                        {
                            break;
                        }
                    }
                    let _ = peer_write.shutdown(Shutdown::Both);
                });

                Ok(Self {
                    inner: client_stream,
                    _tls_guard: Some(TlsBridgeGuard { child }),
                })
            }
        }
    }

    pub fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.inner.set_read_timeout(dur)
    }

    pub fn set_write_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        self.inner.set_write_timeout(dur)
    }

    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        self.inner.set_nodelay(nodelay)
    }
}

impl Read for ConnectorStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for ConnectorStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Keeps plain HTTP on in-process TCP to avoid `curl` fork/exec overhead against the 10ms P99 loopback gate.
pub fn execute_workbench_http_raw(
    endpoint: &WorkbenchEndpoint,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<(u16, String), ConnectorError> {
    let normalized_path = if path.starts_with('/') {
        format!("{}{}", endpoint.http_base_path, path)
    } else {
        format!("{}/{}", endpoint.http_base_path, path)
    };
    match endpoint.scheme() {
        EndpointScheme::Plain => {
            let sock_addr = endpoint.resolve_socket_addr()?;
            let mut stream = TcpStream::connect_timeout(&sock_addr, timeout).map_err(|e| {
                ConnectorError::Transport(format!("HTTP connect {sock_addr}: {e}"))
            })?;
            let _ = stream.set_nodelay(true);
            stream
                .set_read_timeout(Some(timeout))
                .map_err(|e| ConnectorError::Transport(e.to_string()))?;
            stream
                .set_write_timeout(Some(timeout))
                .map_err(|e| ConnectorError::Transport(e.to_string()))?;

            let payload = body.unwrap_or(&[]);
            let mut req_header = format!(
                "{method} {normalized_path} HTTP/1.1\r\nHost: {}\r\n",
                endpoint.host_header()
            );
            for (k, v) in headers {
                req_header.push_str(&format!("{k}: {v}\r\n"));
            }
            if body.is_some() {
                req_header.push_str(&format!("Content-Length: {}\r\n", payload.len()));
            }
            req_header.push_str("Connection: close\r\n\r\n");

            stream
                .write_all(req_header.as_bytes())
                .and_then(|_| {
                    if !payload.is_empty() {
                        stream.write_all(payload)
                    } else {
                        Ok(())
                    }
                })
                .and_then(|_| stream.flush())
                .map_err(|e| ConnectorError::Transport(format!("HTTP write failed: {e}")))?;

            read_http_response(&mut stream)
        }
        EndpointScheme::Tls => {
            let url = endpoint.http_url(path);
            let max_time = timeout.as_secs_f64().max(1.0);
            let mut cmd = Command::new("curl");
            cmd.args([
                "-sS",
                "--max-time",
                &format!("{max_time:.1}"),
                "-X",
                method,
            ]);
            if let Ok(ca_file) = env::var("FLEET_TLS_CA_CERT") {
                if !ca_file.trim().is_empty() {
                    cmd.args(["--cacert", ca_file.trim()]);
                }
            } else if env::var("FLEET_TLS_INSECURE")
                .is_ok_and(|v| v == "1" || v == "true")
                || endpoint.host() == "127.0.0.1"
                || endpoint.host() == "localhost"
            {
                cmd.arg("-k");
            }
            for (k, v) in headers {
                cmd.args(["-H", &format!("{k}: {v}")]);
            }
            if body.is_some() {
                cmd.args(["--data-binary", "@-"]);
            }
            cmd.args(["-w", "\n%{http_code}", &url]);
            cmd.stdin(if body.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = cmd.spawn().map_err(|e| {
                ConnectorError::Transport(format!("spawn curl for {url}: {e}"))
            })?;
            if let Some(payload) = body {
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(payload).map_err(|e| {
                        ConnectorError::Transport(format!("write curl request body: {e}"))
                    })?;
                }
            }
            let output = child.wait_with_output().map_err(|e| {
                ConnectorError::Transport(format!("wait curl for {url}: {e}"))
            })?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(ConnectorError::Transport(format!(
                    "HTTPS request {method} {url} failed: {stderr}"
                )));
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            let (resp_body, status_str) = stdout.rsplit_once('\n').ok_or_else(|| {
                ConnectorError::Protocol(format!("missing status code from curl for {url}"))
            })?;
            let status = status_str.trim().parse::<u16>().map_err(|_| {
                ConnectorError::Protocol(format!(
                    "invalid HTTP status code from curl: {status_str:?}"
                ))
            })?;
            Ok((status, resp_body.to_string()))
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConnectorConfig {
    pub server_url: String,
    pub connector_id: String,
    pub connector_token: String,
    pub projects_root: PathBuf,
    pub state_root: PathBuf,
    pub runtime_options: HostRuntimeOptions,
    pub request_timeout: Duration,
}

impl ConnectorConfig {
    pub fn new(
        server_url: impl Into<String>,
        connector_id: impl Into<String>,
        connector_token: impl Into<String>,
        projects_root: PathBuf,
        state_root: PathBuf,
    ) -> Self {
        Self {
            server_url: server_url.into(),
            connector_id: connector_id.into(),
            connector_token: connector_token.into(),
            projects_root,
            state_root,
            runtime_options: HostRuntimeOptions::default(),
            request_timeout: Duration::from_secs(10),
        }
    }

    pub fn endpoint(&self) -> Result<WorkbenchEndpoint, ConnectorError> {
        WorkbenchEndpoint::parse(&self.server_url)
    }

    pub fn ws_url(&self) -> Result<String, ConnectorError> {
        Ok(self.endpoint()?.ws_url())
    }

    pub fn http_host_port(&self) -> Result<(String, u16), ConnectorError> {
        let ep = self.endpoint()?;
        Ok((ep.host().to_string(), ep.port()))
    }
}

/// Discovers the 6 canonical local peers (5 desktop hosts + `grokbot`) and their effective capabilities.
pub fn discover_local_runtime_providers(
    rt: &HostRuntimeOptions,
) -> Vec<RuntimeProviderDescriptor> {
    let mut out = Vec::with_capacity(6);
    for host in DesktopHostKind::ALL {
        let has_local_channel = match host {
            DesktopHostKind::OpenaiDots => rt.openai_dots.is_some(),
            DesktopHostKind::Cindy => rt
                .cindy
                .as_ref()
                .is_some_and(|cfg| cfg.gateway_socket.exists()),
            DesktopHostKind::DeepseekHarness => rt.dsh.is_some(),
            DesktopHostKind::AntigravityDesktop => rt.antigravity.is_some(),
            DesktopHostKind::Zcode => rt.zcode.is_some(),
        };
        out.push(RuntimeProviderDescriptor {
            provider_id: host.as_str().to_string(),
            display_name: host.app_display_name().to_string(),
            bundle_id: host.bundle_id().to_string(),
            online: true,
            capabilities: EffectiveCapabilities::for_desktop_host(host, has_local_channel),
        });
    }
    out.push(RuntimeProviderDescriptor {
        provider_id: "grokbot".to_string(),
        display_name: GROKBOT_APP_DISPLAY_NAME.to_string(),
        bundle_id: GROKBOT_BUNDLE_ID.to_string(),
        online: true,
        capabilities: EffectiveCapabilities::for_grokbot(),
    });
    out
}

/// Thread-safe RPC handler executing incoming Server -> Connector RPCs under the dual-lane
/// concurrency gate (`8` standard slots / `2` critical slots) and `GenerationFence`.
#[derive(Clone)]
pub struct ConnectorRpcHandler {
    config: ConnectorConfig,
    ledger: FleetLedgerStore,
    sandbox: SandboxFsHandler,
    gate: Arc<ConnectorConcurrencyGate>,
    fence: GenerationFence,
}

impl ConnectorRpcHandler {
    pub fn new(
        config: ConnectorConfig,
        ledger: FleetLedgerStore,
        gate: Arc<ConnectorConcurrencyGate>,
        fence: GenerationFence,
    ) -> Result<Self, ConnectorError> {
        let sandbox = SandboxFsHandler::new(config.projects_root.clone())?;
        Ok(Self {
            config,
            ledger,
            sandbox,
            gate,
            fence,
        })
    }

    pub fn gate(&self) -> &Arc<ConnectorConcurrencyGate> {
        &self.gate
    }

    pub fn fence(&self) -> &GenerationFence {
        &self.fence
    }

    pub fn ledger(&self) -> &FleetLedgerStore {
        &self.ledger
    }

    /// Handles a single incoming JSON-RPC request frame, enforcing generation fencing and
    /// acquiring the appropriate semaphore lane (`critical` for `interaction.respond` /
    /// `session.interrupt`, `standard` for all other methods).
    pub fn handle_frame(&self, frame: &ConnectorRpcFrame) -> Result<ConnectorRpcFrame, ConnectorError> {
        self.fence.verify(frame.generation)?;
        let _permit = self.gate.acquire_for_frame(frame);
        let generation = self.fence.current();
        let req_id = frame.id.clone().unwrap_or_else(|| "0".to_string());
        let Some(method) = frame.method.as_deref() else {
            return Err(ConnectorError::Protocol(
                "incoming RPC frame missing method".into(),
            ));
        };
        let params: Value = match frame.params_json.as_deref() {
            Some(raw) if !raw.trim().is_empty() => serde_json::from_str(raw)
                .map_err(|e| ConnectorError::Protocol(format!("invalid paramsJson: {e}")))?,
            _ => json!({}),
        };

        if let Some(hold_ms) = params.get("simulateHoldMs").and_then(Value::as_u64) {
            if hold_ms > 0 && hold_ms <= 5_000 {
                std::thread::sleep(Duration::from_millis(hold_ms));
            }
        }

        match method {
            "runtime.discover" => {
                let providers = discover_local_runtime_providers(&self.config.runtime_options);
                let result = json!({
                    "connectorId": self.config.connector_id,
                    "generation": generation,
                    "providers": providers,
                });
                Ok(ConnectorRpcFrame::new_response(
                    generation,
                    req_id,
                    result.to_string(),
                ))
            }
            "runtime.ping" => {
                let result = json!({
                    "ok": true,
                    "connectorId": self.config.connector_id,
                    "generation": generation,
                });
                Ok(ConnectorRpcFrame::new_response(
                    generation,
                    req_id,
                    result.to_string(),
                ))
            }
            "session.create" => self.handle_session_create(generation, req_id, &params),
            "session.send" => self.handle_session_send(generation, req_id, &params),
            "session.interrupt" => {
                let session_id = params
                    .get("sessionId")
                    .or_else(|| params.get("session_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let result = json!({
                    "ok": true,
                    "sessionId": session_id,
                    "interrupted": true,
                    "lane": "critical",
                });
                Ok(ConnectorRpcFrame::new_response(
                    generation,
                    req_id,
                    result.to_string(),
                ))
            }
            "interaction.respond" => self.handle_interaction_respond(generation, req_id, &params),
            "workspace.readFile" => self.handle_workspace_read_file(generation, req_id, &params),
            "workspace.writeFile" => self.handle_workspace_write_file(generation, req_id, &params),
            "workspace.listDir" => self.handle_workspace_list_dir(generation, req_id, &params),
            "workspace.exec" => self.handle_workspace_exec(generation, req_id, &params),
            other => Ok(ConnectorRpcFrame::new_error(
                generation,
                req_id,
                "method_not_found",
                format!("unsupported connector RPC method: {other}"),
            )),
        }
    }

    fn handle_session_create(
        &self,
        generation: u64,
        req_id: String,
        params: &Value,
    ) -> Result<ConnectorRpcFrame, ConnectorError> {
        let target = params
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("");
        let (host_raw, slug_raw) = if let Some((h, s)) = target.split_once('/') {
            (h, s)
        } else {
            let h = params
                .get("runtime")
                .or_else(|| params.get("host"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let s = params
                .get("projectSlug")
                .or_else(|| params.get("slug"))
                .and_then(Value::as_str)
                .unwrap_or("");
            (h, s)
        };
        let host = match DesktopHostKind::parse(host_raw) {
            Ok(h) => h,
            Err(err) => {
                return Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "invalid_host",
                    err.to_string(),
                ));
            }
        };
        let title = params
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Workbench Session");
        let delivery_id = params
            .get("deliveryId")
            .or_else(|| params.get("delivery_id"))
            .and_then(Value::as_str)
            .unwrap_or("workbench-create-1");
        let prompt = params
            .get("prompt")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Initialize workbench session");
        let model = params
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string);

        let mut adapter = build_configured_host_adapter(
            host,
            self.config.projects_root.clone(),
            self.config.state_root.clone(),
            &self.config.runtime_options,
        )?;
        let project = match adapter.ensure_sidebar_project(slug_raw, Some(title)) {
            Ok(p) => p,
            Err(err) => {
                return Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "project_dir_boundary_error",
                    err.to_string(),
                ));
            }
        };
        let created = match adapter.create_session(&CreateSessionRequest {
            project_id: project.project_id.clone(),
            project_dir: project.project_dir.clone(),
            title: title.to_string(),
            delivery_id: delivery_id.to_string(),
            prompt: prompt.to_string(),
            model,
        }) {
            Ok(c) => c,
            Err(err) => {
                return Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "session_create_failed",
                    err.to_string(),
                ));
            }
        };

        let canonical_session = CanonicalSessionId::derive(
            &self.config.connector_id,
            host.as_str(),
            &created.session_id,
        )
        .map_err(|e| ConnectorError::Protocol(e.to_string()))?;

        let result = json!({
            "ok": true,
            "sessionId": canonical_session.as_str(),
            "externalSessionId": created.session_id,
            "projectId": project.project_id,
            "projectDir": project.project_dir,
            "projectSlug": slug_raw,
            "deliveryTier": created.tier.as_str(),
        });
        Ok(ConnectorRpcFrame::new_response(
            generation,
            req_id,
            result.to_string(),
        ))
    }

    fn handle_session_send(
        &self,
        generation: u64,
        req_id: String,
        params: &Value,
    ) -> Result<ConnectorRpcFrame, ConnectorError> {
        let host_raw = params
            .get("runtime")
            .or_else(|| params.get("host"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let host = match DesktopHostKind::parse(host_raw) {
            Ok(h) => h,
            Err(err) => {
                return Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "invalid_host",
                    err.to_string(),
                ));
            }
        };
        let slug = params
            .get("projectSlug")
            .or_else(|| params.get("slug"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let external_session_id = params
            .get("externalSessionId")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let delivery_id = params
            .get("deliveryId")
            .or_else(|| params.get("delivery_id"))
            .and_then(Value::as_str)
            .unwrap_or("workbench-send-1");
        let prompt = params
            .get("prompt")
            .or_else(|| params.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("");

        let mut adapter = build_configured_host_adapter(
            host,
            self.config.projects_root.clone(),
            self.config.state_root.clone(),
            &self.config.runtime_options,
        )?;
        let project = match adapter.ensure_sidebar_project(slug, None) {
            Ok(p) => p,
            Err(err) => {
                return Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "project_dir_boundary_error",
                    err.to_string(),
                ));
            }
        };
        let receipt = match adapter.deliver_turn(&DeliverTurnRequest {
            project_id: project.project_id.clone(),
            project_dir: project.project_dir.clone(),
            session_id: external_session_id.to_string(),
            delivery_id: delivery_id.to_string(),
            prompt: prompt.to_string(),
            model: None,
        }) {
            Ok(r) => r,
            Err(HostAdapterError::NativeWriterActive { host, detail }) => {
                return Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "native_writer_active",
                    format!("native writer active on {host}: {detail}"),
                ));
            }
            Err(err) => {
                return Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "deliver_turn_failed",
                    err.to_string(),
                ));
            }
        };

        let result = json!({
            "ok": true,
            "deliveryId": receipt.delivery_id,
            "sessionId": receipt.session_id,
            "turnId": receipt.turn_id,
            "deliveryTier": receipt.tier.as_str(),
        });
        Ok(ConnectorRpcFrame::new_response(
            generation,
            req_id,
            result.to_string(),
        ))
    }

    fn handle_interaction_respond(
        &self,
        generation: u64,
        req_id: String,
        params: &Value,
    ) -> Result<ConnectorRpcFrame, ConnectorError> {
        let notice_id = params
            .get("noticeId")
            .or_else(|| params.get("notice_id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let session_id = params
            .get("sessionId")
            .or_else(|| params.get("session_id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let decision = params
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("approved");

        if !session_id.is_empty() {
            let _ = self.ledger.update_session_runtime_state(
                session_id,
                InteractionStatus::Resolved,
                None,
            );
        }

        let result = json!({
            "ok": true,
            "noticeId": notice_id,
            "sessionId": session_id,
            "decision": decision,
            "status": InteractionStatus::Resolved.as_str(),
            "lane": "critical",
        });
        Ok(ConnectorRpcFrame::new_response(
            generation,
            req_id,
            result.to_string(),
        ))
    }

    fn handle_workspace_read_file(
        &self,
        generation: u64,
        req_id: String,
        params: &Value,
    ) -> Result<ConnectorRpcFrame, ConnectorError> {
        let slug = params
            .get("projectSlug")
            .or_else(|| params.get("slug"))
            .and_then(Value::as_str)
            .unwrap_or("demo-app");
        let path = params.get("path").and_then(Value::as_str).unwrap_or("");
        match self.sandbox.read_file(slug, path) {
            Ok(content) => {
                let res = json!({
                    "ok": true,
                    "projectSlug": slug,
                    "path": path,
                    "content": content,
                    "bytesRead": content.len(),
                });
                Ok(ConnectorRpcFrame::new_response(
                    generation,
                    req_id,
                    res.to_string(),
                ))
            }
            Err(SandboxFsError::ProjectDirBoundary(boundary_err)) => {
                Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "ProjectDirBoundaryError",
                    boundary_err.to_string(),
                ))
            }
            Err(other) => Ok(ConnectorRpcFrame::new_error(
                generation,
                req_id,
                "sandbox_io_error",
                other.to_string(),
            )),
        }
    }

    fn handle_workspace_write_file(
        &self,
        generation: u64,
        req_id: String,
        params: &Value,
    ) -> Result<ConnectorRpcFrame, ConnectorError> {
        let slug = params
            .get("projectSlug")
            .or_else(|| params.get("slug"))
            .and_then(Value::as_str)
            .unwrap_or("demo-app");
        let path = params.get("path").and_then(Value::as_str).unwrap_or("");
        let content = params.get("content").and_then(Value::as_str).unwrap_or("");
        match self.sandbox.write_file(slug, path, content) {
            Ok(written) => {
                let res = json!({
                    "ok": true,
                    "projectSlug": slug,
                    "path": path,
                    "bytesWritten": written,
                });
                Ok(ConnectorRpcFrame::new_response(
                    generation,
                    req_id,
                    res.to_string(),
                ))
            }
            Err(SandboxFsError::ProjectDirBoundary(boundary_err)) => {
                Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "ProjectDirBoundaryError",
                    boundary_err.to_string(),
                ))
            }
            Err(other) => Ok(ConnectorRpcFrame::new_error(
                generation,
                req_id,
                "sandbox_io_error",
                other.to_string(),
            )),
        }
    }

    fn handle_workspace_list_dir(
        &self,
        generation: u64,
        req_id: String,
        params: &Value,
    ) -> Result<ConnectorRpcFrame, ConnectorError> {
        let slug = params
            .get("projectSlug")
            .or_else(|| params.get("slug"))
            .and_then(Value::as_str)
            .unwrap_or("demo-app");
        let path = params.get("path").and_then(Value::as_str).unwrap_or(".");
        match self.sandbox.list_dir(slug, path) {
            Ok(entries) => {
                let res = json!({
                    "ok": true,
                    "projectSlug": slug,
                    "path": path,
                    "entries": entries,
                });
                Ok(ConnectorRpcFrame::new_response(
                    generation,
                    req_id,
                    res.to_string(),
                ))
            }
            Err(SandboxFsError::ProjectDirBoundary(boundary_err)) => {
                Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "ProjectDirBoundaryError",
                    boundary_err.to_string(),
                ))
            }
            Err(other) => Ok(ConnectorRpcFrame::new_error(
                generation,
                req_id,
                "sandbox_io_error",
                other.to_string(),
            )),
        }
    }

    fn handle_workspace_exec(
        &self,
        generation: u64,
        req_id: String,
        params: &Value,
    ) -> Result<ConnectorRpcFrame, ConnectorError> {
        let slug = params
            .get("projectSlug")
            .or_else(|| params.get("slug"))
            .and_then(Value::as_str)
            .unwrap_or("demo-app");
        let program = params.get("program").and_then(Value::as_str).unwrap_or("");
        let args: Vec<OsString> = params
            .get("args")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(OsString::from)
                    .collect()
            })
            .unwrap_or_default();
        match self
            .sandbox
            .execute_in_project(slug, OsStr::new(program), &args)
        {
            Ok(out) => {
                let res = json!({
                    "ok": true,
                    "exitCode": out.exit_code,
                    "stdout": out.stdout,
                    "stderr": out.stderr,
                });
                Ok(ConnectorRpcFrame::new_response(
                    generation,
                    req_id,
                    res.to_string(),
                ))
            }
            Err(SandboxFsError::ProjectDirBoundary(boundary_err)) => {
                Ok(ConnectorRpcFrame::new_error(
                    generation,
                    req_id,
                    "ProjectDirBoundaryError",
                    boundary_err.to_string(),
                ))
            }
            Err(other) => Ok(ConnectorRpcFrame::new_error(
                generation,
                req_id,
                "sandbox_exec_error",
                other.to_string(),
            )),
        }
    }
}

/// Outbound Rust Connector client speaking the Agents-Anywhere v2 Connector protocol
/// (`WS /api/v2/connector/ws` or `/ws/v2/connector` + chunked HTTP `POST /api/v2/connector/ingest` spillover).
pub struct ConnectorClient {
    config: ConnectorConfig,
    ledger: FleetLedgerStore,
    handler: ConnectorRpcHandler,
    ws: Option<WebSocket<ConnectorStream>>,
    pending_outbox: Vec<TimelineItem>,
    next_req_seq: u64,
}

impl ConnectorClient {
    pub fn new(config: ConnectorConfig) -> Result<Self, ConnectorError> {
        let ledger = FleetLedgerStore::open(&config.state_root)?;
        let gate = Arc::new(ConnectorConcurrencyGate::new());
        let fence = GenerationFence::new(1);
        let handler = ConnectorRpcHandler::new(config.clone(), ledger.clone(), gate, fence)?;
        Ok(Self {
            config,
            ledger,
            handler,
            ws: None,
            pending_outbox: Vec::new(),
            next_req_seq: 1,
        })
    }

    pub fn connect(&mut self) -> Result<u64, ConnectorError> {
        let endpoint = self.config.endpoint()?;
        let ws_url = endpoint.ws_url();
        let stream = ConnectorStream::connect(&endpoint, self.config.request_timeout)?;

        let mut req = ws_url
            .as_str()
            .into_client_request()
            .map_err(|e| ConnectorError::InvalidConfig(e.to_string()))?;
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", self.config.connector_token)) {
            req.headers_mut().insert("Authorization", val);
        }
        if let Ok(val) = HeaderValue::from_str(&self.config.connector_id) {
            req.headers_mut().insert("X-Connector-Id", val);
        }

        let ws_config = WebSocketConfig::default()
            .max_message_size(Some(MAX_CONNECTOR_WS_FRAME_BYTES))
            .max_frame_size(Some(MAX_CONNECTOR_WS_FRAME_BYTES));
        let (mut ws, _) = tungstenite::client::client_with_config(req, stream, Some(ws_config))
            .map_err(|e| ConnectorError::Transport(format!("WS handshake failed: {e}")))?;

        let providers = discover_local_runtime_providers(&self.config.runtime_options);
        let hello = ConnectorRpcFrame::new_request(
            self.handler.fence().current(),
            format!("hello-{}", self.next_req_seq),
            "connector.hello",
            json!({
                "connectorId": self.config.connector_id,
                "protocolVersion": "1.1",
                "providers": providers,
            })
            .to_string(),
        );
        self.next_req_seq += 1;
        let hello_json = serde_json::to_string(&hello)
            .map_err(|e| ConnectorError::Protocol(e.to_string()))?;
        ws.send(Message::Text(hello_json.into()))
            .map_err(|e| ConnectorError::Transport(format!("send connector.hello: {e}")))?;

        let reply_msg = ws
            .read()
            .map_err(|e| ConnectorError::Transport(format!("read connector.welcome: {e}")))?;
        let reply_text = match reply_msg {
            Message::Text(t) => t.to_string(),
            other => {
                return Err(ConnectorError::Protocol(format!(
                    "expected text welcome frame, got {other:?}"
                )))
            }
        };
        let welcome_frame: ConnectorRpcFrame = serde_json::from_str(&reply_text)
            .map_err(|e| ConnectorError::Protocol(format!("invalid welcome frame: {e}")))?;
        let leased_gen = welcome_frame.generation.max(1);
        self.handler.fence().advance(leased_gen)?;
        self.ws = Some(ws);

        if !self.pending_outbox.is_empty() {
            let queued = std::mem::take(&mut self.pending_outbox);
            let _ = self.ingest_via_http(&queued)?;
        }

        Ok(leased_gen)
    }

    pub fn generation(&self) -> u64 {
        self.handler.fence().current()
    }

    pub fn handler(&self) -> &ConnectorRpcHandler {
        &self.handler
    }

    pub fn ledger(&self) -> &FleetLedgerStore {
        &self.ledger
    }

    pub fn pending_outbox_len(&self) -> usize {
        self.pending_outbox.len()
    }

    pub fn queue_outbox_items(&mut self, items: Vec<TimelineItem>) {
        self.pending_outbox.extend(items);
    }

    /// Sends a JSON-RPC request frame over the open WebSocket and awaits the matching response.
    pub fn send_rpc_roundtrip(
        &mut self,
        method: &str,
        params_json: String,
    ) -> Result<ConnectorRpcFrame, ConnectorError> {
        let gen = self.generation();
        let req_id = format!("req-{}", self.next_req_seq);
        self.next_req_seq += 1;
        let frame = ConnectorRpcFrame::new_request(gen, req_id, method, params_json);
        let wire = serde_json::to_string(&frame)
            .map_err(|e| ConnectorError::Protocol(e.to_string()))?;
        let send_res = match self.ws.as_mut() {
            Some(ws) => ws.send(Message::Text(wire.into())),
            None => return Err(ConnectorError::Transport("WebSocket is not connected".into())),
        };
        if let Err(e) = send_res {
            self.ws = None;
            return Err(ConnectorError::Transport(format!("ws send failed: {e}")));
        }
        let read_res = match self.ws.as_mut() {
            Some(ws) => ws.read(),
            None => return Err(ConnectorError::Transport("WebSocket is not connected".into())),
        };
        let msg = match read_res {
            Ok(m) => m,
            Err(e) => {
                self.ws = None;
                return Err(ConnectorError::Transport(format!("ws read failed: {e}")));
            }
        };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            other => {
                return Err(ConnectorError::Protocol(format!(
                    "unexpected WS message: {other:?}"
                )))
            }
        };
        let resp: ConnectorRpcFrame = serde_json::from_str(&text)
            .map_err(|e| ConnectorError::Protocol(format!("invalid RPC response: {e}")))?;
        self.handler.fence().verify(resp.generation)?;
        Ok(resp)
    }

    /// Reads and services one incoming RPC frame from the WebSocket server.
    pub fn process_next_ws_rpc(&mut self) -> Result<ConnectorRpcFrame, ConnectorError> {
        let read_res = match self.ws.as_mut() {
            Some(ws) => ws.read(),
            None => return Err(ConnectorError::Transport("WebSocket is not connected".into())),
        };
        let msg = match read_res {
            Ok(m) => m,
            Err(e) => {
                self.ws = None;
                return Err(ConnectorError::Transport(format!("ws read failed: {e}")));
            }
        };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Ping(payload) => {
                if let Some(ws) = self.ws.as_mut() {
                    let _ = ws.send(Message::Pong(payload));
                }
                return Err(ConnectorError::Protocol("ping handled".into()));
            }
            other => {
                return Err(ConnectorError::Protocol(format!(
                    "unexpected WS frame: {other:?}"
                )))
            }
        };
        let incoming: ConnectorRpcFrame = serde_json::from_str(&text)
            .map_err(|e| ConnectorError::Protocol(format!("invalid incoming RPC frame: {e}")))?;
        let response = self.handler.handle_frame(&incoming)?;
        let resp_wire = serde_json::to_string(&response)
            .map_err(|e| ConnectorError::Protocol(e.to_string()))?;
        let send_res = self
            .ws
            .as_mut()
            .map(|ws| ws.send(Message::Text(resp_wire.into())));
        if let Some(Err(e)) = send_res {
            self.ws = None;
            return Err(ConnectorError::Transport(format!(
                "ws send response failed: {e}"
            )));
        }
        Ok(response)
    }

    /// Emits a batch of timeline items. Automatically spills over to `POST /api/v2/connector/ingest`
    /// when the serialized frame exceeds `900 KiB` (`MAX_CONNECTOR_WS_FRAME_BYTES`) or when the
    /// WebSocket is disconnected/broken.
    pub fn emit_timeline_batch(
        &mut self,
        items: &[TimelineItem],
    ) -> Result<TimelineEmitOutcome, ConnectorError> {
        let gen = self.generation();
        let params_json = serde_json::to_string(&json!({
            "connectorId": self.config.connector_id,
            "generation": gen,
            "items": items,
        }))
        .map_err(|e| ConnectorError::Protocol(e.to_string()))?;
        let frame = ConnectorRpcFrame::new_notification(gen, "timeline.itemUpsert", params_json);
        let wire = serde_json::to_string(&frame)
            .map_err(|e| ConnectorError::Protocol(e.to_string()))?;
        let wire_bytes = wire.len();

        if frame.requires_http_ingest_spillover() || wire_bytes > MAX_CONNECTOR_WS_FRAME_BYTES {
            return self.ingest_via_http(items);
        }

        if let Some(ws) = self.ws.as_mut() {
            match ws.send(Message::Text(wire.into())) {
                Ok(()) => {
                    let _ = self.ledger.upsert_timeline_batch(items)?;
                    return Ok(TimelineEmitOutcome {
                        transport: TimelineEmitTransport::WebSocket,
                        wire_bytes,
                        accepted: items.len(),
                        rejected: Vec::new(),
                        generation: gen,
                    });
                }
                Err(_) => {
                    self.ws = None;
                    self.pending_outbox.extend_from_slice(items);
                }
            }
        } else {
            self.pending_outbox.extend_from_slice(items);
        }

        let queued = std::mem::take(&mut self.pending_outbox);
        match self.ingest_via_http(&queued) {
            Ok(outcome) => Ok(outcome),
            Err(err) => {
                self.pending_outbox = queued;
                Err(err)
            }
        }
    }

    /// Posts timeline items to `POST /api/v2/connector/ingest` and isolates partial rejections
    /// (`{ accepted, rejected }`) so accepted items advance their sync fingerprints in
    /// `fleet-ledger.sqlite` while rejected indices are isolated without crashing the loop.
    pub fn ingest_via_http(
        &mut self,
        items: &[TimelineItem],
    ) -> Result<TimelineEmitOutcome, ConnectorError> {
        let gen = self.generation();
        let body_json = serde_json::to_string(&json!({
            "connectorId": self.config.connector_id,
            "generation": gen,
            "items": items,
        }))
        .map_err(|e| ConnectorError::Protocol(e.to_string()))?;
        let wire_bytes = body_json.len();

        let endpoint = self.config.endpoint()?;
        let auth_header = format!("Bearer {}", self.config.connector_token);
        let headers = [
            ("Authorization", auth_header.as_str()),
            ("X-Connector-Id", self.config.connector_id.as_str()),
            ("Content-Type", "application/json"),
        ];
        let (status, resp_body) = execute_workbench_http_raw(
            &endpoint,
            "POST",
            "/api/v2/connector/ingest",
            &headers,
            Some(body_json.as_bytes()),
            self.config.request_timeout,
        )?;
        if status != 200 {
            return Err(ConnectorError::HttpStatus {
                status,
                body: resp_body,
            });
        }
        let batch_result: IngestBatchResult = serde_json::from_str(&resp_body).map_err(|e| {
            ConnectorError::Protocol(format!("invalid IngestBatchResult JSON: {e}"))
        })?;

        let rejected_indices: BTreeSet<usize> =
            batch_result.rejected.iter().map(|r| r.index).collect();
        let accepted_items: Vec<TimelineItem> = items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                if rejected_indices.contains(&idx) {
                    None
                } else {
                    Some(item.clone())
                }
            })
            .collect();

        if !accepted_items.is_empty() {
            let _ = self.ledger.upsert_timeline_batch(&accepted_items)?;
        }

        Ok(TimelineEmitOutcome {
            transport: TimelineEmitTransport::HttpIngestSpillover,
            wire_bytes,
            accepted: batch_result.accepted,
            rejected: batch_result.rejected,
            generation: gen,
        })
    }

    /// Scans local host timelines and streams any new deltas to the Workbench server.
    pub fn sync_local_host_timelines(
        &mut self,
        host: &str,
        host_root: Option<&std::path::Path>,
    ) -> Result<crate::ObserverScanReport, ConnectorError> {
        let report =
            project_host_timelines(&self.ledger, &self.config.connector_id, host, host_root)?;
        Ok(report)
    }
}

fn read_http_response(stream: &mut TcpStream) -> Result<(u16, String), ConnectorError> {
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .map_err(|e| ConnectorError::Transport(format!("read HTTP status line: {e}")))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| {
            ConnectorError::Protocol(format!("malformed HTTP status line: {status_line:?}"))
        })?;

    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| ConnectorError::Transport(format!("read HTTP header: {e}")))?;
        if n == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse::<usize>().ok();
            }
        }
    }

    let mut body = Vec::new();
    if let Some(len) = content_length {
        body.resize(len, 0);
        reader
            .read_exact(&mut body)
            .map_err(|e| ConnectorError::Transport(format!("read HTTP body: {e}")))?;
    } else {
        reader
            .read_to_end(&mut body)
            .map_err(|e| ConnectorError::Transport(format!("read HTTP body to end: {e}")))?;
    }
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

/// Runs the connector event loop for `fleet-cli connector start`.
pub fn run_connector_daemon(
    config: ConnectorConfig,
    once: bool,
    max_messages: Option<usize>,
) -> Result<Value, ConnectorError> {
    let start = Instant::now();
    let mut client = ConnectorClient::new(config.clone())?;
    let leased_generation = client.connect()?;
    let providers = discover_local_runtime_providers(&config.runtime_options);
    let mut handled_rpcs = 0usize;

    if let Some(limit) = max_messages {
        while handled_rpcs < limit {
            match client.process_next_ws_rpc() {
                Ok(_) => handled_rpcs += 1,
                Err(ConnectorError::Protocol(msg)) if msg == "ping handled" => continue,
                Err(_) => break,
            }
        }
    } else if !once {
        loop {
            match client.process_next_ws_rpc() {
                Ok(_) => handled_rpcs += 1,
                Err(ConnectorError::Protocol(msg)) if msg == "ping handled" => continue,
                Err(_) => break,
            }
        }
    }

    Ok(json!({
        "ok": true,
        "connectorId": config.connector_id,
        "serverUrl": config.server_url,
        "leasedGeneration": leased_generation,
        "providers": providers,
        "handledRpcs": handled_rpcs,
        "elapsedMs": start.elapsed().as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workbench_endpoint_parses_aliyun_wss_and_https_gateway_urls() {
        let ep_wss =
            WorkbenchEndpoint::parse("wss://workbench.aliyun.example.com/ws/v2/connector").unwrap();
        assert_eq!(ep_wss.scheme(), EndpointScheme::Tls);
        assert_eq!(ep_wss.host(), "workbench.aliyun.example.com");
        assert_eq!(ep_wss.port(), 443);
        assert_eq!(
            ep_wss.ws_url(),
            "wss://workbench.aliyun.example.com/ws/v2/connector"
        );
        assert_eq!(
            ep_wss.http_url("/api/v2/connector/ingest"),
            "https://workbench.aliyun.example.com/api/v2/connector/ingest"
        );

        let ep_https =
            WorkbenchEndpoint::parse("https://workbench.aliyun.example.com:8443/").unwrap();
        assert_eq!(ep_https.scheme(), EndpointScheme::Tls);
        assert_eq!(ep_https.port(), 8443);
        assert_eq!(
            ep_https.ws_url(),
            "wss://workbench.aliyun.example.com:8443/api/v2/connector/ws"
        );
        assert_eq!(
            ep_https.http_url("/api/v2/health"),
            "https://workbench.aliyun.example.com:8443/api/v2/health"
        );

        let ep_plain = WorkbenchEndpoint::parse("http://127.0.0.1:5174").unwrap();
        assert_eq!(ep_plain.scheme(), EndpointScheme::Plain);
        assert_eq!(ep_plain.port(), 5174);
        assert_eq!(ep_plain.ws_url(), "ws://127.0.0.1:5174/api/v2/connector/ws");
        assert_eq!(
            ep_plain.http_url("/api/v2/providers"),
            "http://127.0.0.1:5174/api/v2/providers"
        );
    }
}
