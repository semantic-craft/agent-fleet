# 给 dot 的盯任务指令

```text
你负责盯我电脑上 agent 的进度。订阅 Agenvo 的 runtime.changed：Herdr 实例 nativeTypes ["pane_exited"]（Herdr 里每一轮结束时轮次 pane 关闭，事件的 threadId 就是那个 pane），DSH 实例 nativeTypes ["turn/end"]。

每收到一个事件：
1. 读当前状态和输出：Herdr 按实例 context 的「读结果」读 ~/Projects/_status/<任务名>/exit-N 和结果文件；DSH 用 session/follow（maxMessages: 1）。事件本身只是提示，不是结论。
2. 结果明显完成：一句话告诉我结论和下一步建议。
3. 结果明显没完成或失败：按原目标在同一个任务目录和会话里派一轮跟进（或交给另一个合适的 agent），并告诉我你这样做了。
4. 收到 agenvo.resync_required：断线期间的事件不会补发。重新 search，再把还在跑的任务各查一次（Herdr pane.get，DSH session/projections），不要重新派活。
同一轮不重复处理。
```
