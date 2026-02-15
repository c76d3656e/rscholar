#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT="${PORT:-3000}"
FRONTEND_DIR="$SCRIPT_DIR/front"
DIST_DIR="$FRONTEND_DIR/dist"

GREEN='\033[0;32m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m'

log() {
    echo -e "${BLUE}[start]${NC} $1"
}

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo -e "${RED}[错误] 缺少命令: $1${NC}"
        exit 1
    fi
}

main() {
    require_cmd cargo
    require_cmd npm

    log "安装前端依赖..."
    cd "$FRONTEND_DIR"
    if [[ -f package-lock.json ]]; then
        npm ci
    else
        npm install
    fi

    log "构建前端..."
    npm run build
    cd "$SCRIPT_DIR"

    log "构建 Rust release..."
    cargo build --release

    BIN_PATH=""
    if [[ -x "$SCRIPT_DIR/target/release/Rscholar" ]]; then
        BIN_PATH="$SCRIPT_DIR/target/release/Rscholar"
    elif [[ -x "$SCRIPT_DIR/target/release/rscholar" ]]; then
        BIN_PATH="$SCRIPT_DIR/target/release/rscholar"
    else
        echo -e "${RED}[错误] 未找到后端可执行文件 (Rscholar/rscholar)${NC}"
        exit 1
    fi

    echo ""
    echo -e "${GREEN}Rscholar 启动${NC}"
    echo -e "  URL   : http://localhost:${PORT}"
    echo -e "  Static: ${DIST_DIR}"
    echo -e "  Bin   : ${BIN_PATH}"
    echo ""

    exec "$BIN_PATH" server --port "$PORT" --serve-static "$DIST_DIR"
}

main "$@"
