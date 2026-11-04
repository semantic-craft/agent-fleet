# BRIEF — agent-fleet 工程化落今日 ANet 经验（2026-11-04）

## Goal
把 HOST 今日已验证的 ANet 运维经验落进 **semantic-craft/agent-fleet** 私有正本仓：可换机重建的脚本＋文档＋测试，禁止只写 `_status` 记帐。

## President voice (must honor)
「搞定的经验要变成我们自己的一套」——在官方 Agent Network 之上维护私有体系；严肃工程项目；代码真实落仓；换电脑也能重建；对 ANet 做底层改造级落地，不是账本。

## Current truth (SEEN 2026-11-04 Asia/Shanghai)
- Hub 正本＝自建 `https://anet.sonori.dev`（ANetHub 0.3.3）；HOST 五身份 Registered；官方 `hub.agentnetwork.org.cn` **已清仓**（非现行回滚正本）。
- anet CLI＝0.3.9；cwd 正本＝`anet autoreply set --work-dir <项目abs>`（官方）；材料须在 work_dir 内或 `--attach`；**禁止**把外循环 `codex -C` 当 Done。
- default 现钉 `~/Projects/glaux-book`（本任务期间可临时改为本仓根；**结束必须恢复 glaux-book**）。
- 身份：default／turing／cindy／agy／dsh；recovery passphrase **禁进 Git／禁聊天明文**（仅 HOST PRIVATE）。
- agent-fleet README／docs/anet.md **过时**：仍写「不经 anet.sonori.dev」——必须改为现网正本。

## Scope (minimal, G≥F style)
1. **Canon docs**（必做）
   - 改 `README.md`、`docs/anet.md`、`docs/architecture.md`：Hub＝`https://anet.sonori.dev`；删／挪「sonori＝遗留」到真正遗留附录且标注日期。
   - 新增/更新 `docs/workdir.md`（或并入 anet.md）：`autoreply set --work-dir`、换仓重设、材料边界、HOST_CWD_WRONG 判据、与 agenvo/Reason 对照（SEEN 吸官方，非整迁）。
   - `docs/new-mac.md`：换机重建清单（装 anet 0.3.9、五身份、`hub-register` 到 sonori、设 work_dir、peers allow）——**不含** recovery 明文。
2. **Scripts（必做，可测）**
   - `scripts/anet-workdir.mjs`（或 .sh）：get／set／assert work_dir；`--require <abs>` 失败非 0。
   - `scripts/anet-hub-state.mjs`：打印 Registered hub URL（只读；无私钥）。
   - `test/` 对 workdir 脚本做至少 1 个纯函数／mock 测（不碰真 daemon 也可）。
3. **Lessons**
   - `lessons.md` 顶插条目：cache cwd≠项目根；正本 work_dir；`-C` 顶替作废。
4. **Skills pointer**
   - 若仓内有 `skills/delegate-via-anet`：把 HOST_CWD 硬规则改成 work_dir 正本（对齐今日 G），旧 `-C` 仅作紧急旁路并标作废收口。

## Out of scope
- 不改 ANet 上游二进制；不 fork 协议
- 不提交 identity／PRIVATE／token／recovery
- 不碰 Sonori 播报／iPhone
- 不把 `_status/agy-five-dispatch*` Desktop F residual 整迁；F 另轨

## Constraints
- worktree 分支 `agent/fleet-canon-workdir-hub-20261104` 基于 origin/main；主 checkout 保持干净
- squash-ready PR；允许 push＋开 PR
- 官方文档优先；只记本机／私有体系增量
- 中文用户向说明可保留；代码／CLI 英文标识

## Done
1. PR 开出且 CI／`npm test`（或仓内等价）绿
2. `rg 'anet.sonori.dev' README.md docs/anet.md` 为正本用法（非「遗留」）
3. `node scripts/anet-workdir.mjs --help`（或等价）可用
4. 本机 default `auto_reply.work_dir` **已恢复** `~/Projects/glaux-book`
5. 回执写 `~/Projects/_status/anet-fleet-engineer-20261104/receipt.md`（task id、PR URL、SHA）

## Evidence sources (read-only)
- `~/Projects/_status/anet-cwd-force-20261104/receipt-slice-g.md`
- `~/Projects/_status/anet-hub-d-cutover-20261104/receipt-d5*.md`（勿抄 PRIVATE）
- `~/Projects/_status/anet-hub-d-cutover-20261104/receipt-official-purge-20261104.md`
