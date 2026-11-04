---
name: dispatch-desktop-reason
description: Choose Reason window delivery or ANet for one task. Same task, single channel. Pointer only; Reason sign-in is a separate track.
---

# Desktop 进窗（Reason）与 ANet

先读 [Reason 边界](../../docs/reason.md)。本技能只帮助选定一条通道。不复制 Reason、ANet 或 codex-host 的命令全文。版本号只看 outer-loop-kit 的那一处钉。

Reason 的签到和 setup 另线。未签到时停在「边界已写明」，不要写成已经接通。

## 何时走哪一条

同一件活先选一条，发出去就不要从另一条补发。

- **ANet**（[delegate-via-anet](../delegate-via-anet/SKILL.md)）：无头、烟雾、Cindy、任务账本。身份、收据和跨身份委派仍只走官方 Agent Network。
- **Reason**：活要出现在已经打开的 Desktop 窗口里，并且 cwd 等于 `~/Projects` 中的那个项目。体验验收是手机 Grok 与 Desktop 双向：看得见、回得来、cwd＝项目。主 A 可以是 Grok、Dots 或 Cindy。

不要用 ANet auto-reply 代替 Desktop 进窗。那条达不到「窗口里看得见、cwd 等于项目」。

## 同活单通道

禁止同一任务同时走 ANet task 和 Reason 进窗。续问留在先选的那一条。超时或结果未知时先回读，不改道重发。

## 不抢壳

Reason 是投递。codex-host 是壳。本仓不实现、不 fork 二者。壳的用法留在上游。

## 签到

登录、本机 setup 和进窗试点不在本技能里做。编制文档不代签。
