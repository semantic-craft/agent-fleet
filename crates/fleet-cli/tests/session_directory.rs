use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fleet-session-directory-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, value: &Value) {
        fs::write(self.0.join(name), serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn run(&self, command: &str, selectors: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
            .args(["sessions", command, "--directory"])
            .arg(&self.0)
            .args(selectors)
            .env_clear()
            .output()
            .unwrap()
    }

    fn snapshot(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        files.sort();
        files
    }

    fn rejected_without_thread_creation(&self, selectors: &[&str], reason: &str) {
        let before = self.snapshot();
        let output = self.run("resolve", selectors);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{output:?}"
        );
        assert!(
            output.stdout.is_empty(),
            "rejection must not return a replacement thread"
        );
        assert_eq!(
            self.snapshot(),
            before,
            "lookup must not create or rewrite session records"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn record(thread: &str) -> Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    json!({
        "id": { "host": "fixture-host", "desktop": "codex", "thread_id": thread },
        "project_cwd": "/fixture/project",
        "state": "open",
        "observed_at": now - 1,
        "expires_at": now + 3600
    })
}

fn parsed(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn session_directory_missing_session_never_creates_thread() {
    let fixture = Fixture::new();
    fixture.rejected_without_thread_creation(&[], "session missing");
    fixture.write("present.json", &record("present"));
    fixture.rejected_without_thread_creation(&["--thread", "missing"], "session missing");
}

#[test]
fn session_directory_missing_directory_is_not_created() {
    let fixture = Fixture::new();
    let missing = fixture.0.join("missing");
    let output = Command::new(env!("CARGO_BIN_EXE_fleet-cli"))
        .args(["sessions", "resolve", "--directory"])
        .arg(&missing)
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!missing.exists());
    assert!(fixture.snapshot().is_empty());
}

#[test]
fn session_directory_ambiguous_requires_explicit_thread() {
    let fixture = Fixture::new();
    fixture.write("a.json", &record("one"));
    fixture.write("b.json", &record("two"));
    fixture
        .rejected_without_thread_creation(&["--project", "/fixture/project"], "session ambiguous");
    let selected = parsed(fixture.run("resolve", &["--thread", "two"]));
    assert_eq!(selected["id"]["thread_id"], "two");
}

#[test]
fn session_directory_expired_explicit_thread_never_falls_back_to_live_thread() {
    let fixture = Fixture::new();
    let mut expired = record("expired");
    expired["observed_at"] = json!(1);
    expired["expires_at"] = json!(2);
    fixture.write("expired.json", &expired);
    fixture.write("live.json", &record("live"));
    fixture.rejected_without_thread_creation(&["--thread", "expired"], "session expired");
    fixture.rejected_without_thread_creation(&[], "session ambiguous");
    assert_eq!(
        parsed(fixture.run("resolve", &["--thread", "live"]))["id"]["thread_id"],
        "live"
    );
}

#[test]
fn session_directory_identity_survives_restart_and_index_file_rename() {
    let fixture = Fixture::new();
    let original = record("native-thread-42");
    fixture.write("original.json", &original);
    let before = parsed(fixture.run("resolve", &[]));
    fs::rename(
        fixture.0.join("original.json"),
        fixture.0.join("renamed.json"),
    )
    .unwrap();
    let after = parsed(fixture.run("resolve", &[]));
    assert_eq!(before, after);
    assert_eq!(after["id"], original["id"]);
    assert_eq!(parsed(fixture.run("list", &[])), json!([original]));
}

#[test]
fn session_directory_identity_scopes_native_threads_by_host_and_desktop() {
    let fixture = Fixture::new();
    fixture.write("one.json", &record("same-native-id"));
    let mut other = record("same-native-id");
    other["id"]["host"] = json!("other-host");
    fixture.write("two.json", &other);
    fixture.rejected_without_thread_creation(&["--thread", "same-native-id"], "session ambiguous");
    assert_eq!(
        parsed(fixture.run(
            "resolve",
            &["--host", "other-host", "--thread", "same-native-id"]
        ))["id"],
        other["id"]
    );
}

#[test]
fn session_directory_duplicate_identity_rejected_even_when_records_match() {
    let fixture = Fixture::new();
    let original = record("same");
    fixture.write("one.json", &original);
    fixture.write("two.json", &original);
    fixture.rejected_without_thread_creation(&[], "duplicate session identity");
    let mut changed = original;
    changed["project_cwd"] = json!("/other/project");
    fixture.write("two.json", &changed);
    fixture.rejected_without_thread_creation(&[], "duplicate session identity");
}

#[test]
fn session_directory_malformed_records_fail_closed() {
    let fixture = Fixture::new();
    fixture.write("valid.json", &record("valid"));
    fs::write(fixture.0.join("broken.json"), b"{").unwrap();
    fixture.rejected_without_thread_creation(&["--thread", "valid"], "invalid session record");
    fs::write(fixture.0.join("broken.json"), b"{}").unwrap();
    fixture.rejected_without_thread_creation(&["--thread", "valid"], "invalid session record");
}

#[test]
fn session_directory_future_observation_invalid_lease_and_closed_are_rejected() {
    let fixture = Fixture::new();
    let mut value = record("target");
    value["observed_at"] = json!(u64::MAX - 1);
    value["expires_at"] = json!(u64::MAX);
    fixture.write("target.json", &value);
    fixture.rejected_without_thread_creation(&[], "invalid observation or expiration time");
    value["observed_at"] = json!(10);
    value["expires_at"] = json!(10);
    fixture.write("target.json", &value);
    fixture.rejected_without_thread_creation(&[], "invalid observation or expiration time");
    value = record("target");
    value["state"] = json!("closed");
    fixture.write("target.json", &value);
    fixture.rejected_without_thread_creation(&[], "session closed");
}

#[test]
fn session_directory_invalid_identity_and_relative_project_are_rejected() {
    let fixture = Fixture::new();
    for invalid in ["", " padded", "has\ncontrol"] {
        let mut value = record(invalid);
        fixture.write("target.json", &value);
        fixture.rejected_without_thread_creation(&[], "identity components");
        value = record("valid");
        value["project_cwd"] = json!("relative/path");
        fixture.write("target.json", &value);
        fixture.rejected_without_thread_creation(&[], "project_cwd must be absolute");
    }
}

#[test]
fn session_directory_unknown_command_or_flag_never_creates_thread() {
    let fixture = Fixture::new();
    let output = fixture.run("create", &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(fixture.snapshot().is_empty());
    fixture.rejected_without_thread_creation(&["--create-if-missing", "true"], "usage:");
}

#[cfg(unix)]
#[test]
fn session_directory_json_symlink_is_rejected() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let target = Fixture::new();
    target.write("target.json", &record("external"));
    symlink(target.0.join("target.json"), fixture.0.join("linked.json")).unwrap();
    fixture.rejected_without_thread_creation(&[], "regular files");
    assert_eq!(
        parsed(target.run("resolve", &[]))["id"]["thread_id"],
        "external"
    );
}

#[test]
fn session_directory_ignores_incomplete_non_json_writes() {
    let fixture = Fixture::new();
    fixture.write("session.json", &record("ready"));
    fs::write(fixture.0.join("pending.tmp"), b"{").unwrap();
    assert_eq!(
        parsed(fixture.run("resolve", &[]))["id"]["thread_id"],
        "ready"
    );
    assert!(Path::new(&fixture.0.join("pending.tmp")).exists());
}
