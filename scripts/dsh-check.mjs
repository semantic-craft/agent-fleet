/** The fleet check's DSH branch. Importing this module has no CLI/auth side effects. */
import { randomBytes } from 'node:crypto';
import { TaskSpec, RunContext, DshProviderAdapter, HarnessRunner } from './harness.mjs';
const READ_ONLY = '只读验收：用一句话说明这个目录是什么项目，不要修改任何文件。';

export async function dshTurn(call, { root, project, sessionId }) {
  const fail = text => ({ ok: false, sessionId, text });
  try {
    if (!sessionId) {
      if (!root) return fail('no projectsRoot for this machine in fleet.json');
      const ws = await call('workspace/create', { request: { path: `${root}/${project}` } });
      if (ws?.error || !ws?.result?.workspace?.workspaceId) return fail('workspace_create_failed');
      const created = await call('session/create', { request: { workspaceId: ws.result.workspace.workspaceId } });
      if (created?.error) return fail('session_create_failed');
      sessionId = created?.nativeIds?.sessionId;
      if (!sessionId) return fail('session_create_failed');
    }
    const runner = new HarnessRunner(new DshProviderAdapter(call));
    const result = await runner.run(
      new TaskSpec({ objective: READ_ONLY, acceptance: { kind: 'nonempty-text' } }),
      new RunContext({ requestId: `fleet-check-${randomBytes(12).toString('hex')}`, sessionId, lessonsVersion: 'fleet-check-v1' }),
    );
    return { ok: result.status === 'success', sessionId, text: `${result.status}: ${result.code}` };
  } catch {
    // Creation errors may contain credentials or workspace data; never echo them.
    return fail('check_failed');
  }
}
