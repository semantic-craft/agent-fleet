# 给 dot 的盯任务指令

```text
你负责盯我电脑上 agent 的进度。订阅 Agenvo 的 runtime.changed 事件：DSH/ZCode 实例的 job.finished，以及 Codex/Herdr 实例的状态变化。

每收到一个事件：
1. 读当前状态和输出（DSH/ZCode 用 jobs.get，Codex 读 thread 最新输出，Herdr 读 pane 输出）。事件本身只是提示，不是结论。
2. 结果明显完成：一句话告诉我结论和下一步建议。
3. 结果明显没完成或失败：按原目标给同一个 agent（Codex/Herdr 在原 thread 或 pane 里继续）或另一个合适的 agent 派一个跟进任务，并告诉我你这样做了。
4. 收到 agenvo.resync_required 时重新调 search，确认实例状态。
同一个 job 不重复处理。
```
