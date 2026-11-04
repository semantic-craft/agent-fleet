# Reason：Desktop 投递边界

Reason 是投递层。它把一件活送进已经打开的 Desktop 窗口，并把 cwd 钉在 `~/Projects` 里的那个项目上。它不是第二套舰队，不保管身份和任务账本，也不代替壳。

本仓不实现 Reason，不 fork 上游，不改 `~/.reason`。这里只写边界。命令全文不复制；版本号只钉在 outer-loop-kit 一处，本页不另钉。

**签到与 setup 另线。** 未签到之前，不要把 Reason 写成已通、已验收，或默认路径已经打绿。

## 和 ANet 怎么分

| | ANet | Reason |
| --- | --- | --- |
| 管什么 | 身份、任务账本、收据、Cindy、无头与烟雾 | 进已开的 Desktop 窗，cwd＝`~/Projects` 中的项目 |
| 不正道 | 用 auto-reply 冒充 Desktop×cwd | 拿来当第二本任务账，或代替 ANet 做跨身份委派 |
| 本仓 | [anet.md](anet.md)、[delegate-via-anet](../skills/delegate-via-anet/SKILL.md) | 本页、[dispatch-desktop-reason](../skills/dispatch-desktop-reason/SKILL.md) |

手机 Grok 与 Desktop 的体验验收是：发出去看得见，结果回得来，cwd 等于项目。主 A 可以换 Grok、Dots 或 Cindy。这条说的是体验，不是签到状态。

## 同活单通道

同一件活只选一条。选定之后，续问和跟进留在这一条上。超时或结果未知时，先回读这一条，不改走另一条重发。

禁止同一任务同时发 ANet task 和 Reason 进窗。

无头、烟雾、Cindy、任务账本走 ANet。要进已经打开的窗口并钉项目目录，走 Reason。

## 不抢壳

codex-host 是壳。Reason 只负责把活送进窗口。本仓两边都不实现、不 fork，也不在技能里抄壳的命令。

## 不做

- 不新建仓，不用 PODAS 当第三 Git 正本（PODAS 只是伞名，`~/Projects` 里没有这个目录）。
- 不把 agenvo、CommHub、门铃桥教回现行步骤。档案在 [legacy/](legacy/README.md)。
- 不代签、不代跑 setup、不改本机 Reason 配置。
