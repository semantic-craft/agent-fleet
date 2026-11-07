use fleet_daemon::{
    detect_antigravity_runtime, detect_cindy_runtime, detect_dsh_runtime,
    detect_openai_dots_runtime, detect_zcode_runtime, HostRuntimeOptions, ZcodeRuntimeConfig,
};
use fleet_workbench_server::{WorkbenchServerConfig, WorkbenchServerHandle};
use std::{
    collections::BTreeMap,
    env,
    path::PathBuf,
    process::ExitCode,
    thread,
    time::Duration,
};

const USAGE: &str = "usage: fleet-workbench-server [--bind <host:port>] [--state-root <path>] [--projects-root <path>] [--static-dir <path>] [--connector-token <token>] [--zcode-root <path>] [--zcode-bin <path>]";

fn parse_args() -> Result<WorkbenchServerConfig, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "help") {
        println!("{USAGE}");
        std::process::exit(0);
    }
    if !args.len().is_multiple_of(2) {
        return Err(USAGE.into());
    }
    let mut opts = BTreeMap::new();
    for pair in args.chunks_exact(2) {
        opts.insert(pair[0].clone(), pair[1].clone());
    }

    let home = env::var("HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    let bind_addr = opts
        .remove("--bind")
        .or_else(|| env::var("WORKBENCH_BIND_ADDR").ok())
        .unwrap_or_else(|| "127.0.0.1:5174".to_string());
    let state_root = opts
        .remove("--state-root")
        .or_else(|| env::var("FLEET_STATE_ROOT").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state/agent-fleet"));
    let projects_root = opts
        .remove("--projects-root")
        .or_else(|| env::var("FLEET_PROJECTS_ROOT").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Projects"));
    let static_dir = opts
        .remove("--static-dir")
        .or_else(|| env::var("WORKBENCH_STATIC_DIR").ok())
        .map(PathBuf::from);
    let connector_token = opts
        .remove("--connector-token")
        .or_else(|| env::var("WORKBENCH_CONNECTOR_TOKEN").ok())
        .unwrap_or_else(|| "dev-token".to_string());

    let mut rt = HostRuntimeOptions {
        openai_dots: detect_openai_dots_runtime(&home),
        cindy: detect_cindy_runtime(&home),
        dsh: detect_dsh_runtime(&home),
        antigravity: detect_antigravity_runtime(&home),
        zcode: detect_zcode_runtime(&home),
    };
    if let (Some(zroot), Some(zbin)) = (opts.remove("--zcode-root"), opts.remove("--zcode-bin")) {
        rt.zcode = Some(ZcodeRuntimeConfig::new(
            PathBuf::from(zroot),
            PathBuf::from(zbin),
        ));
    }

    let mut cfg = WorkbenchServerConfig::new(bind_addr, state_root, projects_root);
    cfg.static_dir = static_dir;
    cfg.connector_token = connector_token;
    cfg.runtime_options = rt;
    Ok(cfg)
}

fn main() -> ExitCode {
    let cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let handle = match WorkbenchServerHandle::start(cfg) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("failed to start fleet-workbench-server: {e}");
            return ExitCode::from(1);
        }
    };
    println!("fleet-workbench-server listening on {}", handle.base_url());
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}
