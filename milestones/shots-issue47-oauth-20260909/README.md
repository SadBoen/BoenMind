# issue #47 门户墙 OAuth/OIDC · 手测证据(2026-09-09)

环境:隔离端口 127.0.0.1:7594(server)+ 127.0.0.1:8765(本地 mock IdP,
Python http.server 实现 /authorize 302 回跳与 /token 背通道);portal.json
配置 oauth 节(密码墙同时启用)。

证据:
1. 01_login_with_sso.png——登录页显示「或使用 SSO(OIDC)登录 →」入口。
2. 02_authed_after_oidc.png——点击后完整走:oauth/login(302→IdP,
   state 防伪)→ /authorize(302 回 callback)→ callback(背通道 code 换
   token,校验 iss/aud/exp)→ 签发 boen_session → 回首页;截图即认证后
   应用界面,/api/portal/state 返回 authed:true。
3. 集成测试 oidc_login_full_flow_and_state_replay_rejected:全链路 +
   state 重放拒 + 密码登录回归;未配置 oauth 时登录页无 SSO 入口、
   行为与现状一致。
4. 测毕:停 server、停 mock IdP、删临时目录、关标签页。
