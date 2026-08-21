# BoenMind 服务器版（Debian/Ubuntu）

打包内容：`web-server` 二进制 + `dist/`（前端静态资源）+ `install.sh` + `uninstall.sh` + `boenmind.service`。

## 安装

```bash
# 解压 boenmind-server_*_linux-x86_64.tar.gz，进入目录后：
sudo bash install.sh
# 浏览器访问 http://服务器IP:17321
```

install.sh 完成：
1. 创建专用系统用户 `boenmind` 与数据目录 `/var/lib/boenmind`
2. 安装二进制到 `/usr/local/bin/boenmind`
3. 拷贝 `dist/` 前端资源
4. 安装并启动 systemd 服务 `boenmind`（监听 `0.0.0.0:17321`，开机自启）

## 升级

```bash
# 解压新版本 tar.gz，进入目录后再次运行：
sudo bash install.sh
```

升级模式自动：
- 备份数据库到 `/var/lib/boenmind/backups/`（使用 `sqlite3 .backup` 一致快照；最多保留最近 7 份）
- 若检测到你自定义过 systemd 单元（如 `--bind 127.0.0.1`），**不覆盖**它，仅保留你的配置
- 若新服务启动失败，自动回滚旧二进制

## 卸载

```bash
sudo bash uninstall.sh             # 停止/禁用服务，卸载程序，保留数据（boenmind.db、backups、配置）
sudo bash uninstall.sh --purge     # 全部清除（含数据目录；不可恢复，慎用）
```

## 常用运维

```bash
systemctl status boenmind     # 查看状态
journalctl -u boenmind -e     # 查看日志
systemctl restart boenmind    # 重启
```

## 自定义配置（不改原单元）

```bash
sudo systemctl edit boenmind   # 打开后覆盖 ExecStart（先用 ExecStart= 清空，再写新值）：
# [Service]
# ExecStart=
# ExecStart=/usr/local/bin/boenmind --db /var/lib/boenmind/boenmind.db --dist /var/lib/boenmind/dist --port 17321 --bind 127.0.0.1
sudo systemctl daemon-reload && sudo systemctl restart boenmind
```

## 安全提示

服务器版以 HTTP 明文且无登录认证运行，配置中的 API 密钥对能访问该端口的任何人可见。
请仅在可信内网使用，或通过反向代理（nginx/caddy）加访问密码 / HTTPS 后对外，
并把服务绑定到 127.0.0.1（见上方自定义配置）。
HTTP 层目前不内置认证（桌面部有 --auth 登录门，服务器版默认关闭以保持局域网透明访问）。