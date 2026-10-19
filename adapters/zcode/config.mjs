/**
 * ZCode receiver settings, resolved env > ~/.config/zcode-dispatch/config.json > discovery.
 * No hostnames, IPs or user paths in code.
 *   host        DISPATCH_HOST / "host"          > this node's tailnet IPv4 (`tailscale status --json`); never 0.0.0.0
 *   port        DISPATCH_PORT / "port"          > 7791
 *   allowedIps  DISPATCH_ALLOWED_IPS / "allowedIps" > [host] (the Agenvo connector runs on this machine)
 *   projectsRoot ZCODE_DISPATCH_PROJECTS_ROOT / "projectsRoot" > ~/Projects; "denyProjects" regex > /finance/i
 * Discovery is off with ZCODE_DISPATCH_DISCOVERY=0.
 */
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export const CONFIG_FILE = () => process.env.ZCODE_DISPATCH_CONFIG || path.join(os.homedir(), '.config', 'zcode-dispatch', 'config.json');

export function readConfigFile(file = CONFIG_FILE()) {
  try { const v = JSON.parse(fs.readFileSync(file, 'utf8')); return v && typeof v === 'object' ? v : {}; } catch { return {}; }
}

const TAILSCALE = ['tailscale', '/usr/local/bin/tailscale', '/opt/homebrew/bin/tailscale', '/Applications/Tailscale.app/Contents/MacOS/Tailscale'];
let cached;
export function tailscaleStatus() {
  if (process.env.ZCODE_DISPATCH_DISCOVERY === '0') return null;
  if (cached !== undefined) return cached;
  cached = null;
  for (const bin of process.env.TAILSCALE_BIN ? [process.env.TAILSCALE_BIN] : TAILSCALE) {
    try { cached = JSON.parse(execFileSync(bin, ['status', '--json'], { timeout: 3000, stdio: ['ignore', 'pipe', 'ignore'] }).toString('utf8')); break; } catch {}
  }
  return cached;
}

const ipv4 = (ips) => (ips || []).find((ip) => /^\d+\.\d+\.\d+\.\d+$/.test(ip)) || null;
const list = (v) => (Array.isArray(v) ? v : String(v || '').split(',')).map((x) => String(x).trim()).filter(Boolean);

export function resolveNetConfig({ env = process.env, file = readConfigFile(), status = tailscaleStatus } = {}) {
  const host = env.DISPATCH_HOST || file.host || ipv4(status()?.Self?.TailscaleIPs) || null;
  const port = Number(env.DISPATCH_PORT || file.port || 7791);
  let allowedIps = list(env.DISPATCH_ALLOWED_IPS || file.allowedIps);
  if (!allowedIps.length && host) allowedIps = [host];
  return { host, port, allowedIps };
}

export function projectsRoot(env = process.env, file = readConfigFile()) {
  const p = env.ZCODE_DISPATCH_PROJECTS_ROOT || file.projectsRoot;
  return p ? (p.startsWith('~/') ? path.join(os.homedir(), p.slice(2)) : p) : path.join(os.homedir(), 'Projects');
}

export function denyProjectsRe(file = readConfigFile()) {
  return new RegExp(typeof file.denyProjects === 'string' && file.denyProjects ? file.denyProjects : 'finance', 'i');
}
