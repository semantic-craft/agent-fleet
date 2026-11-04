#!/usr/bin/env node
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { cli, identityArgs, readStatus, runAnet } from './anet-workdir.mjs';

export function hubState(args, run = runAnet) {
  const { prefix, rest } = identityArgs(args);
  if (rest.length) throw new Error('USAGE: anet-hub-state [--id <identity>]');
  const hub = readStatus(prefix, run)?.hub;
  if (typeof hub?.state !== 'string' || hub.state.toLowerCase() !== 'registered') throw new Error('HUB_NOT_REGISTERED');
  let url;
  try { url = new URL(hub.url); } catch { throw new Error('INVALID_HUB_URL'); }
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) throw new Error('INVALID_HUB_URL');
  return url.origin;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli(hubState);
