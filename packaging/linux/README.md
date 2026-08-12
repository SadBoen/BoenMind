# BoenMind 服务器版（Linux）

BoenMind 的后端服务（`bm-server`）单文件版：前端页面已内嵌进二进制，
无需 Node、无需 nginx，解压即用。数据（配置、数据库、插件、pi 密钥）存放在
`/var/lib/boenmind`（可通过 `BOENMIND_HOME` 环境变量修改）。

- 默认端口：`17321`（`BOENMIND_PORT` 可改）
- 默认监听：`0.0.0.0`（`BOENMIND_BIND` 可改，默认 `127.0.0.1` 时仅本机可访问）

## 方式一：systemd 一键安装（推荐）

```bash
tar xzf boenmind-server_*_linux-*.tar.gz
cd boenmind-server_*_linux-*
sudo bash install.sh
```

脚本会创建专用用户 `boenmind`、安装 systemd 服务并开机自启。
完成后浏览器访问 `http://服务器IP:17321`。

常用管理命令：

```bash
systemctl status boenmind     # 查看状态/日志
systemctl restart boenmind    # 重启
journalctl -u boenmind -f     # 跟踪日志
```

## 方式二：Docker

```bash
docker run -d --name boenmind --restart unless-stopped \
  -p 17321:17321 \
  -v boenmind-data:/var/lib/boenmind \
  ghcr.io/sadboen/boenmind:v0.1.1
```

或使用仓库根目录的 `docker-compose.yml`。

## 手动运行

```bash
sudo -u boenmind BOENMIND_HOME=/var/lib/boenmind BOENMIND_BIND=0.0.0.0 ./bm-server
```

## 配置与数据

| 路径 | 内容 |
|---|---|
| `/var/lib/boenmind/.boenmind/config.toml` | 提供商配置、默认模型、工作文件夹 |
| `/var/lib/boenmind/.boenmind/boenmind.db` | 会话与消息（SQLite） |
| `/var/lib/boenmind/.boenmind/pi/keys/` | 提供商 API 密钥（`file:` 引用，不落盘明文 JSON） |
| `/var/lib/boenmind/.boenmind/extensions/` | 插件 |
| `/var/lib/boenmind/BoenMind/` | 默认工作文件夹 |

## 升级

### 方式一：应用内热升级（推荐，systemd 部署）

浏览器打开应用 →「设置 → 关于 → 检查更新」→「立即升级」：

1. 后端从 GitHub Releases 下载新版本二进制并**验证 ed25519 签名**（与官方
   发布同一把密钥，防篡改）；
2. 原子替换自身（旧版本自动备份到 `/var/lib/boenmind/.boenmind/runtime/`）；
3. 进程以 `exec` 方式重启为新版本——**PID 不变**，systemd 无感知，服务只
   中断一两秒，浏览器页面自动恢复。

若升级过程中进程意外退出（断电/崩溃），下次启动会自动完成替换重启
（`.update-pending` 标记机制）。手动回滚：停服务 → 用
`/var/lib/boenmind/.boenmind/runtime/bm-server-<旧版本>-<arch>.bak` 覆盖
`/usr/local/bin/bm-server` → 启动服务。

### 方式二：手动升级

下载新版 tar.gz，重新执行 `sudo bash install.sh`（数据保留）。

> Docker 部署不支持应用内热升级，请用 `docker pull` 更新镜像后重建容器。

## 安全提示

- 当前版本**无登录认证**：配置中的 API 密钥、工作文件夹文件对任何能访问
  该端口的人都可见。请仅在可信内网使用，或通过反向代理（nginx / caddy）
  加访问密码 / HTTPS 后对外。
