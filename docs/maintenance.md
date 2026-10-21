# 维护节奏

目标是让这套系统长期可用，同时不长出新的桥和副本。

| 频率 | 做什么 | 命令 | 不通过时 |
| --- | --- | --- | --- |
| 每次改配置、升级、重启电脑后 | 本机自检 | `node scripts/fleet.mjs doctor` | 按缺的服务或 connector 去 [runbook](runbook.md) 对应小节处理 |
| 每周（launchd `fleet-weekly-check` 自动跑，周一 9:30，失败弹通知） | guard + doctor + 全链路检查 | `npm run guard && node scripts/fleet.mjs check --dispatch` | 先读输出里失败实例的原生状态，再修；修好后在 [lessons.md](../lessons.md) 记一条 |
| 每周（同一个 launchd 任务自动跑） | 跟进上游：上游一出新 release，就在 `~/Projects/.worktrees/agenvo/upstream-<tag>` 里试合并，并跑 check 和 test | `node scripts/fleet.mjs upstream --try-merge` | 结果是 READY：把 `xw/dispatch-connectors` 快进到那个分支，推送，重新部署 relay，`release agenvo`，跑 `check --dispatch`，再删掉 worktree。结果是冲突：只会落在 protocol 枚举和 releases 映射，手动合并 |
| 上游支持外部 kind 时 | 去掉 fork | 按 [ADR-0002](decisions/0002-minimal-agenvo-fork.md) 的退出条件 | — |
| 组件没有消费者时 | 移除组件 | 按下文"移除组件" | — |

## 加东西之前先问

1. 上游已经有这个能力吗？（看 agenvo 的 `docs/` 和 `apps/`）有就用上游的。
2. 是 CLI 型 agent 吗？是就放进 Herdr，不写 connector。
3. 只有写了个人值才有意义吗？那就放进 overlay，不放进本仓库；如果连 overlay 都需要大量定制，重新考虑要不要做。
4. 会产生新的常驻进程、新的目录或新的仓库吗？先在 [landscape](landscape.md) 里写清它的角色和移除条件，再动手。

## 移除组件

1. 在 [landscape](landscape.md) 把它的状态改成"移除中"。
2. 用 `launchctl bootout` 停掉服务，plist 移进 `~/Library/LaunchAgents/_retired/`。
3. 跑一次 `fleet.mjs check`。
4. 删除本地目录、部署副本、状态目录和 `_retired/` 里的 plist（先确认没有未推送的有用工作），并把这些仓库从每日维护任务里摘掉；最后删掉 landscape 里那一行。
5. GitHub 仓库默认归档；删除须用户逐个确认后执行。

## 什么算"维护好了"

- `~/Projects` 里每个 agent 相关目录在 [landscape](landscape.md) 都有一行，状态与实际一致。
- `~/Projects` 符合 [landscape](landscape.md#projects-的布局) 的布局：每个仓库一份停在默认分支的干净克隆，已合并的 worktree 和本地分支已清理。
- overlay 是 connector 实例 context 的唯一来源，执行 `contexts` 显示全部 in sync。
- `check --dispatch` 通过，relay 上除了用户自己的正式授权，没有残留的测试授权。
