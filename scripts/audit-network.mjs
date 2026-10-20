#!/usr/bin/env node
// Read a supplied Surge profile. Never print rule values, hosts, URLs or credentials.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function auditProfile(text) {
  let section = "";
  const general = new Map();
  const rules = [];
  let hostOverrides = 0;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#") || line.startsWith(";")) continue;
    if (/^\[.*\]$/.test(line)) {
      section = line.toLowerCase();
      continue;
    }
    if (section === "[general]") {
      const at = line.indexOf("=");
      if (at > 0) general.set(line.slice(0, at).trim().toLowerCase(), line.slice(at + 1).trim());
    }
    if (section === "[rule]") rules.push(line);
    if (section === "[host]" && line.includes("=")) hostOverrides++;
  }
  const present = key => general.has(key);
  const match = re => rules.filter(line => re.test(line)).length;
  return {
    scope: "static-profile-only",
    activeProfileAndRuntimeRouteVerified: false,
    general: {
      skipProxyConfigured: present("skip-proxy"),
      tunExcludedRoutesConfigured: present("tun-excluded-routes"),
      dnsConfigured: present("dns-server"),
      encryptedDnsConfigured: present("encrypted-dns-server"),
      dnsHijackConfigured: present("hijack-dns"),
      realIpExceptionsConfigured: present("always-real-ip"),
      quicPolicyConfigured: present("block-quic"),
      listenerConfigured: present("http-listen") || present("socks5-listen"),
    },
    rules: {
      total: rules.length,
      tailscaleOrHeadscale: match(/tailscale|tailscaled|headscale/i),
      reasonOrIroh: match(/reason[-.]net|iroh|pkarr|(?:^|[,.])n0\.computer/i),
      udp: match(/PROTOCOL,UDP/i),
      remoteRuleSets: match(/^RULE-SET,/i),
      final: match(/^FINAL,/i),
    },
    hostOverrides,
    caveat: "Remote rule sets and runtime policy choices are not expanded; absence of a static match does not prove absence of a dependency.",
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3) {
    console.error("Usage: node scripts/audit-network.mjs <Surge-profile>");
    process.exitCode = 2;
  } else {
    try {
      console.log(JSON.stringify(auditProfile(readFileSync(process.argv[2], "utf8")), null, 2));
    } catch {
      console.error("Could not read the supplied profile.");
      process.exitCode = 1;
    }
  }
}
