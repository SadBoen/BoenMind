# BoenMind 服务器版（Debian/Ubuntu）

打包内容：`web-server` 二进制 + `dist/`（前端静态资源）+ `install.sh` + `boenmind.service`。

## 安装（目标 Debian 服务器）

```bash
# 解压 boenmind-server_*_linux-x86_64.tar.gz，进入目录后：
sudo bash install.sh
# 浏览器访问 http://服务器IP:17321
```

install.sh 完成：
1. 创建专用系统用户 `boenmind` 与数据目录 `/var/lib/boenmind`
2. 安装二进制到 `/usr/local/bin/web-server`
3. 拷贝 `dist/` 前端资源
4. 安装并启动 systemd 服务 `boenmind`（监听 `0.0.0.0:17321`，开机自启）

## 卸载

```bash
sudo systemctl disable --now boenmind
sudo rm /etc/systemd/system/boenmind.service
sudo systemctl daemon-reload
sudo rm /usr/local/bin/web-server
sudo rm -rf /var/lib/boenmind   # 删除数据（含数据库与配置）
sudo userdel boenmind
```

## 常用运维

```bash
systemctl status boenmind     # 查看状态
journalctl -u boenmind -e     # 查看日志
systemctl restart boenmind    # 重启
```

## 安全提示

服务器版以 HTTP 明文且无登录认证运行，配置中的 API 密钥对能访问该端口的任何人可见。
请仅在可信内网使用，或通过反向代理（nginx/caddy）加访问密码 / HTTPS 后对外。
HTTP 层目前不内置认证（桌面部有 --auth 登录门，服务器版默认关闭以保持局域网透明访问）。