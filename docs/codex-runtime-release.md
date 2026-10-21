# 专属 Codex runtime 的发布与回滚

此组件只管理 `io.agent-fleet.codex-runtime`，不修改共享 `codex app-server`、Agenvo connector 或 relay scope。Connector 的 endpoint 切换、实例重批和真实 relay 验收是独立步骤。不能把 runtime `/health` 成功当成派活交接验收完成。

## 首次安装

先完成隔离测试、审稿与合并。配置文件默认位于 `~/.config/agent-fleet/codex-runtime.json`，包含绝对路径 `command`、`stateDir`、`socketPath`、`cwd`、`codexHome`。配置不包含密钥；原生 Codex 沿用既有认证。`socketPath` 必须是新私有 Unix endpoint，不能指向共享 server。`stateDir` 为专属运行时元数据目录。

创建私有配置/元数据目录与 `~/Library/Logs/codex-owned-runtime/`，渲染模板：

```sh
node scripts/fleet.mjs launchd codex-runtime > ~/Library/LaunchAgents/io.agent-fleet.codex-runtime.plist
```

核对模板 Node 路径与 `--config` 路径。**此时不要 bootstrap**：模板使用不存在的 `uninstalled` 入口，不能直接启动；发布步骤先把入口改为不可变版本，再首次启动。

```sh
node scripts/fleet.mjs release codex-runtime --sha <已合并的完整 SHA>
```

首装只在该 label 未注册且配置所指 socket、`stateDir/service.lock` 均不存在时放行。注册状态查询失败不等于不存在。已存在的 socket/锁不会被删除或替换。

发布产物位于 `~/.local/share/agent-fleet/releases/codex-runtime/<SHA>/`，包含 `tree/adapters/codex-runtime`、锁定的 `ws` 依赖和逐文件 manifest。构建在无 TTY 下使用 lockfile 安装依赖，禁止安装脚本与可选原生 addon，只携带 runtime 实际需要的 `ws`。进程从不可变目录运行，不从开发 checkout 运行。

## 升级及回滚

```sh
node scripts/fleet.mjs release codex-runtime --sha <已合并的完整 SHA>
node scripts/fleet.mjs release codex-runtime --rollback
```

先核验发布目录和 plist，再通过**同一个私有 Unix socket**的 `GET /health` 核对服务身份、PID 和全部生命周期单元。任何非 released 或 unknown 工作都会阻止切换。`POST /shutdown` 在服务内原子封闭输入并再次核对资源，避免查询到空闲后恰好又进入新任务的竞态。

只有 shutdown 被同一 PID 接受、旧 PID 确认退出、socket 与专属 service.lock 都由旧服务正常清理后，工具才对自己的 label 执行 bootout/bootstrap。超时、busy、失联或响应不一致均停止，不杀进程、不删锁。模板使用 `KeepAlive: {SuccessfulExit: false}`，正常停机不会被 launchd 立即重拉。

切换后必须看到正确服务身份和新 PID。回滚复用范围、旧 manifest、入口及 plist 哈希校验；pending 记录保留失败后的真实状态。若新服务启动失败并残留 socket/锁，工具不会自动清理后重试，必须先核对自有元数据与进程，按恢复流程处理，不能删除原生 Codex 锁。

首次安装没有旧的 runtime 版本可回滚，`--rollback` 会因旧入口不存在拒绝。撤销首装应先确认所有专属任务已释放，再恢复 connector endpoint 并重批对应 scope；之后经 `/shutdown` 正常停止新服务，核验退出后才移除其 launchd 注册。不要关闭共享 server。

发布需串行；现有工具没有跨进程部署锁或完整的中途故障事务恢复。生产切换应先停止新的派活输入，确认相关原生任务及交互状态，再实施受授权的 connector-only 切换及专用测试会话验收。
