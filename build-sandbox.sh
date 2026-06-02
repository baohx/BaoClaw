#!/bin/bash
# build-sandbox.sh — 一键构建 baoclaw-sandbox:latest
# 在宿主机终端执行: bash /home/baohx@spdbfl/BaoClaw/build-sandbox.sh
set -e

echo "========================================="
echo "  构建 baoclaw-sandbox:latest"
echo "========================================="

# 1. Docker daemon
echo ""
echo "[1/5] 检查 Docker..."
if ! docker info >/dev/null 2>&1; then
    echo "启动 Docker..."
    sudo systemctl start docker
    sleep 2
fi
echo "✅ Docker 正常 ($(docker version --format '{{.Server.Version}}'))"

# 2. 基础镜像
echo ""
echo "[2/5] 基础镜像..."
docker pull debian:bookworm-slim
echo "✅ debian:bookworm-slim 就绪"

# 3. Layer 1: 系统工具 + Python3
echo ""
echo "[3/5] Layer 1: 系统工具 + Python3 + xz..."
docker rm -f baoclaw-builder 2>/dev/null || true
docker run --name baoclaw-builder debian:bookworm-slim /bin/sh -c \
    'apt-get update && apt-get install -y --no-install-recommends \
     ca-certificates curl git wget xz-utils jq gawk sed findutils procps python3 && \
     rm -rf /var/lib/apt/lists/* && echo L1_OK'
docker commit baoclaw-builder baoclaw-sandbox:step1
docker rm baoclaw-builder
echo "✅ Layer 1 完成"

# 4. Layer 2: Node.js 22 LTS
echo ""
echo "[4/5] Layer 2: Node.js 22 LTS..."
docker run --name baoclaw-builder baoclaw-sandbox:step1 /bin/sh -c \
    'curl -fsSL https://nodejs.org/dist/v22.16.0/node-v22.16.0-linux-x64.tar.xz \
     | tar -xJ -C /usr/local --strip-components=1 && \
     node --version && npm --version && echo L2_OK'
docker commit baoclaw-builder baoclaw-sandbox:latest
docker rm baoclaw-builder
docker rmi baoclaw-sandbox:step1 2>/dev/null || true
echo "✅ Layer 2 完成"

# 5. 验证
echo ""
echo "[5/5] 验证..."
echo "--- node ---"
docker run --rm baoclaw-sandbox:latest node --version
echo "--- npm ---"
docker run --rm baoclaw-sandbox:latest npm --version
echo "--- python3 ---"
docker run --rm baoclaw-sandbox:latest python3 --version
echo "--- git ---"
docker run --rm baoclaw-sandbox:latest git --version

echo ""
echo "========================================="
echo "  ✅ baoclaw-sandbox:latest 构建完成！"
echo "========================================="
docker images | grep baoclaw-sandbox
