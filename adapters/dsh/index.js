/**
 * agent-fleet-dsh: mounted in the DSH desktop app's own Host (profile "desktop").
 * Its only job is the native bridge (native-bridge.js): the Host's Typert endpoints and
 * plugin-owned POST /bridge/workspace/attachSession on a same-user Unix socket.
 * The latter uses internal workspaceRegistry, not a native endpoint; DSH upgrades
 * changing that service can return 503 or bridge/attach-failed. The Agenvo `dsh`
 * connector attaches here. No dispatch,
 * queue or job state lives here (agent-fleet ADR-0002).
 */
import os from 'node:os';
import path from 'node:path';
import { installNativeBridge } from './native-bridge.js';

export const name = 'agent-fleet-dsh';
export const DEFAULTS = { stateDir: path.join(os.homedir(), '.local', 'state', 'agent-fleet-dsh') };

export function apply(ctx, config = {}) {
  const cfg = { ...DEFAULTS, ...(config || {}) };
  const logger = ctx.logger ? ctx.logger(name) : console;
  installNativeBridge(ctx, { cfg, logger });
}

export default { name, apply };
