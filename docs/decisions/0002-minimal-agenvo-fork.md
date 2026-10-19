# ADR-0002：只为 DSH/ZCode 维护一个最小 agenvo fork

状态：已采纳（2026-10-19）；同日修订：DSH 改用原生 connector，不再需要 jobs 封装

## 背景
上游 Agenvo 没有 DSH 桌面版和 ZCode 桌面版的 connector；用户又要求任务出现在这两个桌面 app 里。

- DSH 桌面版的 Host 有完整的原生会话接口（Typert 端点 `session/list`、`session/page`、`session/follow`、`session/prompt`、`session/cancel`、`session/create` 等），桌面 UI 就用它们；但桌面载体不对外开放 HTTP。我们自己的插件运行在桌面 Host 进程里，可以拿到 `ctx.typertGateway`。
- ZCode 没有可附着的原生管理接口，只有我们的接收端 zcode-dispatch 的 HTTP 契约：`GET /health`、`POST /v1/jobs`、`GET /v1/jobs/<id>`。

## 决定
fork 上游，只做三件事：
1. `packages/protocol` 的实例 kind 枚举新增 `dsh`、`zcode`（relay 会校验这个枚举，所以 relay 也要用 fork 的源码部署）。
2. 新增 `apps/dsh`（命令 `agenvo-dsh`）：插件（dsh-grokbot-dispatch；第二台 Mac 上是 hermes-dsh-dispatch）在同用户 Unix socket 上用一个独立 Peer 转发桌面 Host 的原生端点，connector 附着它，方法名和参数都是 DSH 原生的。覆盖列出会话（含用户手动开的）、读输出、新建、向已有会话追加输入、中断和 `runtime.changed` 事件，与上游 Codex/OpenCode connector 同一水平。设计与依据见 agenvo 的 `design/dsh-connector.zh-CN.md`。
3. 新增 `apps/dispatch`（命令 `agenvo-dispatch`），只服务 ZCode：`health`、`jobs.create`、`jobs.get` 薄封装上面三个 HTTP 调用；对自己派出的 job 轮询并发出 `job.finished` 事件。

fork 里不放部署记录、个人值或本仓库的文档。

## 去掉 fork 的条件
- `apps/dsh` 不含个人值，可以作为 DSH connector 提给上游 Xuanwo/agenvo（连同 `dsh` kind）。它依赖桌面插件提供的 native bridge；bridge 的契约已写进 #53 的上游设计文档；DSH 官方若提供等价的本机附着入口，可以换掉 bridge。
- ZCode 仍只有我们的接收端契约，不提交上游；上游支持外部或插件式的 connector kind 后，把 `apps/dispatch` 移成独立包。
- 两部分都离开 fork 时，删除 fork。

## 不做的事
- 不在 connector 里加队列、重试或调度；原生服务或接收端负责执行，助手负责判断。
- DSH 不再走 jobs 封装：原生接口能做到的，不另造任务模型。
- 不为 CLI 型 agent 写 connector（见 ADR-0003）。

## 补丁怎么放（2026-10-19 起）

- `semantic-craft/agenvo` 是上游的**公开 GitHub fork**。`main` 只用来镜像上游，通过 GitHub 的 "Sync fork" 同步；我们的改动全部在 `xw/dispatch-connectors` 分支上，结构始终是"某个上游提交加上我们的补丁"，不在 `main` 上提交。
- 跟进上游靠 `fleet.mjs upstream --try-merge`，每周自动跑（见 [maintenance](../maintenance.md)）。
- 早期带个人信息的提交历史保存在私有仓库 `agenvo-history`，不再使用。
- 2026-10-19 已把 `apps/dsh`（连同 `dsh` kind、bridge 契约与安装说明、用户指南和隔离测试）提给上游：[Xuanwo/agenvo#53](https://github.com/Xuanwo/agenvo/pull/53)，分支 `semantic-craft/agenvo:dsh-connector`。评审意见在该 PR 里跟进；合并后从 `xw/dispatch-connectors` 删掉 DSH 部分，fork 只剩 ZCode。
