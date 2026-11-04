use fleet_daemon::SessionDirectory;
use fleet_proto::SessionSelector;
use std::{
    collections::BTreeMap,
    env,
    path::Path,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "usage: fleet-cli sessions <list|resolve> --directory PATH [--host HOST] [--desktop DESKTOP] [--thread THREAD_ID] [--project ABSOLUTE_PATH]";

fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|error| error.to_string())
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() < 4
        || args[0] != "sessions"
        || !matches!(args[1].as_str(), "list" | "resolve")
        || args.len() % 2 != 0
    {
        return Err(USAGE.into());
    }
    let mut options = BTreeMap::new();
    for pair in args[2..].chunks_exact(2) {
        if !matches!(
            pair[0].as_str(),
            "--directory" | "--host" | "--desktop" | "--thread" | "--project"
        ) || pair[1].is_empty()
            || pair[1].starts_with("--")
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err(USAGE.into());
        }
    }
    let directory_path = options.remove("--directory").ok_or(USAGE)?;
    if args[1] == "list" && !options.is_empty() {
        return Err("list does not accept selectors".into());
    }
    let directory =
        SessionDirectory::discover(Path::new(directory_path), now()?).map_err(|e| e.to_string())?;
    let output = if args[1] == "list" {
        serde_json::to_string_pretty(&directory.list().collect::<Vec<_>>())
    } else {
        let selector = SessionSelector {
            host: options.get("--host").map(|v| v.to_string()),
            desktop: options.get("--desktop").map(|v| v.to_string()),
            thread_id: options.get("--thread").map(|v| v.to_string()),
            project_cwd: options.get("--project").map(|v| v.into()),
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

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
