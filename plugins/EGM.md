# plugins 蓝图

## Overview

自制宿主适配的源码集合，现有组件为 Cindy × Agent Network。人工说明保留在 [README](./README.md)，当前派活边界见 [ANet 文档](../docs/anet.md)。

## Subdirectories & Boundaries

- [agent-network-cindy](./agent-network-cindy/EGM.md)：Cindy 工具/任务适配、ANet stdio worker、打包与真实 smoke。

## Interface

- [ghost.json:tools](./agent-network-cindy/ghost.json)：子组件公开 send_to_peer、bridge_status；父目录无独立代码入口。

## Invariants

- [worker.cjs:tick/sendToPeer](./agent-network-cindy/node/worker.cjs)：未知创建/发送保留关联，不自动重复；官方状态与本地效果验证分开。

## Side Effects

- [main.js:createJob](./agent-network-cindy/main.js)、[worker.cjs:sendToPeer](./agent-network-cindy/node/worker.cjs)：子组件可驱动宿主任务和 ANet 发送；父目录本身无运行副作用。

## Verification

仓库根 [package.json](../package.json)：`npm test`；子组件 mock 检查不能代替安装与真实回流。
