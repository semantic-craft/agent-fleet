# ZCode receiver

What `agenvo-dispatch` calls to run a task as a **visible ZCode.app session** ([ADR-0002](../../docs/decisions/0002-minimal-agenvo-fork.md), [ADR-0004](../../docs/decisions/0004-desktop-strict-and-full-access.md)). Replaces the former `semantic-craft/zcode-dispatch` repository (2026-10-19), cut down to the one path Agenvo uses.

| Route | |
| --- | --- |
| `GET /health` | `{ok, running, runner, app_available, strict_runner: "app", default_model, checks}` |
| `POST /v1/jobs` | `{project, text, source: "agenvo", runner: "app", mode?, model?, timeout_seconds?, job_id?}` → `202` job; `503` when ZCode.app is not running or its scheduler schema changed |
| `GET /v1/jobs/<id>` | `{state: running/completed/failed, runner: "app"/null, session_id, result, error, exit_code, ...}` |

How a job runs ([appsession.mjs](appsession.mjs)): one one-shot row in ZCode's own automation scheduler (`~/.zcode/v2/tasks-index.sqlite`); the app picks it up within ~20 s and opens a normal sidebar session in `~/Projects/<project>` on the desktop login. The receiver polls that row for the outcome and reads the final reply from the session's message log (`~/.zcode/cli/db/db.sqlite`, opened read-only): the last assistant message's last text part. A session that finishes without an assistant reply yields a null result. Not picked up within 90 s, a model other than the requested one, or ZCode.app not running is an explicit failure; there is no CLI or headless path. The default model is GLM-5.3 Flash; any model must be in ZCode's own provider catalog ([models.mjs](models.mjs)).

Local values (never in this repository): `~/.config/zcode-dispatch/key` (0600, `openssl rand -hex 32`, the same file `agenvo-dispatch` uses as `keyFile`) and optionally `~/.config/zcode-dispatch/config.json` (`host`, `port`, `allowedIps`, `projectsRoot`, `denyProjects`, `defaultModel`). Without `host` it binds this machine's tailnet IPv4; `allowedIps` defaults to that host. State and logs: `~/.local/state/zcode-dispatch/`.

Install on a new Mac: `node scripts/fleet.mjs launchd zcode-receiver > ~/Library/LaunchAgents/<prefix>.zcode-receiver.plist`, bootstrap it once, then `node scripts/fleet.mjs release zcode`. Tests: `npm test` at the repository root.
