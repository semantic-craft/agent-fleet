#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { isAbsolute, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function runAnet(args) {
  try {
    return execFileSync('anet', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 15000 });
  } catch {
    throw new Error('ANET_COMMAND_FAILED');
  }
}

export function readStatus(prefix, run = runAnet) {
  try {
    return JSON.parse(run([...prefix, 'status', '--json']));
  } catch {
    throw new Error('ANET_STATUS_UNAVAILABLE');
  }
}

export function identityArgs(args) {
  const rest = [...args];
  const index = rest.indexOf('--id');
  if (index < 0) return { prefix: [], rest };
  const id = rest[index + 1];
  if (!id || id.startsWith('-')) throw new Error('INVALID_ID');
  rest.splice(index, 2);
  return { prefix: ['--id', id], rest };
}

function absolute(value) {
  if (typeof value !== 'string' || !isAbsolute(value)) throw new Error('ABSOLUTE_WORK_DIR_REQUIRED');
  return resolve(value);
}

export function workdir(args, run = runAnet) {
  const { prefix, rest } = identityArgs(args);
  const command = rest.shift() ?? 'get';
  let required;
  if (command === 'set' && rest.length === 1) required = absolute(rest.shift());
  else if (['get', 'assert'].includes(command)) {
    if (rest.length === 2 && rest[0] === '--require') {
      required = absolute(rest[1]);
      rest.splice(0);
    }
    if (rest.length || (command === 'assert' && !required)) throw new Error('USAGE: get|set <abs>|assert --require <abs> [--id <identity>]');
  } else throw new Error('USAGE: get|set <abs>|assert --require <abs> [--id <identity>]');
  if (command === 'set') run([...prefix, 'autoreply', 'set', '--work-dir', required]);
  const current = absolute(readStatus(prefix, run)?.auto_reply?.work_dir);
  if (required && current !== required) throw new Error('HOST_CWD_WRONG');
  return current;
}

export function cli(action) {
  try { console.log(action(process.argv.slice(2))); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli(workdir);
