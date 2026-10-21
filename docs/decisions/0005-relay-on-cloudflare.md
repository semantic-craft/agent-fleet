# ADR-0005：relay 放 Cloudflare，用 Cloudflare 托管的自有域名

状态：已采纳（2026-10-19）

## 决定
relay 用上游的 Cloudflare 部署方式（Worker + SQLite Durable Object + KV），绑定一个 DNS 托管在 Cloudflare 的域名的子域，关闭 `workers.dev` 地址。

## 理由
- 助手（OpenAI、xAI）在海外，直连 Cloudflare 最稳；中国大陆机房的域名在 80/443 上需要 ICP 备案。
- 跨境的只是 connector 那一段长连接，它会自动重连，不影响 agent 运行。
- 不和 VPS 上已有的反代、备份和 Firecrawl 等关键服务挤在同一台机器上。

## 注意
- Workers 自定义域名要求该域名的 zone 在同一个 Cloudflare 账号里；DNS 在别家托管的域名不能直接绑。
- `workers.dev` 子域取自账号名，不是你的品牌；对外地址用自有域名。
- 更换 `BASE_URL` 后，connector 只需改 `relay` 字段并重启，原配对凭据仍然有效；ChatGPT 的连接器要重新添加。
