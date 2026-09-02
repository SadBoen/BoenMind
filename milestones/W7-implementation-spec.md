# W7 实现规格:设置「关于」页 + 在线升级

日期:2026-09-02 · 来源:用户两点(①以后未经明示不许发新版本——已立为铁规矩;②实现在线升级+设置左侧加「关于」) · 状态:**已交付(当日;apply 全链留待下次授权发版实战)**

## 1. 铁规矩(先于一切)

**未经用户明确说"发新版本",严禁打 tag / 发 GitHub Release**。push main 不受限。
在线升级的"检查更新"只读 GitHub,不发布,无此限制。

## 2. 方案

- **版本对齐**:workspace 版本 0.1.0-m1 → **0.0.2**(与 release 线一致;此后发版随 tag 同步 bump bm workspace version,登记 PLAYBOOK)。
- **GET /admin/about**:`{version, platform(os/arch), data_dir, repo}`。
- **POST /admin/about/check-update**:查 `api.github.com/repos/$BOEN_UPDATE_REPO(默认 SadBoen/BoenMind)/releases/latest`(UA 必带);三方比较 `tag_name` vs 当前版本(3 段数值);按当前平台选资产(linux-x86_64.tar.gz / windows-x86_64.tar.gz);返回 `{current, latest, update_available, notes, asset}`。
- **POST /admin/about/apply-update**(**仅允许回环地址**——admin 面现为公开挂载欠账,升级=换二进制,必须收口):
  1. check-update;已是最新 → 400;
  2. 下载资产 + .sha256 到 `<data_dir>/upgrade/`,校验和比对(sha2);
  3. tar+flate2 解包到 staging(发布包两平台统一 .tar.gz 格式);
  4. 换装:运行中 exe 改名 .old(Windows 允许)→ 拷入新 exe → 替换 web_dir(dist)→ 合并 plugins/;
  5. 以 `BOEN_UPGRADE_CHILD=1` + 原 args/env/cwd 拉起新进程;本进程优雅停机排空(INV-12)后退出;
  6. 子进程绑定重试 ≤60s(等旧实例让出端口;仅升级子进程重试,常启仍快速失败=单进程铁律)。
- **前端「关于」页**:当前版本/平台/数据目录 + 「检查更新」按钮(结果含发行说明)+「一键升级」(确认→apply→轮询 /health 直到响应→带时间戳强刷页面取新前端)。
- **release.yml**:补 windows 作业(server+plugin+dist 同包 .tar.gz);发布作业改为收集两平台制品后统一建 Release。

## 3. 已知边界

- apply 全链要等**下一次用户授权的真实发版**才能在线实战;本次以「已是最新」拒绝路径+守卫+解包单测兜底;
- 回滚 = 安装目录 `boenmind-server.old-*` 手工换回(不做自动回滚);
- admin 面 token 鉴权仍是公开挂载欠账(BACKLOG 在案),升级端点已按回环限制收敛。

## 4. 验收门

1. 设置左侧出现「关于」;页面显示版本 0.0.2/平台/数据目录;
2. 「检查更新」实测返回 latest=v0.0.2 且「已是最新」(当前 0.0.2);
3. /health 与关于页版本一致;clippy/fmt/全量测试/validate 全绿;
4. 发版后真实升级演练 = 留待用户下次授权发版时进行(届时验证)。
