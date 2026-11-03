# ANet 机制归属与 Projects 盘点

2026-11-02 源码盘点与验证记录。范围为 `~/Projects` 的活动仓库、技能源与安装产物，以及历史材料目录的分类。目标是让可复用派发、跟进、回流和宿主适配由 agent-fleet 维护；官方协议、身份和任务状态机仍由上游 Agent Network 实现。官方入口：<https://docs.agentnetwork.org.cn/docs/zh-cn/latest/intro/overview.html>；当前使用边界见 [anet.md](anet.md)。本表是源码证据，不是全机运行验收。

## 现状、归属与处置

路径均相对于 `~/Projects`。处置列描述目标和本轮分工，不表示已合并或部署；消费者实际合并、安装更新和端到端回流尚未据此得到证明。

| 现路径 | 职责与调用者 | 唯一维护源与处置 | 验证证据／限制 |
| --- | --- | --- | --- |
| `agent-fleet/docs/anet.md`、`plugins/agent-network-cindy/` | 机器接入说明、Cindy 宿主适配；供操作者和宿主使用 | fleet 保留接入与适配；调用官方 MCP | 已读文档与源文件；运行回流须另验 |
| `outer-loop-kit/contracts/dispatch-anet.md` | 外循环调用；现有完整 CLI/MCP、环境、退出码、审查、收据与回流正文 | 通用机制归 fleet；外循环仅保留业务职责和指针，独立 worktree 迁移 | 已读正文、AGENTS、`scripts/check.sh`；初查工作区干净 |
| `outer-loop-kit/contracts/dispatch-agenvo.md` | 历史 stub；README、检查脚本消费 | 迁移引用及检查后才移除旧 stub | `scripts/check.sh` 原第 14–16、33–41 行要求 stub 存在，不能只删文件 |
| `xw-skills/live/xw-stack/skills/delegate-via-anet/` | 宿主技能入口，含 ORIGIN 与测试；完整复制派发机制 | 机制正文与测试归 fleet；xw-stack 保留分发适配和来源指针 | 已读 SKILL；live 是干净 detached worktree，不直接改发行副本 |
| `xw-skills/xw-stack/skills/delegate-via-agenvo/` | 旧技能源码及备份；旧目录、目录索引引用 | 独立 worktree 调整源仓消费者；保留他人改动 | 主 checkout 落后 origin 17 提交，技能、README、检查与文档工具有未提交修改；不可覆盖 |
| `xw-skills/xw-tooling/skills.toml` | 技能选择、安装及宿主接线 | 安装管理继续归 tooling；消费 fleet 技能源，不再维护 ANet 机制正文 | 主 checkout 第 488 行仍登记旧技能并指向 live；需核对当前发行清单后更新，未确认安装完成 |
| `agent-plugins/codex/plugins/xw-stack/` | marketplace 安装产物；含旧 Agenvo 技能和来源记录 | 不是技能源；经分发流程更新，保留其他插件 | 主 checkout ahead 1 / behind 9；AGENTS、README、codex/README 已改，xw-stack 产物未跟踪 |
| `agent-plugins/README.md` | 插件安装导航 | 独立 worktree 已修正旧 ZCode 源码路径，待审查合并 | 原文仍指 `agent-fleet/adapters/zcode`，该目录已不存在；文档修复不等于卸载运行服务 |
| `magpie-ops/tools/agenvo_pilot.py`、`agenvo_config_guard.py` | 2026-10-25 一次性评估；guard 含恢复旧 MCP 配置的执行分支 | 远端已退役：`origin/main` 的 `70e7741`（PR #1）删除两脚本；本地已安全快进，原有未提交文件哈希核对保持不变。不运行恢复分支，不把模型路由并入 ANet | 已读脚本；本仓 AGENTS、README 有他人修改，NOTES 未跟踪；未读凭据或执行脚本 |
| `alma-relay-ops/README.md` | 网络运维者；仍描述 CommHub 门铃 | 远端 `origin/main` 的 `6870966`（PR #2）已澄清 ANet 边界；本地主 checkout 已安全快进，网络底座保留 | 原第 13 行称旧控制面“不砍、本仓不改”；初查工作区干净；未停服务 |
| `_forks/agent-network/` | 远端为 `sleep2agi/agent-network`，README 指向 `anet.sh`；是旧 ANet/CommHub 实现 | 与当前 Research 0.3.9 区分，不并入 fleet、不作当前协议来源；核实运行消费者后才退役 | 已查 remote、README 和 Git 状态（干净）；当前是否仍被旧部署引用未完全确认 |
| `TeslaHub/`、`kotone/`、`vps-ops/` 等 | 共用 AGENTS、产品资料及历史运维引用 | 保留消费者及历史事实；关键词命中不构成冗余实现 | 检索发现引用；未发现需要吞并的 ANet 协议实现 |
| `_status/`、`_artifacts/`、`_shared-work/` | 回执、测试输出、快照、历史证据 | 保留证据职责；与活动源码分开，不因旧名称批量删除 | 历史快照不是维护正本；未逐一阅读所有产物 |

`_forks/agenvo` 在旧 landscape 中出现，本轮目录枚举未发现该路径；不得把旧描述当作当前存在的目录或可复活的安装入口。`dsh-superagent-prototype` 等独立研究上游与业务项目继续保留，不能以名称包含 agent 就删除。

## 验证边界与交接

- 使用 `rg` 搜索 `agenvo`、`agent.network`、`anet`，读取上述关键源码、契约和 AGENTS，并用 `git status --short --branch` 核对相关仓库；未读取私人配置或凭据。
- 活动仓库定向扫描排除了依赖、vendor、输出目录、锁文件及历史产物；点名文件经过读取，其余关键词命中仅是分类线索。未覆盖二进制、全部 ignored 文件或所有隐藏运行状态。
- 首轮广域扫描遇到 `_shared-work/research-infra/rollback/opencli-baseline-3c4a3b1/app-shim.retired` 读取权限错误，未重试或绕过。因此不能声称 Projects 每个文件都已审阅。
- outer-loop-kit 与 xw-stack 消费者集中化、Cindy 适配修复、agent-plugins 文档修复分别在独立工作区推进。后续 remaining_consumers 执行 fetch 确认 magpie PR #1 和 alma PR #2 已进入各自远端 main；旧内容来自尚未更新的本地主 checkout。这里只确认已盘点，最终提交、评审、测试、合并和安装证据由任务交付记录给出。
- 不改他人脏工作区，不把安装根当源仓，不删除独立上游、无关项目或历史回执。尚未完成部署切换或真实跨身份回流验收，源码测试通过也不能替代该验收。

## 本轮交付验证

- 活动源码覆盖：明确枚举 39 个顶层或 xw-skills 子仓，搜索 `agenvo|agent.network|\banet\b`；排除隐藏/ignored、依赖、vendor、输出、二进制和日志。另检查相关全局技能入口；这是限定覆盖，不是全盘无残留承诺。
- Fleet 源码 `npm test` 完整通过（精确提交和数量见 PR 验证记录），private guard 与路由检查通过。Unix socket 测试在沙箱内返回 EPERM，获准在沙箱外临时测试环境重跑后通过；未启动真实 ANet 服务。
- Cindy 恢复与回流定向测试通过，生成包与源文件逐字节一致。本地 callId 账本防止同调用重发，不承诺网络幂等。请求消息 ID 与正文绑定到同一最新用户消息，非文本输入不回退旧正文；宿主状态读取失败不发送进度。
- outer-loop `scripts/check.sh` 通过。xw-stack 全量检查退出 0：182 项（根 52、技能 130）通过；pstack-learn 检查通过，未设置 `PSTACK_SNAPSHOT` 时跳过上游快照哈希核对。此前中断的慢测已由完整重跑取代。
- 本机独立只读 Codex 进程审查四仓精确提交。Fleet 评审发现的 P2 经修复和复审闭环，最终源码结论 merge-ready YES；其他三仓源码审查通过。此为独立源码审查，不是代用户执行 GitHub approval。GitHub 未提供 CI 检查项，不表述为 CI 通过。
- 沙箱内 `turing peers list` 返回 `daemon_not_running`；沙箱外只读核验确认已有节点实际在线，入站仍 closed。没有运行 `anet up`、注册身份、变更 trust/inbound 或网络设置；“待启动许可”已撤销。
- 已有安装版本完成一次真实标记任务：精确标记返回，task completed、delivery confirmed、receipt verified，effect_status 未提供。
- 四仓对齐 PR（`agent-fleet` #108、`outer-loop-kit` #16、`xw-stack` #47、`agent-plugins` #13）均已合并入 `main`，`xw-skills/live/xw-stack` 已快进到合并提交。
- `agent-network-cindy` 0.3.1 修复宿主 `utilityProcess` 包装脚本下 `require.main !== module` 导致 worker 未启动的问题后重新打包（SHA-256 `e2fc9ef1f6f8a30edf47a89f2ffa3b3fea2d8ed9e1d0c4dc7bd5d7b6d3dde226`），已安装到本机 Cindy 宿主并完成双向实测：
  - A 方向（`turing -> cindy -> turing`）：任务 `ix_899f66cdb2889af4e311661f71cbebc6` 返回精确标记 `CINDY_ANET_OK_20261102`，`TASK_STATE_COMPLETED`、`anet.delivery: confirmed`、`anet.receipt_verified: verified`；长任务期间 `[working] Cindy host run remains active; result not yet verified.` 正常回流。
  - B 方向（`cindy -> send_to_peer -> turing`）：Cindy 插件工具 `send_to_peer` 向 `turing` 发起任务 `ix_7e86dae89c3663097931b47f18175c6d` 与用户确认后的 `ix_6d48cf1febec15622c96d403dba9157d`，`turing` provider 收件箱实收 `CINDY_ANET_ORIGIN_OK_20261102`。
- `ps` 与 `lsof` 核实当前无运行进程占用 `~/Projects/_forks/agent-network`。
- 缺失关联账本、宿主取消/续问转发及观察期限后的恢复仍需人工核对原任务与宿主。不声称完整生命周期自动化；未知效果不自动重派。
