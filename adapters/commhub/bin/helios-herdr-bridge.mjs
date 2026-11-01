#!/usr/bin/env node
import { CommHubClient } from "../client.mjs";
import { HerdrCommHubBridge } from "../herdr-bridge.mjs";
import { createHerdrHandler } from "../herdr-handler.mjs";

const alias = process.env.ANET_ALIAS || process.env.HERDR_BRIDGE_ALIAS;
if (!alias) {
  console.error("set ANET_ALIAS or HERDR_BRIDGE_ALIAS");
  process.exit(1);
}

const ledgerPath = process.env.HERDR_BRIDGE_LEDGER || null;
const client = new CommHubClient({ alias });
const bridge = new HerdrCommHubBridge({
  alias,
  agent: process.env.HERDR_BRIDGE_AGENT || "herdr",
  version: process.env.HERDR_BRIDGE_VERSION || "herdr-commhub-bridge",
  ledgerPath,
  handler: createHerdrHandler({
    workspaceId: process.env.HERDR_WORKSPACE_ID || null,
    timeoutMs: Number(process.env.HERDR_BRIDGE_TIMEOUT_MS || 29 * 60_000),
  }),
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    void bridge.stop().finally(() => process.exit(0));
  });
}

await bridge.start();
