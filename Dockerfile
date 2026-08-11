# syntax=docker/dockerfile:1
# BoenMind 服务器版 Docker 镜像（多架构：linux/amd64、linux/arm64）
#
# 构建：docker build -t ghcr.io/sadboen/boenmind:v0.1.1 .
# 运行：docker run -d -p 17321:17321 -v boenmind-data:/var/lib/boenmind \
#         ghcr.io/sadboen/boenmind:v0.1.1

# ---------- 阶段 1：前端构建 ----------
FROM node:22-slim AS frontend
WORKDIR /app
RUN corepack enable
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

# ---------- 阶段 2：后端构建（内嵌前端产物，--features embed） ----------
FROM rust:1.97-slim AS backend
WORKDIR /repo
COPY backend/ ./backend/
# rust-embed 的 folder 相对 crate 根解析为 <repo>/frontend/dist，必须与仓库布局一致
COPY --from=frontend /app/dist ./frontend/dist
RUN cargo build --release --manifest-path backend/Cargo.toml -p bm-server --features embed

# ---------- 阶段 3：运行时 ----------
FROM debian:bookworm-slim
RUN useradd --system --home-dir /var/lib/boenmind --shell /usr/sbin/nologin boenmind \
    && mkdir -p /var/lib/boenmind && chown -R boenmind:boenmind /var/lib/boenmind
COPY --from=backend /repo/backend/target/release/bm-server /usr/local/bin/bm-server
ENV BOENMIND_HOME=/var/lib/boenmind \
    BOENMIND_BIND=0.0.0.0
VOLUME ["/var/lib/boenmind"]
EXPOSE 17321
USER boenmind
ENTRYPOINT ["bm-server"]
