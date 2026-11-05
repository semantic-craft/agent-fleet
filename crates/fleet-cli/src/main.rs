use fleet_daemon::{
    run_host_poteto_relay, AntigravityDesktopAdapter, AntigravityRuntimeConfig, CindyAdapter,
    CindyRuntimeConfig, DeepSeekHarnessAdapter, DesktopHostAdapter, DshRuntimeConfig,
    HostRuntimeOptions, OpenAiDotsAdapter, OpenAiDotsRuntimeConfig, PotetoRelayTask,
    SessionDirectory,
};
use fleet_proto::{
    CreateSessionRequest, DeliverTurnRequest, DesktopHostKind, SessionSelector,
};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "usage: fleet-cli <sessions|hosts|relay> ...";

fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|error| error.to_string())
}

fn parse_key_value_pairs(args: &[String], allowed_keys: &[&str]) -> Result<BTreeMap<String, String>, String> {
    if !args.len().is_multiple_of(2) {
        return Err(USAGE.into());
    }
    let mut options = BTreeMap::new();
    for pair in args.chunks_exact(2) {
        let key = pair[0].as_str();
        let val = pair[1].as_str();
        if !allowed_keys.contains(&key)
            || val.is_empty()
            || val.starts_with("--")
            || options.insert(key.to_string(), val.to_string()).is_some()
        {
            return Err(USAGE.into());
        }
    }
    Ok(options)
}

fn parse_runtime_options(options: &BTreeMap<String, String>) -> Result<HostRuntimeOptions, String> {
    let mut rt = HostRuntimeOptions::default();
    match (
        options.get("--codex-root"),
        options.get("--app-server-socket"),
    ) {
        (Some(root), Some(sock)) => {
            rt.openai_dots = Some(OpenAiDotsRuntimeConfig::new(
                PathBuf::from(root),
                PathBuf::from(sock),
            ));
        }
        (None, None) => {}
        _ => {
            return Err(
                "both --codex-root and --app-server-socket are required for openai-dots runtime"
                    .into(),
            )
        }
    }

    match (options.get("--cindy-root"), options.get("--gateway-socket")) {
        (Some(root), Some(sock)) => {
            rt.cindy = Some(CindyRuntimeConfig::new(
                PathBuf::from(root),
                PathBuf::from(sock),
            ));
        }
        (None, None) => {}
        _ => {
            return Err(
                "both --cindy-root and --gateway-socket are required for cindy runtime".into(),
            )
        }
    }

    if let Some(base_url) = options.get("--dsh-base-url") {
        rt.dsh = Some(DshRuntimeConfig::new(base_url.clone()));
    }

    match (options.get("--antigravity-root"), options.get("--agy-bin")) {
        (Some(root), Some(bin)) => {
            rt.antigravity = Some(AntigravityRuntimeConfig::new(
                PathBuf::from(root),
                PathBuf::from(bin),
            ));
        }
        (None, None) => {}
        _ => {
            return Err(
                "both --antigravity-root and --agy-bin are required for antigravity-desktop runtime"
                    .into(),
            )
        }
    }

    Ok(rt)
}

fn run_sessions(args: &[String]) -> Result<(), String> {
    if args.len() < 4
        || !matches!(args[1].as_str(), "list" | "resolve")
        || !args.len().is_multiple_of(2)
    {
        return Err(USAGE.into());
    }
    let mut options = parse_key_value_pairs(
        &args[2..],
        &["--directory", "--host", "--desktop", "--thread", "--project"],
    )?;
    let directory_path = options.remove("--directory").ok_or(USAGE)?;
    if args[1] == "list" && !options.is_empty() {
        return Err("list does not accept selectors".into());
    }
    let directory = SessionDirectory::discover(Path::new(&directory_path), now()?)
        .map_err(|e| e.to_string())?;
    let output = if args[1] == "list" {
        serde_json::to_string_pretty(&directory.list().collect::<Vec<_>>())
    } else {
        let selector = SessionSelector {
            host: options.get("--host").cloned(),
            desktop: options.get("--desktop").cloned(),
            thread_id: options.get("--thread").cloned(),
            project_cwd: options.get("--project").map(PathBuf::from),
        };
        let record = directory
            .resolve(&selector, now()?)
            .map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(record)
    }
    .map_err(|e| e.to_string())?;
    println!("{output}");
    Ok(())
}

fn build_configured_adapter(
    host: DesktopHostKind,
    projects_root: PathBuf,
    state_root: PathBuf,
    rt: &HostRuntimeOptions,
) -> Result<Box<dyn DesktopHostAdapter>, String> {
    match host {
        DesktopHostKind::OpenaiDots => {
            let adapter = match rt.openai_dots.clone() {
                Some(cfg) => OpenAiDotsAdapter::with_runtime(projects_root, state_root, cfg),
                None => OpenAiDotsAdapter::new(projects_root, state_root),
            }
            .map_err(|e| e.to_string())?;
            Ok(Box::new(adapter))
        }
        DesktopHostKind::Cindy => {
            let adapter = match rt.cindy.clone() {
                Some(cfg) => CindyAdapter::with_runtime(projects_root, state_root, cfg),
                None => CindyAdapter::new(projects_root, state_root),
            }
            .map_err(|e| e.to_string())?;
            Ok(Box::new(adapter))
        }
        DesktopHostKind::DeepseekHarness => {
            let adapter = match rt.dsh.clone() {
                Some(cfg) => DeepSeekHarnessAdapter::with_runtime(projects_root, state_root, cfg),
                None => DeepSeekHarnessAdapter::new(projects_root, state_root),
            }
            .map_err(|e| e.to_string())?;
            Ok(Box::new(adapter))
        }
        DesktopHostKind::AntigravityDesktop => {
            let adapter = match rt.antigravity.clone() {
                Some(cfg) => {
                    AntigravityDesktopAdapter::with_runtime(projects_root, state_root, cfg)
                }
                None => AntigravityDesktopAdapter::new(projects_root, state_root),
            }
            .map_err(|e| e.to_string())?;
            Ok(Box::new(adapter))
        }
    }
}

fn run_hosts(args: &[String]) -> Result<(), String> {
    if args.len() < 4 {
        return Err(USAGE.into());
    }
    let sub = args[1].as_str();
    let allowed = &[
        "--host",
        "--projects-root",
        "--state-root",
        "--slug",
        "--title",
        "--delivery-id",
        "--project-id",
        "--project-dir",
        "--session-id",
        "--prompt",
        "--model",
        "--codex-root",
        "--app-server-socket",
        "--cindy-root",
        "--gateway-socket",
        "--dsh-base-url",
        "--antigravity-root",
        "--agy-bin",
    ];
    let options = parse_key_value_pairs(&args[2..], allowed)?;
    let host_raw = options.get("--host").ok_or(USAGE)?;
    let host = DesktopHostKind::parse(host_raw).map_err(|e| e.to_string())?;
    let projects_root = PathBuf::from(options.get("--projects-root").ok_or(USAGE)?);
    let state_root = PathBuf::from(options.get("--state-root").ok_or(USAGE)?);
    let rt = parse_runtime_options(&options)?;
    let mut adapter = build_configured_adapter(host, projects_root, state_root, &rt)?;

    let output = match sub {
        "ensure-project" => {
            let slug = options.get("--slug").ok_or(USAGE)?;
            let title = options.get("--title").map(String::as_str);
            let binding = adapter
                .ensure_sidebar_project(slug, title)
                .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&binding).map_err(|e| e.to_string())?
        }
        "create-session" => {
            let req = CreateSessionRequest {
                delivery_id: options.get("--delivery-id").ok_or(USAGE)?.clone(),
                project_id: options.get("--project-id").ok_or(USAGE)?.clone(),
                project_dir: PathBuf::from(options.get("--project-dir").ok_or(USAGE)?),
                title: options.get("--title").ok_or(USAGE)?.clone(),
                prompt: options.get("--prompt").ok_or(USAGE)?.clone(),
                model: options.get("--model").cloned(),
            };
            let receipt = adapter.create_session(&req).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?
        }
        "deliver-turn" => {
            let req = DeliverTurnRequest {
                delivery_id: options.get("--delivery-id").ok_or(USAGE)?.clone(),
                project_id: options.get("--project-id").ok_or(USAGE)?.clone(),
                session_id: options.get("--session-id").ok_or(USAGE)?.clone(),
                project_dir: PathBuf::from(options.get("--project-dir").ok_or(USAGE)?),
                prompt: options.get("--prompt").ok_or(USAGE)?.clone(),
                model: options.get("--model").cloned(),
            };
            let receipt = adapter.deliver_turn(&req).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?
        }
        _ => return Err(USAGE.into()),
    };

    println!("{output}");
    Ok(())
}

fn run_relay(args: &[String]) -> Result<(), String> {
    if args.len() < 4 || args[1] != "self-dev" {
        return Err(USAGE.into());
    }
    let allowed = &[
        "--host",
        "--projects-root",
        "--state-root",
        "--slug",
        "--title",
        "--slice",
        "--goal",
        "--done-means",
        "--proof",
        "--prior-evidence",
        "--followup",
        "--model",
        "--receipt-out",
        "--codex-root",
        "--app-server-socket",
        "--cindy-root",
        "--gateway-socket",
        "--dsh-base-url",
        "--antigravity-root",
        "--agy-bin",
    ];
    let options = parse_key_value_pairs(&args[2..], allowed)?;
    let host_raw = options.get("--host").ok_or(USAGE)?;
    let host = DesktopHostKind::parse(host_raw).map_err(|e| e.to_string())?;
    let projects_root = PathBuf::from(options.get("--projects-root").ok_or(USAGE)?);
    let state_root = PathBuf::from(options.get("--state-root").ok_or(USAGE)?);
    let slug = options.get("--slug").ok_or(USAGE)?.clone();
    let slice_id = options
        .get("--slice")
        .cloned()
        .unwrap_or_else(|| "L".into());
    let title = options
        .get("--title")
        .cloned()
        .unwrap_or_else(|| format!("{} /poteto-mode ({slug})", host.as_str()));
    let goal = options.get("--goal").ok_or(USAGE)?.clone();
    let done_means = options.get("--done-means").ok_or(USAGE)?.clone();
    let proof_to_show = options
        .get("--proof")
        .cloned()
        .unwrap_or_else(|| "real command output and artifact receipt from scripts/check".into());
    let prior_evidence = options
        .get("--prior-evidence")
        .cloned()
        .unwrap_or_else(|| "docs/pstack-relay.md;.scratch/pstack-relay/decisions.tsv".into());
    let followup_prompt = options.get("--followup").cloned().unwrap_or_else(|| {
        format!(
            "Verify Slice {slice_id} on {} sidebar session for `{slug}` and emit closure receipt.",
            host.as_str()
        )
    });
    let model = options.get("--model").cloned();
    let rt = parse_runtime_options(&options)?;

    let task = PotetoRelayTask {
        slice_id,
        slug,
        title,
        goal,
        done_means,
        proof_to_show,
        prior_evidence,
        followup_prompt,
        model,
    };

    let receipt = run_host_poteto_relay(host, projects_root, state_root, &task, &rt)
        .map_err(|e| e.to_string())?;
    let json_text = serde_json::to_string_pretty(&receipt).map_err(|e| e.to_string())?;
    if let Some(out_path) = options.get("--receipt-out") {
        let p = Path::new(out_path);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        fs::write(p, format!("{json_text}\n")).map_err(|e| e.to_string())?;
    }
    println!("{json_text}");
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    let Some(first) = args.first().map(String::as_str) else {
        return Err(USAGE.into());
    };
    match first {
        "sessions" => run_sessions(&args),
        "hosts" => run_hosts(&args),
        "relay" => run_relay(&args),
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
