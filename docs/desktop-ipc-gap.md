# Desktop IPC 缺口

开发机已确认：ChatGPT.app（`com.openai.codex`）的内嵌 `codex ... app-server`
由 ChatGPT 父进程通过 stdio/unix-socketpair 连接，没有可供外部客户端连接的命名监听 socket。
不能把共享控制 socket 当作这个内嵌服务器。

- `~/.codex/app-server-control/app-server-control.sock` 的符号链接指向
  `/tmp/codex-daemon-<uid>/<hash>`。已测服务器为 `codex-tui/0.166.0`，
  `thread/loaded/list` 返回 `{"data":[],"nextCursor":null}`，即使 Desktop 有打开的线程。
- `~/.codex/ipc/ipc.sock` 由 ChatGPT 持有，但已测不接受 app-server WebSocket 升级或 RPC。
- System Events 能激活 ChatGPT 并向输入框键入；AX 树不透明，Computer Use 曾拒绝该 bundle。
  这些事实不构成 app-server 的监听路径。
- `codex queue --thread <uuid> --message ...` 写入 `~/.codex/queue_1.sqlite` 的
  `queued_items`，不使 TUI loaded list 出现该线程，也不证明线程在 Desktop 可见。

探针只发现进程、解析 socket，并执行控制 socket 的 `initialize`、`initialized`、
`thread/loaded/list` 和 IPC socket 的 WS 握手。父进程为 ChatGPT 的 app-server
优先分类为 Desktop 内嵌进程，不能仅凭 app-server 参数把它归为 TUI。
探针不会启动服务器、恢复线程、发消息、写队列或操作 UI；`fleet-daemon` 的
`desktop.rs` 保持原样。Hub 正本仍为 **https://anet.sonori.dev**，探针不联系 Hub。

在仓库根目录重跑（Python 3，无第三方依赖）：

```sh
mkdir -p .scratch
python3 scripts/probe-desktop-ipc.py > .scratch/probe-desktop-ipc.out 2> .scratch/probe-desktop-ipc.err
probe_status=$?
printf 'probe exit=%s\n' "$probe_status"
```

默认路径是当前用户的 `~/.codex`；可用 `--codex-home /absolute/path` 显式指定。
每个握手或 RPC 最多等待 3 秒。输出包含路径及 symlink 目标、进程 PID/PPID 与分类、
控制服务器 userAgent、完整 loaded-list JSON，以及 IPC 握手的失败阶段和原因。

退出 **0** 仅表示路径有效、控制服务器 userAgent 已知且不含 `codex-tui`、
loaded `data` 非空；仍需独立证据确认 Desktop 归属与线程可见性。
退出 **2** 表示路径/发现失败、控制端不可达、UA 未知或为 TUI、或 loaded list 为空；
同时输出 `DESKTOP_LOADED_LIST=empty-or-unreachable`。该标记是统一缺口信号，
具体原因看前面的诊断。IPC 握手失败单独报告，不会被误写成 Desktop 连接成功。
