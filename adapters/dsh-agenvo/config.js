/** Non-secret, operator-owned configuration. Never accepts token/header/env/code inputs. */
import { isAbsolute, resolve, relative } from 'node:path';
import { lstatSync, realpathSync } from 'node:fs';
import { homedir } from 'node:os';
const fail = code => { throw Object.assign(new Error(code), { code }); };
const fields = ['relayUrl','nodePath','proxyPath','oauthDirectory','callbackPort'];
export function buildConfig(input) {
  if (!input || typeof input !== 'object' || Array.isArray(input) || Object.keys(input).some(k=>!fields.includes(k)) || fields.some(k=>!Object.hasOwn(input,k))) fail('invalid_config');
  let url; try { url = new URL(input.relayUrl); } catch { fail('invalid_relay'); }
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || url.pathname !== '/mcp') fail('invalid_relay');
  for (const k of ['nodePath','proxyPath','oauthDirectory']) if (typeof input[k] !== 'string' || !isAbsolute(input[k]) || input[k].includes('\0')) fail('absolute_path_required');
  if (!Number.isSafeInteger(input.callbackPort) || input.callbackPort < 1024 || input.callbackPort > 65535) fail('invalid_callback_port');
  // A dedicated directory is required; never consume the common mcp-remote cache.
  const directory=resolve(input.oauthDirectory), shared=resolve(homedir(),'.mcp-auth');
  if (directory === homedir() || directory === shared || !relative(shared,directory).startsWith('..')) fail('dedicated_oauth_directory_required');
  return { transport:'stdio', serverName:'agenvo', command:input.nodePath,
    args:[input.proxyPath,url.href,String(input.callbackPort),'--transport','http-only','--host','127.0.0.1'],
    env:{MCP_REMOTE_CONFIG_DIR:directory}, cwd:directory,
    failOnStartupError:true, toolCallTimeoutMs:35000,
    reconnect:{ enabled:false } };
}
export function verifyPaths(config) {
  // Metadata only: no credential reads, cache creation, package install or shell.
  for(const file of [config.command,config.args[0]]) {
    const info=lstatSync(file);
    if(!info.isFile() || info.isSymbolicLink() || realpathSync(file)!==file) fail('reviewed_regular_file_required');
  }
  const dir=config.env.MCP_REMOTE_CONFIG_DIR, info=lstatSync(dir);
  if(!info.isDirectory() || info.isSymbolicLink() || realpathSync(dir)!==dir || typeof process.getuid!=='function' || info.uid!==process.getuid() || (info.mode & 0o077)) fail('private_oauth_directory_required');
}
