# ADR-0001：以 Agenvo 作为唯一的远程控制面

状态：已采纳（2026-10-19）

## 背景
此前每接一个"入口 × 运行时"组合就要写一个桥（Grok→Codex、ChatGPT DOT→主力 Mac、Hermes→DSH、DSH↔ZCode、Yobiru 消息总线……），
每个桥各有一套鉴权、账本和教训，行为也不一致。

## 决定
所有远程入口（ChatGPT、dots、Grok Bot）都作为 MCP 客户端，连接同一个 Agenvo relay；每台电脑上的每种运行时只挂一个 connector。
新增入口不写新桥，只要它支持 OAuth + Streamable HTTP 的 MCP 即可。

## 理由
- 上游已经解决出站连接、断线重连、设备配对、OAuth、事件推送和 64 KiB 帧限制这些通用问题。
- 上游的设计原则（Thread 粒度、保留原生语义、connector 只负责连接）正好约束住我们以前容易越写越大的桥。

## 代价
- 依赖一个尚在预览阶段、未经安全审计的上游项目（v0.2.0）。
- relay 不做端到端加密。
