# agent-fleet Blueprint

## System Overview

本仓库维护多 agent 协作的文档、宿主适配、离线证据校验和保留的运维/harness 库。人工导航与现行派活规则仍在 [README](./README.md) 和 [docs/anet.md](./docs/anet.md)。本蓝图描述本地源码；官方 ANet、Cindy、DSH 与平台原生工具均为外部依赖，不据此声明部署或当前健康。

## Subdirectories & System Boundaries

- [scripts](./scripts/EGM.md)：fleet 配置与模板 CLI、证据检查、可选持久 journal、保留的 DSH harness 与历史路由库。
- [plugins](./plugins/EGM.md)：Cindy 宿主适配、ANet stdio worker、打包与 smoke。
- [派活技能](./skills/delegate-via-anet/SKILL.md)、[架构](./docs/architecture.md)、[模板](./templates/launchd/) 为非代码支持材料，不生成空组件契约。patches/ 是未部署候选补丁与 manifest；test/ 是测试目录，均不作为现行源码组件生成契约。

## Interface

- [fleet.mjs:commands](./scripts/fleet.mjs)：doctor、contexts、launchd、caller-plan；check/release 返回退役提示及退出码 2。
- [ghost.json:tools](./plugins/agent-network-cindy/ghost.json)：Cindy 工具 send_to_peer/bridge_status，内部宿主任务和 MCP 边界见子契约。
- [harness.mjs:HarnessRunner](./scripts/harness.mjs)、[file-journal.mjs:FileJournal](./scripts/file-journal.mjs)：注入 transport 的有界执行/观察和显式目录恢复库，不是官方 ANet 的实现。

## Global Invariants

- [harness.mjs:HarnessRunner](./scripts/harness.mjs)、[worker.cjs:tick/sendToPeer](./plugins/agent-network-cindy/node/worker.cjs)：mutation 前保留尝试记录，未知结果不得以新请求冒充恢复；两套本地存储分别服务各自宿主，不能相互替代。
- [compatibility.mjs:assessCompatibility/assessRecovery](./scripts/compatibility.mjs)：输入证据校验不等于来源认证，合成 canary 不得晋升生产 verified；恢复只观察相同 native ID。
- [lib.mjs:loadConfig](./scripts/lib.mjs)、[worker.cjs:loadConfig](./plugins/agent-network-cindy/node/worker.cjs)：机器 overlay 与插件运行配置留在调用者的配置目录；仓库配置样例不包含实际身份或任务状态。

## Global Side Effects

- [fleet.mjs:contexts/doctor](./scripts/fleet.mjs)：显式 apply 会写 connector 配置；doctor 查询 launchd。
- [harness.mjs:DshProviderAdapter](./scripts/harness.mjs)：可经注入 transport 提交/取消会话；[FileJournal](./scripts/file-journal.mjs) 写私有恢复文件并启动短期锁助手。
- [worker.cjs:startMcp/tick](./plugins/agent-network-cindy/node/worker.cjs)：常驻 worker 启动 ANet MCP 子进程、驱动 Cindy 并保存运行账本；smoke 会发真实任务。离线文档检查不需要这些副作用。

## System Verification

[package.json](./package.json) 指定 Node >=24.13.0 与 `npm test`（Node 测试、private guard、路由 lint）。验收应隔离 HOME/overlay。`npm run check` 仍指向退役 fleet check，会退出 2，不是离线测试替代品。

2026-11-03 bootstrap 使用 EGM.md 保留全部手写 README；BFS 共发现 17 个子目录，已获全树授权，源码剪枝后含根共 5 个契约节点。忽略隐藏/系统、依赖、输出、符号链接、gitignore/egmignore 匹配、纯文档/配置与纯测试目录。差异记录见 [drift report](./docs/2026-11-03_DRIFT_REPORT.md)。
