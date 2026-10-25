/** Optional outbound caller entry. Namespace export preserves Cordis inject metadata. */
import { buildConfig, verifyPaths } from './config.js';
export const name='agent-fleet-agenvo-client';
export const inject=['tools'];
/** Testable composition seam; dependencies are supplied by the host, never model arguments. */
export async function mount(ctx, input, { McpClient, scopeOf, verify = verifyPaths }) {
  const config=buildConfig(input);
  if(!scopeOf(ctx)) throw Object.assign(new Error('scoped_context_required'),{code:'scoped_context_required'});
  verify(config);
  // Let the official plugin validate its own version-dependent configuration.
  const resolved=McpClient.Config(config);
  // Cordis owns the child fiber; unload disposes transport/tools with the parent.
  // No automatic retry of calls, queue, credentials, background wakeup or result store here.
  await ctx.plugin(McpClient,resolved);
}
export async function apply(ctx, config) {
  const [McpClient,{scopeOf}]=await Promise.all([
    import('@deepseek-ai/dsh-mcp-client'),import('@deepseek-ai/dsh-scope'),
  ]);
  await mount(ctx,config,{McpClient,scopeOf});
}
