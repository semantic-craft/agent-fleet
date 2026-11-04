#![cfg(unix)]

use fleet_daemon::{NodeDaemon, SelectedSession};
use fleet_proto::{SessionId, SessionRecord, SessionSelector, SessionState};
use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::fs::symlink,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    directory: PathBuf,
    a: PathBuf,
    b: PathBuf,
    cache: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "fleet-cwd-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let directory = root.join("sessions");
        let a = root.join("project-a");
        let b = root.join("project-b");
        let cache = root.join("anet-cache");
        for path in [&directory, &a, &b, &cache] {
            fs::create_dir(path).unwrap();
        }
        Self {
            root,
            directory,
            a,
            b,
            cache,
        }
    }
    fn daemon(&self) -> NodeDaemon {
        NodeDaemon::new(self.directory.clone(), vec![self.cache.clone()]).unwrap()
    }
    fn record(&self, name: &str, cwd: PathBuf) -> SessionRecord {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        SessionRecord {
            id: SessionId {
                host: "fixture-host".into(),
                desktop: "fixture-desktop".into(),
                thread_id: name.into(),
            },
            project_cwd: cwd,
            state: SessionState::Open,
            observed_at: now - 1,
            expires_at: now + 3600,
        }
    }
    fn write(&self, record: &SessionRecord) {
        let temp = self.directory.join("record.tmp");
        fs::write(&temp, serde_json::to_vec(record).unwrap()).unwrap();
        fs::rename(
            temp,
            self.directory.join(format!("{}.json", record.id.thread_id)),
        )
        .unwrap();
    }
    fn reject(&self, daemon: &mut NodeDaemon, token: SelectedSession, label: &str) {
        let marker = self.root.join("executed");
        let result = daemon.execute(
            token,
            OsStr::new("/bin/sh"),
            &[
                OsString::from("-c"),
                OsString::from("printf executed > \"$1\""),
                OsString::from("sh"),
                marker.clone().into_os_string(),
            ],
        );
        assert!(result.is_err(), "{label}: {result:?}");
        assert!(!marker.exists(), "{label}: rejected command executed");
        println!("REJECT {label}: {}; executed=0", result.unwrap_err());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn selector(name: &str) -> SessionSelector {
    SessionSelector {
        thread_id: Some(name.into()),
        ..Default::default()
    }
}

#[test]
fn cwd_follows_session_switches_real_commands_between_projects() {
    let f = Fixture::new();
    f.write(&f.record("a", f.a.clone()));
    f.write(&f.record("b", f.b.clone()));
    let initial_cwd = std::env::current_dir().unwrap();
    let mut daemon = f.daemon();
    for (name, expected) in [("a", &f.a), ("b", &f.b), ("a", &f.a)] {
        let selected = daemon.select(&selector(name)).unwrap();
        let output = daemon
            .execute(selected, OsStr::new("/bin/pwd"), &[])
            .unwrap();
        assert!(output.status.success());
        let actual = String::from_utf8(output.stdout).unwrap();
        assert_eq!(actual.trim(), expected.to_str().unwrap());
        println!(
            "EXEC session={name} command=/bin/pwd stdout={}",
            actual.trim()
        );
    }
    assert_eq!(std::env::current_dir().unwrap(), initial_cwd);
    assert_ne!(initial_cwd, f.a);
    assert_ne!(initial_cwd, f.b);
}

#[test]
fn cwd_follows_session_rejects_stale_and_foreign_selections() {
    let f = Fixture::new();
    f.write(&f.record("a", f.a.clone()));
    f.write(&f.record("b", f.b.clone()));
    let mut daemon = f.daemon();
    let a = daemon.select(&selector("a")).unwrap();
    let _b = daemon.select(&selector("b")).unwrap();
    let _a_again = daemon.select(&selector("a")).unwrap();
    f.reject(&mut daemon, a, "switch A-B-A");
    let a = daemon.select(&selector("a")).unwrap();
    assert!(daemon.select(&selector("missing")).is_err());
    f.reject(
        &mut daemon,
        a,
        "failed selection invalidates previous selection",
    );
    let mut other = f.daemon();
    let foreign = other.select(&selector("a")).unwrap();
    let _local = daemon.select(&selector("a")).unwrap();
    f.reject(&mut daemon, foreign, "foreign daemon token");
}

#[test]
fn cwd_follows_session_rejects_record_changes_before_spawn() {
    for kind in [
        "cwd",
        "closed",
        "expired",
        "missing",
        "malformed",
        "ambiguous",
        "thread",
        "refresh",
    ] {
        let f = Fixture::new();
        let mut record = f.record("a", f.a.clone());
        f.write(&record);
        let mut daemon = f.daemon();
        let selected = daemon.select(&SessionSelector::default()).unwrap();
        match kind {
            "refresh" => {
                record.expires_at += 1;
                f.write(&record);
            }
            "cwd" => {
                record.project_cwd = f.b.clone();
                f.write(&record);
            }
            "closed" => {
                record.state = SessionState::Closed;
                f.write(&record);
            }
            "expired" => {
                record.observed_at -= 10;
                record.expires_at = record.observed_at + 1;
                f.write(&record);
            }
            "missing" => {
                fs::remove_file(f.directory.join("a.json")).unwrap();
            }
            "malformed" => {
                fs::write(f.directory.join("a.json"), b"{").unwrap();
            }
            "ambiguous" => f.write(&f.record("b", f.b.clone())),
            "thread" => {
                fs::remove_file(f.directory.join("a.json")).unwrap();
                f.write(&f.record("b", f.b.clone()));
            }
            _ => unreachable!(),
        }
        f.reject(&mut daemon, selected, kind);
    }
}

#[test]
fn cwd_follows_session_rejects_missing_replaced_and_redirected_directories() {
    for kind in ["missing", "file", "replacement", "cache-symlink"] {
        let f = Fixture::new();
        f.write(&f.record("a", f.a.clone()));
        let mut daemon = f.daemon();
        let selected = daemon.select(&selector("a")).unwrap();
        fs::rename(&f.a, f.root.join("original-a")).unwrap();
        match kind {
            "file" => fs::write(&f.a, b"file").unwrap(),
            "replacement" => fs::create_dir(&f.a).unwrap(),
            "cache-symlink" => symlink(&f.cache, &f.a).unwrap(),
            "missing" => (),
            _ => unreachable!(),
        }
        f.reject(&mut daemon, selected, kind);
    }
}

#[test]
fn cwd_follows_session_rejects_invalid_projects_at_selection() {
    let f = Fixture::new();
    let hidden = f.root.join(".cache/project");
    let mac_cache = f.root.join("Library/Caches/project");
    let anet = f.root.join(".anet/project");
    for path in [&hidden, &mac_cache, &anet] {
        fs::create_dir_all(path).unwrap();
    }
    let alias = f.root.join("cache-alias");
    symlink(&f.cache, &alias).unwrap();
    let lexical = f.root.join(".cache/link-to-project");
    symlink(&f.a, &lexical).unwrap();
    let file = f.root.join("regular-file");
    fs::write(&file, b"file").unwrap();
    let mut daemon = f.daemon();
    for path in [
        f.root.join("missing"),
        file,
        f.cache.clone(),
        hidden,
        mac_cache,
        anet,
        alias,
        lexical,
        f.directory.clone(),
    ] {
        f.write(&f.record("a", path.clone()));
        let result = daemon.select(&selector("a"));
        assert!(result.is_err(), "accepted {}", path.display());
        println!("REJECT selection cwd={}; executed=0", path.display());
    }
}

#[test]
fn cwd_follows_session_rejects_relative_commands_and_invalid_cache_config() {
    let f = Fixture::new();
    f.write(&f.record("a", f.a.clone()));
    let mut daemon = f.daemon();
    let selected = daemon.select(&selector("a")).unwrap();
    assert!(daemon.execute(selected, OsStr::new("pwd"), &[]).is_err());
    assert!(NodeDaemon::new(f.directory.clone(), vec![f.root.join("missing-cache")]).is_err());
}

#[test]
fn cwd_follows_session_rejects_replaced_configuration_roots() {
    for config in ["cache", "session-directory"] {
        let f = Fixture::new();
        f.write(&f.record("a", f.a.clone()));
        let mut daemon = f.daemon();
        let selected = daemon.select(&selector("a")).unwrap();
        let root = if config == "cache" {
            &f.cache
        } else {
            &f.directory
        };
        fs::rename(root, f.root.join("previous-root")).unwrap();
        fs::create_dir(root).unwrap();
        f.write(&f.record("a", f.a.clone()));
        f.reject(&mut daemon, selected, config);
    }
}
