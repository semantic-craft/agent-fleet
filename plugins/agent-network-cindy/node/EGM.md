# Cindy resident worker 组件契约

## Overview

[worker.cjs](./worker.cjs) 以逐行 JSON-RPC 连接 Cindy 插件主入口，另启动外部 ANet CLI 的 MCP stdio 子进程。它保存宿主任务关联和待核对状态；ANet 协议实现属于外部 CLI，Cindy task API 属于宿主。源码没有网络 listener。

## Interface

- [worker.cjs:stdin JSON-RPC](./worker.cjs)：请求必须带非 null id；支持 `dispatch/hello`、`dispatch/done`、`bridge/status`、`anet/send_to_peer`。解析失败返回 -32700，未知方法 -32601，操作错误 -32000。
- [worker.cjs:runJob/flushJobs](./worker.cjs)：向 [main.js](../main.js) 发 `dispatch/job` 通知，kind 为 create/status，以随机 jobId 关联 `dispatch/done`；create 等待 90 秒，status 等待 30 秒。
- [worker.cjs:startMcp/tool](./worker.cjs)：启动 `<anetBin> --id <anetId> mcp`；initialize 的 protocolVersion 为 2025-03-26，然后 notifications/initialized、tools/call。调用 list_tasks/get_task/reply_task/send_message；不是本地实现这些 ANet 工具。
- [worker.cjs:loadConfig](./worker.cjs)：config.json 字段为 anetId、allowPeers、peerAid、可选 anetBin。默认运行目录在 `~/Library/Application Support/CindyGlobal/agent-network-cindy`；anetBin 默认 `~/.local/bin/anet`。
- [worker.cjs:module.exports](./worker.cjs)：导出 `tick`、`state`、`status`、`sendToPeer`、`setTestDependencies`。`AGENT_NETWORK_CINDY_TEST` 禁用启动轮询/stdin 入口；`AGENT_NETWORK_CINDY_DIR_TEST` 覆写运行目录；测试注入替换 tool/runJob/startMcp。
- [worker.cjs:POLL_MS/STATUS_EVERY_MS/MAX_RUN_MS/TASK_HISTORY_LENGTH](./worker.cjs)：10 秒轮询、15 秒宿主观察节流、30 分钟观察窗口；新任务列表 history_length=5，已知任务回读为 100，分页使用 nextPageToken/page_token。
- [worker.cjs:bridge/status/sendToPeer](./worker.cjs)：JSON-RPC result 分别是状态快照与 `{sent,outcome,...}`；两者不自行添加 `{ok,result}` 包装。主入口如何消费取决于 Cindy `node.request` 的宿主返回约定。

## Invariants

- [worker.cjs:loadConfig](./worker.cjs)：anetId 须匹配 1–32 位小写字母/数字/连字符，peerAid 必须在 allowPeers；此处不验证 AID 格式、可执行文件或既有文件权限。
- [worker.cjs:state/writeJson](./worker.cjs)：仅 state.json 不存在时初始化；损坏 JSON、非 version 1 或非对象 tasks 会阻止启动；outgoing 缺失或为假值时初始化，其余必须是非数组对象。保存失败抛错，阻止后续受该保存保护的副作用。创建 mode 为目录 0700/文件 0600；没有既有权限修复或 fsync 承诺。
- [worker.cjs:tick/latestUserMessage](./worker.cjs)：busy 防止轮询重入，未知 peer 被拒绝，重复分页 token 报错；任务绑定最新用户 messageId。创建前重读官方状态与同一消息，正文为空或超过 32768 字符不创建；先保存 CREATE_OUTCOME_UNKNOWN，再请求宿主创建，不自动重建未知 run。
- [worker.cjs:tick](./worker.cjs)：官方终结、续问或关联缺失转 unverified；没有 Cindy cancel/续问转发。30 分钟期限不是远端失败证明；活跃宿主状态才触发五分钟节流的 working 更新。
- [worker.cjs:pickOutput/tick](./worker.cjs)：仅 run.outputMessageId 对应的 assistant 消息可作完成输出；空或超过 60000 字符保持 unverified。终态 reply 之前保存 delivery-unverified，未知回执只读 get_task；delivery-state-observed 不证明效果或回执核验。
- [worker.cjs:sendToPeer](./worker.cjs)：稳定 callId 长度 1–256，SHA-256 作为本地索引；同 callId 不允许换正文/peer，已有 task ID 只回读，无 ID 保留未知。先存 ledger 再 send_message；新 callId 是新请求，不提供网络幂等。IPC 文本 trim 后限制 1–8000 字符。
- [worker.cjs:startMcp](./worker.cjs)：子进程只继承 PATH/HOME/LANG/TMPDIR，不继承 ANET_HOME/ANET_DATA_DIR。stdout 供 JSON-RPC，诊断发 stderr。

## Side Effects

- [worker.cjs:writeJson/saveState/saveStatus](./worker.cjs)：创建运行目录，写 `.tmp` 后 rename 成 state.json/status.json。ledger 可保存请求/回复正文、peer 与宿主关联，必须留在运行目录，不能进入 Git。bridge/status 也会刷新 status.json，并非磁盘零写入。
- [worker.cjs:startMcp/rpc/tool](./worker.cjs)：启动 ANet 子进程并通过其发起真实任务查询、发送和回复。stdin EOF 时 kill 子进程并退出；初始化失败也尝试 kill。
- [worker.cjs:runJob/tick](./worker.cjs)：驱动 Cindy 创建/执行任务、读取 run 与消息；启动 timer 并写宿主 IPC。超过本地观察期限不会停止宿主运行。

## Verification

从仓库根目录运行 `node --test test/cindy-worker.test.mjs`，完整入口为 [package.json](../../../package.json) 的 `npm test`。测试依赖注入不证明宿主 IPC 包装、安装、真实 ANet 回流或用户可见结果。文档验收不启动 worker 或运行 smoke。
