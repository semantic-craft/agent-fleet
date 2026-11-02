#!/usr/bin/env python3
"""Pack a Cindy plugin source dir into <id>-<version>.cindy, mirroring Cindy 0.1.121
buildGhostPackage (JSZip, DEFLATE, platform UNIX, file entries only, no folder entries,
skip dotfiles / node_modules / *.cindy, unix perms preserved)."""
import json, os, stat, sys, zipfile, hashlib
src = os.path.abspath(sys.argv[1]); outdir = os.path.abspath(sys.argv[2])
m = json.load(open(os.path.join(src, 'ghost.json')))
out = os.path.join(outdir, f"{m['id']}-{m['version']}.cindy")
SKIP_NAMES = {'.', 'node_modules', 'dist', 'evidence', 'pack.py', 'smoke.sh', 'readme.md'}
def skip(n):
    low = n.lower()
    return n.startswith('.') or low in SKIP_NAMES or low.endswith('.cindy')
files=[]
for root, dirs, fs in os.walk(src):
    dirs[:] = sorted(d for d in dirs if not skip(d))
    for f in sorted(fs):
        if skip(f): continue
        p=os.path.join(root,f)
        if os.path.islink(p) or not os.path.isfile(p): continue
        files.append(os.path.relpath(p, src).replace(os.sep,'/'))
for need in [m['entry'], m.get('node',{}).get('entry')]:
    if need and need not in files: sys.exit(f'missing declared file {need}')
with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
    for rel in files:
        st=os.stat(os.path.join(src,rel)); mode=stat.S_IMODE(st.st_mode) & 0o755
        zi=zipfile.ZipInfo(rel, date_time=(2026,11,2,0,0,0)); zi.create_system=3
        zi.external_attr=((0o100000|mode)<<16); zi.compress_type=zipfile.ZIP_DEFLATED
        z.writestr(zi, open(os.path.join(src,rel),'rb').read())
h=hashlib.sha256(open(out,'rb').read()).hexdigest()
print(out); print('files', files); print('sha256', h, 'bytes', os.path.getsize(out))
