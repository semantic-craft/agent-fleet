use fleet_proto::{DesktopHostKind, ExternalAgentManifest};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Probes whether a process matching `pattern` is running via `pgrep -f`.
pub fn is_process_running(pattern: &str) -> bool {
    Command::new("pgrep")
        .arg("-f")
        .arg(pattern)
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Reads back column names of a SQLite table via `sqlite3 -json <db> "PRAGMA table_info(<table>)"`.
/// Tries `-readonly` first; host daemons holding a hot WAL (e.g. Antigravity) can make
/// read-only opens fail with SQLITE_CANTOPEN, in which case a plain open is used — the
/// query itself (`PRAGMA table_info`) never writes, and a plain open only performs the
/// same WAL recovery any reader process would.
pub fn read_sqlite_table_columns(
    sqlite3_bin: &Path,
    db_path: &Path,
    table: &str,
) -> Result<Vec<String>, String> {
    if !db_path.exists() {
        return Err(format!("database file does not exist: {}", db_path.display()));
    }
    let query = format!("PRAGMA table_info({table})");
    let readonly = Command::new(sqlite3_bin)
        .arg("-readonly")
        .arg("-json")
        .arg(db_path)
        .arg(&query)
        .output()
        .map_err(|e| format!("failed to execute sqlite3: {e}"))?;
    let output = if readonly.status.success() {
        readonly
    } else {
        Command::new(sqlite3_bin)
            .arg("-json")
            .arg(db_path)
            .arg(&query)
            .output()
            .map_err(|e| format!("failed to execute sqlite3: {e}"))?
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("sqlite3 pragma failed: {}", stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed: Vec<Value> = serde_json::from_str(&stdout)
        .map_err(|e| format!("invalid sqlite3 JSON output: {e}"))?;
    let columns = parsed
        .into_iter()
        .filter_map(|row| row.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    Ok(columns)
}

/// Verifies whether `required` columns all exist in `actual`.
pub fn check_columns_alignment<'a>(
    actual: &[String],
    required: &'a [&'a str],
) -> (bool, Vec<String>, Vec<&'a str>) {
    let mut verified = Vec::new();
    let mut missing = Vec::new();
    for &req in required {
        if actual.iter().any(|c| c.eq_ignore_ascii_case(req)) {
            verified.push(req.to_string());
        } else {
            missing.push(req);
        }
    }
    let aligned = missing.is_empty();
    (aligned, verified, missing)
}

/// One pinned schema expectation: every column in `columns` must exist in `table` of the
/// host's live SQLite database labelled `db`. This is the single source of truth shared by
/// the live probe, `fleet-cli doctor --gate`, and the `schema_contract_sync` test; adapters
/// that drift from these lists fail the gate instead of silently corrupting host state.
#[derive(Clone, Debug)]
pub struct SchemaExpectation {
    pub db: &'static str,
    pub table: &'static str,
    pub columns: &'static [&'static str],
}

/// Adapter-pinned column expectations per host DB. Run `fleet-cli doctor --gate` after any
/// host desktop app upgrade; a `diverged` schema fails the gate with the missing columns.
pub fn schema_expectations(host: DesktopHostKind) -> Vec<SchemaExpectation> {
    host.spec()
        .sqlite_tables
        .iter()
        .map(|exp| SchemaExpectation {
            db: exp.db,
            table: exp.table,
            columns: exp.columns,
        })
        .collect()
}

/// Evaluates doctor host reports and returns human-readable gate failures. A failure means
/// a detected host's live schema `diverged` from `schema_expectations` (missing columns) or
/// a JSON-storage host reports `partial` alignment. `missing` (app never ran) and Cindy's
/// `no-local-gateway` (dispatch routed via ANet by design) are not gate failures.
pub fn evaluate_gate_failures(host_reports: &[Value]) -> Vec<String> {
    let mut failures = Vec::new();
    for report in host_reports {
        let host = report
            .get("host")
            .and_then(Value::as_str)
            .unwrap_or("<unknown-host>");
        let live = report.get("live").cloned().unwrap_or_else(|| json!({}));
        let schema_status = live
            .get("schema_status")
            .and_then(Value::as_str)
            .unwrap_or("");
        let detected = live
            .get("detected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !detected {
            continue;
        }
        if schema_status == "unreadable" {
            failures.push(format!(
                "host {host}: live schema unreadable after retries (busy/locked/hot-WAL recovery); \
                 re-run fleet-cli doctor --gate when the host app is idle"
            ));
        } else if schema_status == "diverged" || schema_status == "partial" {
            let missing = live
                .get("missing_columns")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            failures.push(format!(
                "host {host}: live schema {schema_status} (expected columns missing: {missing}); \
                 run fleet-cli doctor for details and realign the adapter before dispatching"
            ));
        }
    }
    failures
}

/// Checks every expectation for one database file against its live table columns.
/// Returns per-table verified columns and `table.column` entries for anything missing.
/// Retries a read a few times: host daemons (e.g. Antigravity) briefly make their
/// SQLite unreadable to `-readonly` clients while recovering a hot WAL, which must
/// not be mistaken for schema divergence.
fn read_sqlite_table_columns_with_retry(
    sqlite3_bin: &Path,
    db_path: &Path,
    table: &str,
) -> Result<Vec<String>, String> {
    const ATTEMPTS: u32 = 3;
    const DELAY_MS: u64 = 400;
    let mut last_err = String::new();
    for attempt in 1..=ATTEMPTS {
        match read_sqlite_table_columns(sqlite3_bin, db_path, table) {
            Ok(cols) => return Ok(cols),
            Err(e) => {
                last_err = e;
                if attempt < ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(DELAY_MS));
                }
            }
        }
    }
    Err(last_err)
}

fn check_sqlite_expectations(
    sqlite3_bin: &Path,
    db_path: &Path,
    expectations: &[SchemaExpectation],
    verified_tables: &mut BTreeMap<String, Vec<String>>,
    missing_columns: &mut Vec<String>,
    unreadable_tables: &mut Vec<String>,
) -> bool {
    let mut all_aligned = true;
    for exp in expectations {
        match read_sqlite_table_columns_with_retry(sqlite3_bin, db_path, exp.table) {
            Ok(actual) => {
                let (aligned, verified, missing) = check_columns_alignment(&actual, exp.columns);
                verified_tables.insert(exp.table.to_string(), verified);
                if !aligned {
                    all_aligned = false;
                    for m in missing {
                        missing_columns.push(format!("{}.{}", exp.table, m));
                    }
                }
            }
            Err(_) => {
                // Read failed after retries (busy/locked/hot-WAL recovery): the schema
                // was NOT observed, so claiming `diverged` would be dishonest. Report
                // the table as unreadable; the gate treats persistent unreadability as
                // its own failure, never as column drift.
                all_aligned = false;
                unreadable_tables.push(exp.table.to_string());
            }
        }
    }
    all_aligned
}

/// Probes the real, read-only live contract for a canonical desktop host.
pub fn probe_host_live_contract(host: DesktopHostKind, home: &Path) -> Value {
    let sqlite3_bin = PathBuf::from("/usr/bin/sqlite3");
    let spec = host.spec();
    let root = spec.resolve_root_dir(home);
    let bin_opt = spec.resolve_bin_path(home);
    let sock_opt = spec.resolve_socket_path(home, &root);
    let process_running = is_process_running(spec.process_grep_pattern);
    let expectations = schema_expectations(host);

    match host {
        DesktopHostKind::AntigravityDesktop => {
            let db_path = spec
                .resolve_db_path(&root, "conversation_summaries")
                .expect("antigravity conversation_summaries db path in HostSpec");
            let hub_port_file = spec
                .resolve_aux_path_in_root(&root, "daemon_dir")
                .and_then(|dir| dir.read_dir().ok())
                .and_then(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .find(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|name| name.starts_with("hub_") && name.ends_with(".json"))
                                .unwrap_or(false)
                        })
                });

            let exp = expectations
                .first()
                .expect("antigravity schema expectations must be non-empty");
            let mut unreadable = false;
            let (db_exists, cols_aligned, verified, missing) = if db_path.is_file() {
                match read_sqlite_table_columns_with_retry(&sqlite3_bin, &db_path, exp.table) {
                    Ok(actual) => {
                        let (aligned, ver, miss) = check_columns_alignment(&actual, exp.columns);
                        (
                            true,
                            aligned,
                            ver,
                            miss.into_iter().map(String::from).collect::<Vec<_>>(),
                        )
                    }
                    Err(_) => {
                        unreadable = true;
                        (true, false, Vec::new(), Vec::new())
                    }
                }
            } else {
                (false, false, Vec::new(), Vec::new())
            };

            let schema_status = if !db_exists {
                "missing"
            } else if unreadable {
                "unreadable"
            } else if cols_aligned {
                "aligned"
            } else {
                "diverged"
            };

            let ready = root.is_dir() && cols_aligned && (process_running || bin_opt.is_some());

            json!({
                "detected": root.is_dir(),
                "root_dir": root,
                "bin_path": bin_opt,
                "process_running": process_running,
                "db_path": db_path,
                "db_columns_aligned": cols_aligned,
                "schema_status": schema_status,
                "hub_socket_file": hub_port_file,
                "verified_columns": verified,
                "missing_columns": missing,
                "ready": ready,
            })
        }
        DesktopHostKind::Zcode => {
            let tasks_db = spec
                .resolve_db_path(&root, "tasks-index")
                .expect("zcode tasks-index db path in HostSpec");
            let cli_db = spec
                .resolve_db_path(&root, "cli-db")
                .expect("zcode cli-db path in HostSpec");

            let mut verified_tables = BTreeMap::new();
            let mut missing_columns = Vec::new();
            let mut unreadable_tables = Vec::new();
            let mut all_aligned = tasks_db.is_file() && cli_db.is_file();

            for (label, db_path) in [("tasks-index", &tasks_db), ("cli-db", &cli_db)] {
                if db_path.is_file() {
                    let db_expectations: Vec<_> = expectations
                        .iter()
                        .filter(|e| e.db == label)
                        .cloned()
                        .collect();
                    if !check_sqlite_expectations(
                        &sqlite3_bin,
                        db_path,
                        &db_expectations,
                        &mut verified_tables,
                        &mut missing_columns,
                        &mut unreadable_tables,
                    ) {
                        all_aligned = false;
                    }
                } else {
                    all_aligned = false;
                }
            }

            let schema_status = if !tasks_db.is_file() && !cli_db.is_file() {
                "missing"
            } else if !unreadable_tables.is_empty() {
                "unreadable"
            } else if all_aligned {
                "aligned"
            } else {
                "diverged"
            };

            let ready = root.is_dir() && all_aligned && (process_running || bin_opt.is_some());

            json!({
                "detected": root.is_dir(),
                "root_dir": root,
                "bin_path": bin_opt,
                "process_running": process_running,
                "tasks_index_db": tasks_db,
                "cli_db": cli_db,
                "db_columns_aligned": all_aligned,
                "schema_status": schema_status,
                "verified_tables": verified_tables,
                "missing_columns": missing_columns,
                "ready": ready,
            })
        }
        DesktopHostKind::DeepseekHarness => {
            let workspace_file = spec
                .resolve_aux_path_in_root(&root, "workspace_storage")
                .expect("dsh workspace_storage path in HostSpec");
            let session_cache_dir = spec
                .resolve_aux_path_in_root(&root, "session_cache_dir")
                .expect("dsh session_cache_dir path in HostSpec");
            let sessions_dir = spec
                .resolve_aux_path_in_root(&root, "sessions_dir")
                .expect("dsh sessions_dir path in HostSpec");

            let workspace_valid = workspace_file.is_file()
                && fs::read_to_string(&workspace_file)
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .is_some();

            let storage_aligned = workspace_valid && session_cache_dir.is_dir();
            let schema_status = if !workspace_file.is_file() {
                "missing"
            } else if storage_aligned {
                "aligned"
            } else {
                "partial"
            };

            let ready = root.is_dir() && storage_aligned && (process_running || bin_opt.is_some());

            json!({
                "detected": root.is_dir(),
                "root_dir": root,
                "bin_path": bin_opt,
                "process_running": process_running,
                "workspace_storage": workspace_file,
                "session_cache_dir": session_cache_dir,
                "sessions_dir": sessions_dir,
                "storage_aligned": storage_aligned,
                "schema_status": schema_status,
                "missing_columns": Vec::<String>::new(),
                "ready": ready,
            })
        }
        DesktopHostKind::OpenaiDots => {
            let db_path = spec
                .resolve_db_path(&root, "state_5")
                .expect("openai-dots state_5 db path in HostSpec");

            let mut verified_tables = BTreeMap::new();
            let mut missing_columns = Vec::new();
            let mut unreadable_tables = Vec::new();
            let mut all_aligned = db_path.is_file();

            if db_path.is_file() {
                if !check_sqlite_expectations(
                    &sqlite3_bin,
                    &db_path,
                    &expectations,
                    &mut verified_tables,
                    &mut missing_columns,
                    &mut unreadable_tables,
                ) {
                    all_aligned = false;
                }
            } else {
                all_aligned = false;
            }

            let schema_status = if !db_path.is_file() {
                "missing"
            } else if !unreadable_tables.is_empty() {
                "unreadable"
            } else if all_aligned {
                "aligned"
            } else {
                "diverged"
            };

            let ready = root.is_dir() && all_aligned && (process_running || sock_opt.is_some());

            json!({
                "detected": root.is_dir(),
                "root_dir": root,
                "db_path": db_path,
                "socket_path": sock_opt,
                "socket_connected": sock_opt.is_some(),
                "process_running": process_running,
                "db_columns_aligned": all_aligned,
                "schema_status": schema_status,
                "verified_tables": verified_tables,
                "missing_columns": missing_columns,
                "ready": ready,
            })
        }
        DesktopHostKind::Cindy => {
            let anet_bridge_cfg = spec
                .resolve_aux_path_in_home(home, "anet_bridge_config")
                .expect("cindy anet_bridge_config path in HostSpec");
            let anet_bridge_present = anet_bridge_cfg.is_file();
            let ready =
                root.is_dir() && (sock_opt.is_some() || (anet_bridge_present && process_running));
            let schema_status = if !root.is_dir() {
                "missing"
            } else if sock_opt.is_some() {
                "aligned"
            } else {
                "no-local-gateway"
            };
            let dispatch_channel = if sock_opt.is_some() {
                spec.primary_channel.as_str()
            } else {
                spec.fallback_channel
                    .unwrap_or(spec.primary_channel)
                    .as_str()
            };

            json!({
                "detected": root.is_dir(),
                "root_dir": root,
                "socket_path": sock_opt,
                "socket_connected": sock_opt.is_some(),
                "anet_bridge_config": anet_bridge_present.then_some(anet_bridge_cfg),
                "process_running": process_running,
                "dispatch_channel": dispatch_channel,
                "schema_status": schema_status,
                "missing_columns": Vec::<String>::new(),
                "ready": ready,
            })
        }
    }
}

/// Probes the real, read-only live contract for a registered external agent manifest.
pub fn probe_external_agent_live_contract(agent: &ExternalAgentManifest, home: &Path) -> Value {
    let command_exists = agent.command.is_file();
    let mut details = json!({
        "command_path": agent.command,
        "command_exists": command_exists,
    });

    let mut ready = command_exists;

    if agent.id == "herdr" {
        let sock = home.join(".config").join("herdr").join("herdr.sock");
        let server_running = sock.exists() || is_process_running("herdr");
        details["server_socket"] = json!(sock);
        details["server_running"] = json!(server_running);
        ready = command_exists && (server_running || is_process_running("herdr"));
    }

    details["ready"] = json!(ready);
    details
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_columns_alignment_matches_and_detects_missing() {
        let actual = vec!["id".into(), "title".into(), "created_at".into()];
        let required = &["id", "title"];
        let (aligned, verified, missing) = check_columns_alignment(&actual, required);
        assert!(aligned);
        assert_eq!(verified, vec!["id", "title"]);
        assert!(missing.is_empty());

        let required_with_missing = &["id", "nonexistent_col"];
        let (aligned2, verified2, missing2) = check_columns_alignment(&actual, required_with_missing);
        assert!(!aligned2);
        assert_eq!(verified2, vec!["id"]);
        assert_eq!(missing2, vec!["nonexistent_col"]);
    }

    #[test]
    fn test_read_sqlite_table_columns_returns_error_for_missing_file() {
        let sqlite3_bin = PathBuf::from("/usr/bin/sqlite3");
        let non_existent = PathBuf::from("/tmp/nonexistent-db-12345.sqlite");
        let res = read_sqlite_table_columns(&sqlite3_bin, &non_existent, "any_table");
        assert!(res.is_err());
    }

    #[test]
    fn test_probe_host_live_contract_on_empty_temp_dir() {
        let temp = std::env::temp_dir().join("test-fleet-doctor-isolated");
        let _ = fs::create_dir_all(&temp);
        for kind in DesktopHostKind::ALL {
            let report = probe_host_live_contract(kind, &temp);
            assert_eq!(report["detected"], false);
            assert_eq!(report["ready"], false);
        }
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_probe_external_agent_live_contract_for_herdr() {
        let temp = std::env::temp_dir().join("test-fleet-doctor-agent");
        let _ = fs::create_dir_all(&temp);
        let manifest = ExternalAgentManifest {
            id: "herdr".into(),
            display_name: "Herdr Terminal Multiplexer".into(),
            command: PathBuf::from("/bin/echo"),
            args_template: vec!["dispatch".into()],
            resume_args_template: Vec::new(),
            description: "Herdr".into(),
        };
        let probe = probe_external_agent_live_contract(&manifest, &temp);
        assert_eq!(probe["command_exists"], true);
        assert_eq!(probe["ready"], true);
        let _ = fs::remove_dir_all(&temp);
    }

    /// The pinned schema expectations must stay in sync with the adapters' SQL: every
    /// expected table and column must still appear in the adapter sources that touch the
    /// host databases. If an adapter drops or renames a column without updating
    /// `schema_expectations`, this test fails before any live dispatch can corrupt state.
    #[test]
    fn schema_contract_sync() {
        let adapter_sources = format!(
            "{}{}{}",
            include_str!("antigravity.rs"),
            include_str!("tri_hosts.rs"),
            include_str!("host_adapter.rs"),
        );
        for kind in DesktopHostKind::ALL {
            for exp in schema_expectations(kind) {
                assert!(
                    adapter_sources.contains(exp.table),
                    "schema expectation table `{}` ({}) not found in adapter sources",
                    exp.table,
                    kind.as_str()
                );
                for column in exp.columns {
                    assert!(
                        adapter_sources.contains(column),
                        "schema expectation column `{}` for {}.{}` ({}) not found in adapter sources",
                        column,
                        exp.db,
                        exp.table,
                        kind.as_str()
                    );
                }
            }
        }
    }

    #[test]
    fn gate_failures_flag_diverged_but_not_missing_or_anet_routed() {
        let reports = vec![
            json!({
                "host": "zcode",
                "live": {"detected": true, "schema_status": "diverged", "missing_columns": ["tasks.task_status"]},
            }),
            json!({
                "host": "antigravity-desktop",
                "live": {"detected": true, "schema_status": "aligned", "missing_columns": []},
            }),
            json!({
                "host": "deepseek-harness",
                "live": {"detected": true, "schema_status": "partial", "missing_columns": []},
            }),
            json!({
                "host": "cindy",
                "live": {"detected": true, "schema_status": "no-local-gateway", "missing_columns": []},
            }),
            json!({
                "host": "openai-dots",
                "live": {"detected": false, "schema_status": "missing", "missing_columns": []},
            }),
        ];
        let failures = evaluate_gate_failures(&reports);
        assert_eq!(failures.len(), 2, "expected zcode+deepseek failures: {failures:?}");
        assert!(failures[0].contains("zcode") && failures[0].contains("tasks.task_status"));
        assert!(failures[1].contains("deepseek-harness"));

        // Unreadable (busy/hot-WAL) is its own failure, never reported as column drift.
        let unreadable = vec![json!({
            "host": "antigravity-desktop",
            "live": {"detected": true, "schema_status": "unreadable", "missing_columns": []},
        })];
        let failures = evaluate_gate_failures(&unreadable);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("unreadable"));
        assert!(!failures[0].contains("missing:"));
    }
}

