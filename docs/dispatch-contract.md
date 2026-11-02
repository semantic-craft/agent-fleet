# Agent Network 派活与回流契约

维护源为 agent-fleet；消费者引用本页，不复制 CLI、任务恢复或回流机制。操作入口为 [delegate-via-anet](../skills/delegate-via-anet/SKILL.md)。上游协议来自 [Agent Network 官方文档](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/)，Fleet 不定义第二套身份或任务状态机。

## 派出

使用已安装版本的官方 CLI / MCP。节点持有身份、权限与任务；插件和 agents wire 是接入替代方案，不能为同一身份装两个处理者。环境根目录按 [anet.md](anet.md) 核实；不要把机器配置或 ~/.anet 状态提交到 Git。对端必须是完整 AID，授权扩展须用户明确批准。

保存目标、身份、task id、运行时和已知状态。拿不到 task id 时标记未确认，先查原生状态；断连、超时、未知结果不得自动重派。CLI --watch / MCP wait_task 是观察，不是新的委派。跟进、续问、取消均针对同一任务。取消请求不等于执行端已停止。

## 回程

派工方负责跟进。事件或 SSE 只作为重新读取任务的提示；不能宣称会唤醒离线宿主。没有事件时使用官方查询，不能临时新增 webhook。长时间无进展应报告 task id 与待办，不暗等，也不重派。

分开记录 ANet 终态、实际副作用（effect_status）、回执核验（receipt_verified）与交付（delivery）。按官方结果实际提供的字段回读（见 [MCP 结果](https://docs.agentnetwork.org.cn/docs/zh-cn/latest/howto/mcp.html#reading-a-result)），缺失则记未提供；本地业务验收另列，不伪造协议字段，也不把结果字段添加为调用参数。completed 不能证明业务成功；超时与断连保留未确认/未验证，不能推断执行失败或无副作用。

终态与业务验收后写持久收据；未结束或失败也写明实际状态。收据至少包含：任务、身份/对端 AID、task id、运行时/实际模型（若有）、官方状态、业务结论、证据、未验项和时间。使用同目录临时文件原子替换并回读，避免半份回执。外循环可使用 ~/Projects/_status/<任务>-receipt.md；收据与私钥、任务正文均不进入公共模板仓。

只读烟雾使用唯一标记，要求仅返回该文本、不调用工具或改文件；必须由真实宿主返回并回读同一 task id。源码测试不代替运行验收。

## 消费者边界

outer-loop-kit 保留角色分工、业务审批和汇报格式；xw-stack 保留技能发现入口。安装清单与接线仍由 xw-tooling 维护，插件分发由 agent-plugins 维护。协议机制只在本仓迭代；上游独立源码、运行状态和历史证据不吞并到本仓。
