---
name: delegate-via-anet
description: Dispatch and follow work through official Agent Network; agent-fleet owns the reusable workflow.
---

# 经官方 Agent Network 派活

本技能是可复用派活流程的唯一维护源。先读 [派活契约](../../docs/dispatch-contract.md) 和 [本机接入](../../docs/anet.md)，再读取目标仓 AGENTS.md。官方协议与工具 schema 以 [官方文档](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/) 和已安装版本为准。编码 agent 用官方 `anet agents wire` 与 `anet mcp` 接入（[接入编码 Agent](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/coding-agents.html)、[MCP](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/mcp.html)）。Herdr 只做 pane 宿主。

1. 明确目标、完整对端 AID、身份和已有授权。只使用已批准的对端；不得自行增加 trust、开放 inbound 或复制身份私钥。allow 与 trust 不等价。
2. 写清任务上下文、仓库、约束、验收及提交权限。开发在独立 worktree；只读任务明确只读。审查者先依用户点名，再依目标仓默认；无覆盖时遵守 agent-fleet AGENTS.md，不能自行改成 agy。记录选择依据及被审提交。
3. 使用官方 CLI `anet --id <身份> delegate <完整AID> "<目标>" --watch` 或当前 schema 的官方 MCP `send_message`。保存 task id、对端和身份。无可用官方入口则报告缺口，不回退旧控制面。调用方已在执行且用户未要求跨身份委派的工作留在当前会话。
4. 对同一 task id 用 `get_task` / `wait_task` 或 CLI 跟进；续问使用同一任务。超时、断连或响应未知先回读，不新建第二次委派。调用之间不要忙等；长任务定期报告状态与未验项。
5. 对照业务验收检查产物，保存契约要求的收据。ANet completed 只证明协议终态；不能代替测试、真实宿主回流、交付或合并证据。

## HOST_CWD 硬门禁

工作目录正本是 `anet status --json` → `auto_reply.work_dir`，不是 ANet cache cwd。派活前对执行身份设置 `anet --id <身份> autoreply set --work-dir <项目绝对路径>`，换项目重设；用 `scripts/anet-workdir.mjs --id <身份> assert --require <项目绝对路径>` 核对。材料在 work_dir 内或用 `--attach`。详细边界见 [workdir](../../docs/workdir.md)。

实际宿主 cwd 仍在 cache、与项目根不符时必须报 `HOST_CWD_WRONG`，不能报 Done。旧 `codex -C` 应急 bypass 已 deprecated；外循环代跑当 Done 已废止，不能替代 ANet 目录修复与宿主验收。今日 G 的 SEEN 吸收官方能力，不是 agenvo／Reason 的完整迁移。

## 同活只走一条

无头、烟雾、Cindy 和任务账本用上面的 ANet 步骤。要进已经打开的 Desktop 窗口，并让 cwd 等于 `~/Projects` 里的项目，改走 [dispatch-desktop-reason](../dispatch-desktop-reason/SKILL.md)，边界在 [docs/reason.md](../../docs/reason.md)。同一件活选定一条之后，不要再从另一条补发。本技能不复制 Reason 命令，也不把未签到的 Reason 写成已通。不要用 ANet auto-reply 代替 Desktop 进窗。

Magpie 属独立可选模型路由。点名模型时核对实际运行模型，不修改私人默认。点名其他执行环境时依该环境现行文档和可用工具；本技能不提供退役 Agenvo/CommHub/Herdr 复活步骤。

来源：从 xw-stack 的 delegate-via-anet 与 outer-loop-kit 派活契约提取；2026-11-03 收敛维护权。消费者只保留入口、业务角色与展示规范。
