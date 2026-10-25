# ADR-0001：以 Agenvo 作为唯一的远程控制面

状态：已采纳（2026-10-19）

## 背景
用户从几个入口（ChatGPT、dots、Grok Bot）指挥多台电脑上的多种运行时（Codex、DSH、ZCode、Herdr 里的 CLI agent）。
如果每个"入口 × 运行时"组合各接一条链路，每条都要一套鉴权、账本和教训，行为也会不一致。

## 决定
使用自建 MCP 路线的入口（ChatGPT、dots、Grok Bot）作为 MCP 客户端，连接同一个 Agenvo relay；每台电脑上的每种运行时只挂一个 connector。
新增入口不写新桥，只要它支持 OAuth + Streamable HTTP 的 MCP 即可。

## 理由
- 上游已经解决出站连接、断线重连、设备配对、OAuth、事件推送和 64 KiB 帧限制这些通用问题。
- 上游的设计原则（Thread 粒度、保留原生语义、connector 只负责连接）正好约束住我们容易越写越大的冲动。

## 代价
- 依赖一个尚在预览阶段、未经安全审计的上游项目（v0.2.0）。
- relay 不做端到端加密。

## 适用范围澄清（2026-10-25）

dot 已有原生平台任务工具和电脑授权时，Codex 优先该通道，不强制绕经 Agenvo。DSH／GrokBot→Agenvo→Codex Desktop 的外部需求仍保留；Agenvo→DSH／Herdr 不变。各调用方分别验收，见[调用方路由](../caller-routing.md)。
