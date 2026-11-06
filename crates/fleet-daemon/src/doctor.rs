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

/// Reads back column names of a SQLite table via `sqlite3 -readonly -json <db> "PRAGMA table_info(<table>)"`.
pub fn read_sqlite_table_columns(
    sqlite3_bin: &Path,
    db_path: &Path,
    table: &str,
) -> Result<Vec<String>, String> {
    if !db_path.exists() {
        return Err(format!("database file does not exist: {}", db_path.display()));
    }
    let query = format!("PRAGMA table_info({table})");
    let output = Command::new(sqlite3_bin)
        .arg("-readonly")
        .arg("-json")
        .arg(db_path)
        .arg(&query)
        .output()
        .map_err(|e| format!("failed to execute sqlite3: {e}"))?;
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

/// Probes the real, read-only live contract for a canonical desktop host.
pub fn probe_host_live_contract(host: DesktopHostKind, home: &Path) -> Value {
    let sqlite3_bin = PathBuf::from("/usr/bin/sqlite3");

    match host {
        DesktopHostKind::AntigravityDesktop => {
            let root = std::env::var("ANTIGRAVITY_DATA_DIR")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".gemini").join("antigravity"));
            let db_path = root.join("conversation_summaries.db");
            let mut candidate_bins = vec![
                home.join(".local").join("bin").join("agy"),
                PathBuf::from("/Applications/Antigravity.app/Contents/Resources/app/bin/agy"),
                home.join(".local").join("libexec").join("agy").join("agy"),
            ];
            if let Ok(b) = std::env::var("AGY_BIN") {
                if !b.trim().is_empty() {
                    candidate_bins.insert(0, PathBuf::from(b));
                }
            }
            let bin_opt = candidate_bins.into_iter().find(|p| p.is_file());
            let process_running = is_process_running("Antigravity.app|bin/agy");

            let hub_port_file = root
                .join("daemon")
                .read_dir()
                .ok()
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

            let required_cols = &[
                "conversation_id",
                "title",
                "preview",
                "step_count",
                "last_modified_time",
                "workspace_uris",
                "status",
                "project_id",
            ];

            let (db_exists, cols_aligned, verified, missing) =
                if db_path.is_file() {
                    match read_sqlite_table_columns(&sqlite3_bin, &db_path, "conversation_summaries") {
                        Ok(actual) => {
                            let (aligned, ver, miss) = check_columns_alignment(&actual, required_cols);
                            (true, aligned, ver, miss.into_iter().map(String::from).collect::<Vec<_>>())
                        }
                        Err(_) => (true, false, Vec::new(), required_cols.iter().map(|s| s.to_string()).collect()),
                    }
                } else {
                    (false, false, Vec::new(), required_cols.iter().map(|s| s.to_string()).collect())
                };

            let schema_status = if !db_exists {
                "missing"
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
            let root = std::env::var("ZCODE_HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".zcode"));
            let tasks_db = root.join("v2").join("tasks-index.sqlite");
            let cli_db = root.join("cli").join("db").join("db.sqlite");
            let mut candidate_bins = vec![
                home.join("bin").join("zcode"),
                home.join(".local").join("bin").join("zcode"),
                PathBuf::from("/Applications/ZCode.app/Contents/Resources/cli/zcode"),
            ];
            if let Ok(b) = std::env::var("ZCODE_BIN") {
                if !b.trim().is_empty() {
                    candidate_bins.insert(0, PathBuf::from(b));
                }
            }
            let bin_opt = candidate_bins.into_iter().find(|p| p.is_file());
            let process_running = is_process_running("ZCode.app|bin/zcode");

            let mut verified_tables = BTreeMap::new();
            let mut missing_columns = Vec::new();
            let mut all_aligned = tasks_db.is_file() && cli_db.is_file();

            if tasks_db.is_file() {
                let check_table = |tbl: &str, req: &[&str], verified_tables: &mut BTreeMap<String, Vec<String>>, missing_columns: &mut Vec<String>, all_aligned: &mut bool| {
                    if let Ok(actual) = read_sqlite_table_columns(&sqlite3_bin, &tasks_db, tbl) {
                        let (aligned, ver, miss) = check_columns_alignment(&actual, req);
                        verified_tables.insert(tbl.to_string(), ver);
                        if !aligned {
                            *all_aligned = false;
                            for m in miss {
                                missing_columns.push(format!("{tbl}.{m}"));
                            }
                        }
                    } else {
                        *all_aligned = false;
                        for m in req {
                            missing_columns.push(format!("{tbl}.{m}"));
                        }
                    }
                };

                check_table("tasks", &["task_id", "title", "project_id", "task_status"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
                check_table("projects", &["project_id", "host_id", "name"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
                check_table("project_roots", &["project_id", "root_path"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
                check_table("hosts", &["host_id", "kind"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
            } else {
                all_aligned = false;
            }

            if cli_db.is_file() {
                let check_table = |tbl: &str, req: &[&str], verified_tables: &mut BTreeMap<String, Vec<String>>, missing_columns: &mut Vec<String>, all_aligned: &mut bool| {
                    if let Ok(actual) = read_sqlite_table_columns(&sqlite3_bin, &cli_db, tbl) {
                        let (aligned, ver, miss) = check_columns_alignment(&actual, req);
                        verified_tables.insert(tbl.to_string(), ver);
                        if !aligned {
                            *all_aligned = false;
                            for m in miss {
                                missing_columns.push(format!("{tbl}.{m}"));
                            }
                        }
                    } else {
                        *all_aligned = false;
                        for m in req {
                            missing_columns.push(format!("{tbl}.{m}"));
                        }
                    }
                };

                check_table("session", &["id", "project_id", "slug", "directory", "title"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
                check_table("message", &["id", "session_id", "data", "sequence"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
            } else {
                all_aligned = false;
            }

            let schema_status = if !tasks_db.is_file() && !cli_db.is_file() {
                "missing"
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
            let root = std::env::var("DSH_HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".dsh"));
            let mut candidate_bins = vec![
                home.join(".local").join("bin").join("dsh"),
                home.join("bin").join("dsh"),
                PathBuf::from("/Applications/DeepSeek Harness.app/Contents/Resources/runtime/cli/bin/dsh"),
            ];
            if let Ok(b) = std::env::var("DSH_BIN") {
                if !b.trim().is_empty() {
                    candidate_bins.insert(0, PathBuf::from(b));
                }
            }
            let bin_opt = candidate_bins.into_iter().find(|p| p.is_file());
            let process_running = is_process_running("DeepSeek Harness|bin/dsh");

            let workspace_file = root.join("storages").join("workspace.json");
            let session_cache_dir = root.join("storages").join("session_projcache").join("sessions");
            let sessions_dir = root.join("sessions");

            let workspace_valid = if workspace_file.is_file() {
                fs::read_to_string(&workspace_file)
                    .ok()
                    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                    .is_some()
            } else {
                false
            };

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
            let root = std::env::var("CODEX_HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"));
            let db_path = root.join("state_5.sqlite");
            let process_running = is_process_running("Codex.app|app-server");

            let mut candidate_sockets = vec![
                root.join("app-server.sock"),
                root.join("app-server-control").join("app-server-control.sock"),
                root.join("ipc").join("ipc.sock"),
                root.join("app-server-daemon").join("daemon-updater.sock"),
            ];
            if let Ok(sock) = std::env::var("CODEX_APP_SERVER_SOCKET") {
                if !sock.trim().is_empty() {
                    candidate_sockets.insert(0, PathBuf::from(sock));
                }
            }
            let sock_opt = candidate_sockets.into_iter().find(|p| p.exists());

            let mut verified_tables = BTreeMap::new();
            let mut missing_columns = Vec::new();
            let mut all_aligned = db_path.is_file();

            if db_path.is_file() {
                let check_table = |tbl: &str, req: &[&str], verified_tables: &mut BTreeMap<String, Vec<String>>, missing_columns: &mut Vec<String>, all_aligned: &mut bool| {
                    if let Ok(actual) = read_sqlite_table_columns(&sqlite3_bin, &db_path, tbl) {
                        let (aligned, ver, miss) = check_columns_alignment(&actual, req);
                        verified_tables.insert(tbl.to_string(), ver);
                        if !aligned {
                            *all_aligned = false;
                            for m in miss {
                                missing_columns.push(format!("{tbl}.{m}"));
                            }
                        }
                    } else {
                        *all_aligned = false;
                        for m in req {
                            missing_columns.push(format!("{tbl}.{m}"));
                        }
                    }
                };

                check_table("threads", &["id", "cwd", "title", "project_id"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
                check_table("projects", &["id"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
                check_table("project_roots", &["project_id", "path"], &mut verified_tables, &mut missing_columns, &mut all_aligned);
            } else {
                all_aligned = false;
            }

            let schema_status = if !db_path.is_file() {
                "missing"
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
            let root = std::env::var("CINDY_HOME")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cindy"));
            let process_running = is_process_running("Cindy.app|cindy");

            let mut candidate_sockets = vec![
                root.join("cindy-gateway.sock"),
                root.join("gateway.sock"),
                home.join("Library/Application Support/Cindy/gateway.sock"),
                home.join("Library/Application Support/CindyGlobal/gateway.sock"),
            ];
            if let Ok(sock) = std::env::var("CINDY_GATEWAY_SOCKET") {
                if !sock.trim().is_empty() {
                    candidate_sockets.insert(0, PathBuf::from(sock));
                }
            }
            let sock_opt = candidate_sockets.into_iter().find(|p| p.exists());
            let ready = root.is_dir() && (sock_opt.is_some() || process_running);

            json!({
                "detected": root.is_dir(),
                "root_dir": root,
                "socket_path": sock_opt,
                "socket_connected": sock_opt.is_some(),
                "process_running": process_running,
                "schema_status": if root.is_dir() { "aligned" } else { "missing" },
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
            args_template: vec!["dispatched".into()],
            resume_args_template: Vec::new(),
            description: "Herdr".into(),
        };
        let probe = probe_external_agent_live_contract(&manifest, &temp);
        assert_eq!(probe["command_exists"], true);
        assert_eq!(probe["ready"], true);
        let _ = fs::remove_dir_all(&temp);
    }
}

