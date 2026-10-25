# 原生独立会话的显式选择

适用：用户明确要在 Codex Desktop 创建独立会话，并指定模型和思考档位。
入口仍是宿主提供的原生 `create_thread`，不新增调度器、不接管模型默认或权限。
新会话 / 子 agent 的意图区分由人类请求决定；无法从“开两个 agent”单独推断。
输入框模型选择描述的是该会话，不是它的子 agent。

## 一份选择，两个核对点

`scripts/native-thread-selection.mjs` 是显式选择的唯一参数构造器。先按宿主实时 schema
与模型目录准备 JSON，目录只记录实际支持的模型 ID 和档位，不在仓库复制型号清单：

```json
{
  "intent": {
    "kind": "independent-thread",
    "selection": {"model": "provider/model-id", "thinking": "high"},
    "request": {
      "title": "A · scoped ticket",
      "prompt": "目标、独占工作树、文件范围、验证与交付边界",
      "target": {"type": "project", "projectId": "discovered-project-id", "environment": {"type": "local"}}
    }
  },
  "catalog": [{"model": "provider/model-id", "efforts": ["low", "high"]}]
}
```

1. `node scripts/native-thread-selection.mjs prepare <项目内意图.json>`：产生原生工具参数，
   直接传给 `create_thread`，不要重新手写参数而漏掉 `model` / `thinking`。
   或以 `{intent, tool, args}` 调用 `check-request` 检查拟提交请求。无匹配目录就失败，
   不降档、不换模型、不改默认；未知创建结果先回读原任务，不能再次创建。
2. 保存原生 threadId；通过宿主读回该线程的实际轮次、模型/档位、回复或工具执行。
   用 `{intent, threadId, observation:{threadId,turnId,model,thinking,executed:true}}`
   调用 `check-observation`。`executed` 必须来自实际回复/工具记录，不能由 active 推断。
   证据源路径或工具回执保存在所属项目，离线检查器不认证输入证据的真实性。
3. 选择验证只给 `execution-observed`。Desktop 选中态、实际底层 provider、工作树是否正确、
   业务验收各自留证；工具禁止操作 Desktop 时保留 UI 未验，不换接口绕过。

已有执行者时先取得停止/交接确认，保留 Git 修改和执行回执，再交接原工作区。
GitHub / 项目事实页由协调者单一写入；接收方不能把已有未提交改动当成可丢弃的脏文件。
这些属于跨工具的协调判断，参数检查器不伪装成分布式锁。

## 执行约束与限制

| 约束 | 机制 |
| --- | --- |
| 显式模型/档位不能漏传或重复拥有 | prepare 只从 selection 派生；check-request 拒绝不一致 |
| 独立会话不能用子 agent 交付 | check-request 拒绝 spawn_agent |
| accepted 或别的线程不能当本轮选模通过 | check-observation 核对 threadId、turnId、执行证据及模型/档位 |
| 同一缺陷不随重构回来 | `npm test` 运行历史漏参/错误会话类型/错误回读回归 |

宿主内置工具并不由本仓维护，直接调用可绕过本检查器；本模块是可执行入口合同，
不是平台级强制 hook。宿主日后提供可注册的调用拦截点时再接入，不能虚构已有挂钩。
本仓路由 lint 要求原生派工指针存在；它不能证明 agent 执行过步骤。

2026-10-25 的证据：用户明确独立会话之前，代理创建了使用默认 Astra/medium 的协调会话，
只在提示词里要求 DeepSeek/high 子 agent；用户截图揭示会话层级不同。最终两条独立
会话创建参数及原生 turn context 均为所选 DeepSeek/high，并有命令执行。模型目录和
UI 显示不等同于供应商 HTTP 请求证明。本例不需要重读第三方 UI 项目才能复用。
