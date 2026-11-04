# Receipt: official hub purge (HOST) — 2026-11-04

## Oral (3 lines)
1. **PASS** — all five `anet hub-leave https://hub.agentnetwork.org.cn` → deregistered; sonori witness untouched.
2. Purged: official remote registration (5/5) + plugin-env official-docs comment + rotated/truncated daemon/install logs that still cited official URL.

---

- **When:** 2026-11-04 20:47:24 CST (Asia/Shanghai)
- **Machine:** HOST (`eff7cad1-2f47-4fb5-b403-c0d6aaf4d4e4`)
- **Operator role:** 图灵 · 技術本部長 (executor via Aristotle president order)
- **Witness (canonical):** `https://anet.sonori.dev`
- **Official left:** `https://hub.agentnetwork.org.cn`
- **No backup** of official hub state (president: 不用备份). Before-status text snapshot only under `logs/`.

## Found (before)

| Item | Result |
|------|--------|
| Five live hub_join / config `hub_url` | Already `https://anet.sonori.dev` / `registered` (post-D5) |
| Witness | sonori for all five |
| Active config files citing official URL | **none** |
| Residual official URL text | `daemon.log` (root + turing/cindy), `logs/install-20261101T003536Z.log`; `plugin-env.sh` comment pointed at docs.agentnetwork.org.cn |
| Join tokens / stale hub_join for official | **none** in live anet homes |
| Recovery `~/.anet-recovery/*.anetrec` | Current sonori-era files only (kept) |

Before snapshot: `logs/before-official-purge-status-20261104-204439.txt`

## Actions (deleted / cleared)

| Action | Detail |
|--------|--------|
| CLI `anet --id <id> hub-leave https://hub.agentnetwork.org.cn` | default, turing, cindy, agy, dsh — each returned `status: deregistered`, `undelivered: 0`, exit 0 |
| Idempotent re-check (default) | Official returns `404 not_found` / nothing to leave; local status still sonori Registered |
| `~/.anet/plugin-env.sh` | Comment retargeted to sonori witness (exports unchanged) |
| Log residue | Copied then truncated: root/turing/cindy `daemon.log` + install log → `logs/rotated-daemon-logs-20261104-204628/` |

Leave logs: `logs/hub-leave-official-20261104-204554.txt`, `logs/hub-leave-official-rest-20261104-204609.txt`

## Post-status table

| id | hub | witness | find |
|----|-----|---------|------|
| default | Registered at https://anet.sonori.dev | https://anet.sonori.dev | `anet --id default find` **exit 0** |
| turing | Registered at https://anet.sonori.dev | https://anet.sonori.dev | — |
| cindy | Registered at https://anet.sonori.dev | https://anet.sonori.dev | — |
| agy | Registered at https://anet.sonori.dev | https://anet.sonori.dev | — |
| dsh | Registered at https://anet.sonori.dev | https://anet.sonori.dev | — |

After snapshot: `logs/after-official-purge-status-20261104-204638.txt`

## Constraints checklist

- [x] Identities remain Registered at sonori
- [x] No Hangzhou hub operator actions
- [x] peers.allow unchanged
- [x] No official-state full backup tree; status text only
- [x] No app code / builds

## SHA256

- `logs/before-official-purge-status-20261104-204439.txt`: `605d74846eea9e587a7a6aa86323cbef57e001e9bcdeb5b959ada828b5c44e32`
- `logs/after-official-purge-status-20261104-204638.txt`: `5c33deb051be68e613f15a70a036e7b855cc84c40c391325c4ce7837d187a419`
- `logs/hub-leave-official-20261104-204554.txt`: `aba444840574da67866132eb39fdc10bf7f19cf52fbbb23c66e3ddcc11419ee1`
- `logs/hub-leave-official-rest-20261104-204609.txt`: `27adce41ce556f32057f6e34e14e6e0344fd6fad004a19c055c5959b05f58c2c`

- **This receipt** `receipt-official-purge-20261104.md`: `2ab2db07324a0fb8d7d5ee15de1853c2a005cb38ce22286a369475700b17132d`
