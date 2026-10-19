# ADR-0002：只为 DSH/ZCode 维护一个最小 agenvo fork

状态：已采纳（2026-10-19）

## 背景
DSH 桌面版和 ZCode 桌面版没有 Agenvo 能直接附着的原生管理接口；用户又要求任务出现在这两个桌面 app 里。
我们已有的接收端（dsh-grokbot-dispatch、zcode-dispatch）提供统一的 HTTP 契约：`GET /health`、`POST /v1/jobs`、`GET /v1/jobs/<id>`。

## 决定
fork 上游，只做两件事：
1. `packages/protocol` 的实例 kind 枚举新增 `dsh`、`zcode`（relay 会校验这个枚举，所以 relay 也要用 fork 的源码部署）。
2. 新增 `apps/dispatch`（命令 `agenvo-dispatch`），三个方法 `health`、`jobs.create`、`jobs.get` 分别薄封装上面三个 HTTP 调用；对自己派出的 job 轮询并发出 `job.finished` 事件。

fork 里不放部署记录、个人值或本仓库的文档。

## 去掉 fork 的条件
上游支持外部或插件式的 connector kind，或者 DSH、ZCode 有了 Agenvo 官方 connector。届时把 `apps/dispatch` 移成独立包或直接删掉。
可以向上游提一个"允许外部 kind"的 issue；不提交 DSH/ZCode 专用代码。

## 不做的事
- 不在 connector 里加队列、重试或调度；接收端负责执行，助手负责判断。
- 不为 CLI 型 agent 写 connector（见 ADR-0003）。
