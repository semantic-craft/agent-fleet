/** Native scoped Agenvo MCP connection. No relay, dispatch queue or call retries. */
export async function connectScoped(ctx, { relayUrl, authProvider }, { Client, StreamableHTTPClientTransport, scopeOf, createMcpToolDefinition, fetch }) {
  if (!scopeOf(ctx)) throw new Error('scoped_context_required');
  const url = new URL(relayUrl);
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || url.pathname !== '/mcp') throw new Error('invalid_relay_url');
  if (!authProvider || typeof authProvider.token !== 'function') throw new Error('authorization_required');
  // Credentials are provided by the host service, never tool arguments.
  if (!await authProvider.token()) throw new Error('authorization_required');
  const client = new Client({ name: 'agent-fleet-dsh-native', version: '0.1.0' });
  const transport = new StreamableHTTPClientTransport(url, { authProvider, fetch, onInsufficientScope: 'throw', reconnectionOptions: { maxRetries: 0 } });
  const disposers = [];
  let closed = false, closing;
  const dispose = () => closing ??= (async () => {
    closed = true;
    for (const unregister of disposers.splice(0)) unregister();
    await client.close();
  })();
  ctx.effect(() => dispose, 'agent-fleet-native.connection');
  ctx.on('internal/plugin', fiber => {
    if (fiber === ctx.fiber && fiber.uid === null) return dispose();
  }, { global: true });
  client.onclose = () => { closed = true; for (const unregister of disposers.splice(0)) unregister(); };
  try {
    await client.connect(transport, { timeout: 15000 });
    if (closed) throw new Error('connection_disposed');
    const response = await client.listTools({}, { timeout: 15000 });
    if (closed) throw new Error('connection_disposed');
    // Agenvo's actual public contract: do not register a growing arbitrary catalogue.
    const tools = response.tools;
    if (response.nextCursor || tools.length !== 2 || new Set(tools.map(t => t.name)).size !== 2 || !tools.every(t => ['search', 'execute'].includes(t.name))) throw new Error('unsupported_agenvo_catalog');
    for (const tool of tools) {
      if (tool.execution?.taskSupport === 'required') throw new Error('unsupported_task_tool');
      const definition = createMcpToolDefinition(ctx, {
        name: `mcp__agenvo__${tool.name}`, rawName: tool.name,
        description: tool.description ?? '', inputSchema: tool.inputSchema, outputSchema: tool.outputSchema,
        call: (args, execution) => {
          if (closed) throw new Error('connection_closed');
          return client.callTool({ name: tool.name, arguments: args }, { signal: execution.signal, timeout: 60000, toolDefinition: tool });
        },
      });
      disposers.push(ctx.tools.register(definition));
    }
    return { dispose };
  } catch (error) {
    await dispose().catch(() => {});
    // SDK errors may contain remote response bodies; do not forward them into host logs.
    throw new Error('native_mcp_connection_failed');
  }
}
