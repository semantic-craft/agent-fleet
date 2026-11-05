# Cindy × Agent Network 组件契约

## Overview

本目录是 Cindy 宿主适配插件。保留 [README](./README.md) 的安装与交付说明；本契约描述源码边界。manifest 当前版本 0.3.1，Node >=24.13.0 来自仓库 package.json，Cindy 最低版本 0.1.121 来自 ghost.json。

## Interface

- [ghost.json:tools/entry/node](./ghost.json)：公开 `send_to_peer(text)` 与 `bridge_status()`；主入口 main.js，常驻 Node 入口 node/worker.cjs，协议 json-rpc-stdio，宿主任务能力 agent.tasks=true。
- [main.js:cindy.onHostMessage/toolCall](./main.js)：消费 tool-call、node-notification/dispatch/job 和 node-status/running。send_to_peer 转 `anet/send_to_peer(text,callId)`，bridge_status 转 `bridge/status`，启动及 worker running 时发送 dispatch/hello。
- [main.js:createJob/statusJob/handleJob](./main.js)：create 使用 cindy.tasks.create/send；status 使用 getRun/readMessages(limit=100)；通过 dispatch/done 回传 jobId/result。Cindy API 来自外部宿主，不在此实现。
- [node 契约](./node/EGM.md)：持久关联、未知结果恢复、ANet MCP 子进程和官方任务回读。
- [pack.py:src/out](./pack.py)：`python3 pack.py <src> <outdir>` 生成 `<id>-<version>.cindy` 并输出文件清单、SHA-256 和大小。输出目录必须已经存在。
- [smoke.sh:an/REQ](./smoke.sh)：真实双向 smoke；REQ_ID 覆写 requester，A/B 分别检查精确标记和 provider inbox。不是离线验证入口。

## Invariants

- [main.js:toolCall/createJob](./main.js)：发送文本 trim 后 1–8000 字符；宿主 create 输入须为字符串且不超过 32768；create/send 使用不同 requestKey 后缀，send 失败保留 taskId。
- [main.js:toolCall/node](./main.js)：消费 `node.request` 返回的 ok/result，空值或异常转失败；worker 的原始 JSON-RPC result 未自带这层包装，需宿主约定匹配，不能由 mock 成功推断真实宿主成功。
- [pack.py:SKIP_NAMES/skip](./pack.py)：跳过点路径、node_modules、dist、evidence、pack.py、smoke.sh、README.md 和 .cindy；检查入口存在与 manifest 长度/id 约束。ZIP 为固定时间、排序文件、DEFLATE、UNIX 权限；文件符号链接不打包。EGM.md 不在排除表，未来重新打包会纳入本目录与 node 的契约；本轮不重建发行包。
- [smoke.sh:set -u](./smoke.sh)：没有 set -e/pipefail；失败可能只输出 CHECK，退出 0 不能代替 A/B PASS 与官方回执核验。

## Side Effects

- [main.js:createJob/statusJob/cindy.onHostMessage](./main.js)：通过宿主创建任务、发送文本、读运行状态/消息并发工具结果；网络与持久状态由 [worker](./node/EGM.md) 承担。
- [pack.py:ZipFile](./pack.py)：读取源文件并写入/覆盖输出包，不执行安装。
- [smoke.sh:EV/delegate](./smoke.sh)：读取本机身份/peer/状态，写 evidence/，真实发起两次 delegate；B 还要求 Cindy 向配置 peer 发送新任务。此目录产物不进入文档改动。

## Verification

从仓库根目录运行 `node --test test/cindy-worker.test.mjs` 或 [package.json](../../package.json) 的 `npm test`。宿主加载与真实 A/B 回流独立验收；不要为文档核对执行 pack、安装或 smoke。
