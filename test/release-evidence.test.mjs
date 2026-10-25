import test from 'node:test';import assert from 'node:assert/strict';
import {mkdtempSync,writeFileSync,rmSync,symlinkSync} from 'node:fs';import {tmpdir} from 'node:os';import {join} from 'node:path';import {createHash} from 'node:crypto';
import {verifyRelease} from '../scripts/release-evidence.mjs';
const sha='a'.repeat(40),hash=createHash('sha256').update('fixture').digest('hex');
function fixture(t){const r=mkdtempSync(join(tmpdir(),'release-evidence-'));t.after(()=>rmSync(r,{recursive:true,force:true}));writeFileSync(join(r,'file'),'fixture');writeFileSync(join(r,'release.json'),JSON.stringify({sha,files:{file:hash}}));return r;}
test('verifies pinned files without authenticity or process claims',t=>{const r=verifyRelease(fixture(t),sha);assert.equal(r.status,'verified-files');assert.equal(r.sourceAuthenticityProven,false);assert.equal(r.runningProcessProven,false);});
test('wrong or missing revision blocks',t=>{const r=fixture(t);assert.equal(verifyRelease(r).status,'blocked');assert.equal(verifyRelease(r,'b'.repeat(40)).status,'blocked');});
test('modified and extra files block',t=>{const r=fixture(t);writeFileSync(join(r,'extra'),'private');assert.equal(verifyRelease(r,sha).status,'blocked');rmSync(join(r,'extra'));writeFileSync(join(r,'file'),'changed');assert.equal(verifyRelease(r,sha).status,'blocked');});
test('unsafe manifest names block',t=>{const r=fixture(t);for(const n of ['../private','/private','a/../file','a\\b']){writeFileSync(join(r,'release.json'),JSON.stringify({sha,files:{[n]:hash}}));assert.equal(verifyRelease(r,sha).status,'blocked');}});
test('symlink entries rejected without exposing targets',t=>{const r=fixture(t);rmSync(join(r,'file'));symlinkSync('/nonexistent-private',join(r,'file'));assert.deepEqual(verifyRelease(r,sha),{status:'blocked',code:'release_evidence_invalid'});});
test('invalid manifest produces fixed error',t=>{const r=fixture(t);writeFileSync(join(r,'release.json'),'PRIVATE_INVALID');assert.deepEqual(verifyRelease(r,sha),{status:'blocked',code:'release_evidence_invalid'});});
