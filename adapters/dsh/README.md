# DSH native bridge plugin (`agent-fleet-dsh`)

A DSH desktop plugin that exposes the desktop Host's own Typert endpoints (`session/list`, `session/follow`, `session/prompt`, ...) on `~/.local/state/agent-fleet-dsh/native.sock` (directory 0700, socket 0600), for the Agenvo `dsh` connector ([ADR-0002](../../docs/decisions/0002-minimal-agenvo-fork.md)). Calls go through `ctx.typertGateway` with one owner Peer that never answers forwarded interactions; results and RemoteErrors come back unchanged.

Install or update: `node scripts/fleet.mjs release dsh`. It builds the pushed commit into `~/.local/share/agent-fleet/releases/dsh/<sha>/plugin`, points `releases/dsh/live` at it, links `agent-fleet-dsh` → `releases/dsh/live/plugin` in the desktop profile, and waits for the bridge to answer from a new process. A running desktop app does not load the new plugin by itself: quit and reopen DeepSeek Harness after every `release dsh`. Then set the `dsh` instance endpoint to `unix://$HOME/.local/state/agent-fleet-dsh/native.sock` and re-approve the instance in the relay (runbook "日常操作").

## Plugin-owned workspace registration

`POST /bridge/workspace/attachSession` accepts `{"args":{"path":"/tmp/<任务目录>/app","sessionId":"<sessionId>","title":"<optional title>"}}`. This is **not a native Typert endpoint**: it uses the Host's internal `workspaceRegistry`. The `/bridge/` prefix keeps plugin methods separate from native `/api/` and `/stream/` forwarding, even if DSH later adds a method with the same name.

The path must be absolute and refer to an existing directory; sessionId and an optional title must be nonempty strings. An existing project keeps its title. For a new project, the default title is the canonical path's basename; a collision with an existing title adds ` · <short parent path>`. The parent drops a leading `/private`, replaces the home prefix with `~`, then drops a leading `/`. For example, `/private/tmp/turing-koe-p1/koe` becomes `koe · tmp/turing-koe-p1` if `koe` already exists; `$HOME/work/koe` becomes `koe · ~/work`. An explicit title overrides this naming rule only for new projects.

Before creating a project, the bridge uses `readSessionHeader` when available and compares the realpaths of the session cwd and requested path. Unknown sessions or mismatched paths fail before creation; `attachSession` remains the final validator. Registration does not take the session write lock, so a running CLI session can be attached and later resumed. The bridge never deletes projects; if header preflight is unavailable or state changes after preflight, a final attach failure can leave the newly created project.

Success is HTTP 200 with `{ok:true,value:{workspace:{workspaceId,path,title,sessionIds},created}}`. Invalid arguments return HTTP 400 with `{error:message}`. An unavailable registry returns HTTP 503 with `{error:"DSH Host workspace registry is not available"}`. Registry failures return HTTP 200 with `{ok:false,error:{code,message,details}}`, preserving the original message and code, or using `bridge/attach-failed` when no code exists.

DSH upgrades that rename the internal service or change its signature can produce 503 or `bridge/attach-failed`. `GET /health` includes `workspaceRegistry: <bool>` for checking service availability after release; it does not prove a session was registered successfully.
