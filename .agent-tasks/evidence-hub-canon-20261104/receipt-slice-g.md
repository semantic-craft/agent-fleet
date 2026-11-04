# Receipt — Slice G（cwd 强制项目 abs｜sonori）2026-11-04

- **Time (Asia/Shanghai):** 2026-11-04 21:03 CST
- **Hub:** `https://anet.sonori.dev`｜anet **0.3.9** `d1ad854c`
- **Requester:** turing → **provider:** default `bafyreiffbyp67p2sthuwtp7lx4tb4rhh55gg46cphdhemrxbk5bhjwlmay`
- **Config change (reversible):** `anet autoreply set --work-dir ~/Projects/glaux-book`
  - before: `auto_reply.work_dir` absent（`probe/config.before.json`）
  - after: `work_dir=~/Projects/glaux-book`（`probe/config.after-workdir.json`）

## Hub ix

| Role | ix | Agent report | State |
| --- | --- | --- | --- |
| G2 cwd smoke | `ix_3edefb36ca1ebc39a156c67a3946bc82` | CWD=glaux-book；CACHE_PREFIX=no | COMPLETED |
| G3a AppSupport read | `ix_a1b610f889d69e13e6120c74c121a807` | cwd 绿；仓外 MD_SHA=FAIL | INPUT_REQUIRED（未 message 解卡） |
| G3b in-project SHA | `ix_ce49316934a45097cbf27ae5fc80090a` | CWD=glaux-book；CACHE_PREFIX=no；MD_SHA=943bd6d3…；SHA_MATCH=yes；STATUS=PASS | COMPLETED |

## 5CJ8TDTZ 产物

- Canonical：`…/Application Support/glaux-book/zh-Hans/062_…/James Grimmelmann_2016_不存在计算机创作的作品——而且这也是好事.md`
- sha256：`943bd6d3cc6e2055f103cd10aa8efcb96c6ce7fc90947ccc0e266f4a515c364c`
- Hub 校验副本（同字节）：`~/Projects/glaux-book/.agent-tasks/anet-cwd-force-20261104/5CJ8TDTZ-zh.md`
- **未**使用 `codex exec -C` 顶替；**未** `anet message` 解卡

## Verdict

- **Hub 执行面 cwd：PASS**（非 cache）
- **业务 SHA：PASS**（命中既有 deepseek 产物）
- **Overall：PASS**（正本仍 0.3.9／sonori；官方 `--work-dir`）

## Evidence

- `agenvo-workdir-seen.md` SHA256=`4b0ca280e749a9a2fe4b57eb7cdcad09c148ca5ab79d13245e4d4bd3b4c2b4d3`
- `probe/cwd-smoke-delegate.log`／`g3b-inproject-delegate.log`／`work-dir-set.out`
- PIN.md（社长作废 -C）
