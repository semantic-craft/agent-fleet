# 维护节奏

迁移与升级先读[兼容来源、模型选择保护及在途恢复门槛](migration-and-upgrades.md)；历史通过不代表未知新版本可用。

目标是让这套系统长期可用，同时不长出新的桥和副本。

| 频率 | 做什么 | 命令 | 不通过时 |
| --- | --- | --- | --- |
| 每次改配置、升级、重启电脑后 | 本机自检 | `node scripts/fleet.mjs doctor` | 按缺的服务或 connector 去 [runbook](runbook.md) 对应小节处理 |
| 每周（模板 `fleet-weekly-check`，周一 9:30，失败弹通知。主力 Mac 上这个任务在 `~/Library/LaunchAgents/_retired/`、overlay 的 services 也不列它：目前要手动跑，或用 `fleet.mjs launchd fleet-weekly-check` 重新生成并加载） | guard + doctor | `npm run guard && node scripts/fleet.mjs doctor` | 按缺的服务去 [runbook](runbook.md)。本机派活验收另跑 anet `delegate --watch` 与 Cindy 插件 smoke |
| 上游 fork | 不再跟进 | 历史命令已删除。档案见 [ADR-0002](legacy/0002-minimal-agenvo-fork.md) | — |
| 组件没有消费者时 | 移除组件 | 按下文"移除组件" | — |

## 加东西之前先问

1. 上游已经有这个能力吗？（看官方 ANet 文档和已安装二进制的 `guide`）有就用上游的。
2. 官方插件或 `agents wire` 是否已经支持该宿主？选一种接入；只有缺失的宿主能力才由本仓维护薄适配，不新增派发控制面。
3. 只有写了个人值才有意义吗？那就放进 overlay，不放进本仓库；如果连 overlay 都需要大量定制，重新考虑要不要做。
4. 会产生新的常驻进程、新的目录或新的仓库吗？先在 [landscape](landscape.md) 里写清它的角色和移除条件，再动手。

## 移除组件

1. 在 [landscape](landscape.md) 把它的状态改成"移除中"。
2. 用 `launchctl bootout` 停掉服务，plist 移进 `~/Library/LaunchAgents/_retired/`。
3. 跑一次 `fleet.mjs doctor`。本机派活再按 [anet.md](anet.md) 做 smoke。
4. 查清消费者、未推送工作、在途任务及恢复需要；确认冗余后先可恢复地移出本地目录和部署副本。身份、任务与恢复状态不随源码清理删除，并把这些仓库从每日维护任务里摘掉；最后删掉 landscape 里那一行。
5. GitHub 仓库默认归档；删除须用户逐个确认后执行。

## 什么算"维护好了"

- `~/Projects` 里每个 agent 相关目录在 [landscape](landscape.md) 都有一行，状态与实际一致。
- `~/Projects` 符合 [landscape](landscape.md#projects-的布局) 的布局：每个仓库一份停在默认分支的干净克隆，已合并的 worktree 和本地分支已清理。 收尾须遵守根 [AGENTS.md](../AGENTS.md)「worktree 不遗留」。
- 历史部署的 overlay 是 connector 实例 context 的唯一来源，执行 `contexts` 显示全部 in sync。
- 本机派活以 anet `delegate --watch` 与 Cindy 插件 smoke 为准，见 [anet.md](anet.md)。
