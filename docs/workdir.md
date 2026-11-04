# ANet 项目工作目录

ANet 0.3.9 的工作目录正本是 `anet status --json` 的 `auto_reply.work_dir`。ANet cache cwd 不等于项目根目录；裸 `anet autoreply` 可能 panic，不用它探测配置。

```sh
anet --id turing autoreply set --work-dir /absolute/project
node scripts/anet-workdir.mjs --id turing get
node scripts/anet-workdir.mjs --id turing set /absolute/project
node scripts/anet-workdir.mjs --id turing assert --require /absolute/project
node scripts/anet-hub-state.mjs --id turing
```

脚本继承调用方已选定的 ANet 数据根；`--id` 显式选身份，省略时使用 CLI 默认身份。`set` 设置后回读，`get` 只输出目录；`--require <绝对路径>` 可用于 get / assert，错配非零退出。Hub 脚本只在 Registered 时输出 URL，不输出原始 status、密钥或诊断正文。

每次换项目，重新给执行身份设置 `--work-dir`，派活前 assert；不要把本机调用方的 `pwd` 当作对端证明。材料必须放在 work_dir 下，或通过官方 `--attach` 携带。回执核对实际宿主 cwd：仍在 cache、不等于目标项目根时标记 **HOST_CWD_WRONG**，不能报 Done。配置 assert 通过不替代宿主执行验收。

外循环 `codex -C` 代跑充当 Done 的做法已废止；旧应急绕路 deprecated，不能用于正常派活或宣称 ANet 已修好。由任务所有者在需要时恢复原 work_dir，诊断脚本不自动恢复或改写其他身份。

与 agenvo／Reason 的区别：agenvo 是助手与本机 runtime 的 MCP 中继；Reason Network 提供会话目录与跨机会话总线，Desktop 进窗仍按 [reason.md](reason.md)。本次 SEEN 吸收官方 work_dir 能力，不代表完整迁移，也不证明 Reason 已签到。同一件活只选一条通道。
