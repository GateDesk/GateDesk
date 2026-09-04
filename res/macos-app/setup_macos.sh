#!/usr/bin/env bash
# ============================================================
# RustDesk macOS 编译环境一键配置脚本
# 依据官方文档 https://rustdesk.com/docs/zh-cn/dev/build/osx/
#
# 用法:
#   ./setup_macos.sh                 # 仅安装全部依赖(Sciter + Flutter 可选)
#   ./setup_macos.sh --sciter        # 额外下载 Sciter 组件
#   ./setup_macos.sh --flutter       # 额外安装 Flutter 工具链
#   ./setup_macos.sh --build         # 依赖装完后直接构建(默认 Sciter 版)
#   ./setup_macos.sh --build --flutter  # 构建 Flutter 版
#
# 可覆盖的环境变量: REPOS_DIR VCPKG_ROOT VCPKG_TAG RUST_VERSION
#                   FRB_VERSION FLUTTER_VERSION
# ============================================================
set -euo pipefail

# ---------- 可配置参数 ----------
REPOS_DIR="${REPOS_DIR:-$HOME/repos}"
VCPKG_ROOT="${VCPKG_ROOT:-$REPOS_DIR/vcpkg}"
RUSTDESK_DIR="${RUSTDESK_DIR:-$REPOS_DIR/rustdesk}"
VCPKG_TAG="${VCPKG_TAG:-2023.04.15}"
RUST_VERSION="${RUST_VERSION:-1.75.0}"
FRB_VERSION="${FRB_VERSION:-1.80.1}"
FLUTTER_VERSION="${FLUTTER_VERSION:-3.16.9}"

WITH_SCITER=0
WITH_FLUTTER=0
DO_BUILD=0

for arg in "$@"; do
  case "$arg" in
    --sciter)  WITH_SCITER=1 ;;
    --flutter) WITH_FLUTTER=1 ;;
    --build)   DO_BUILD=1 ;;
    -h|--help)
      sed -n '2,16p' "$0"
      exit 0 ;;
    *) echo "未知参数: $arg (可用: --sciter --flutter --build -h)" >&2; exit 1 ;;
  esac
done

# ---------- 工具函数 ----------
info() { printf '\033[1;36m[INFO]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[WARN]\033[0m %s\n' "$*"; }
err()  { printf '\033[1;31m[ERR]\033[0m %s\n' "$*"; exit 1; }

# 代理配置：vcpkg/rustup/pip 等下载外网源码必需。
# 可用 PROXY= 覆盖，PROXY="" 禁用（仅当本机可直连外网时）。
PROXY="${PROXY:-http://127.0.0.1:7890}"
if [ -n "$PROXY" ]; then
  export HTTP_PROXY="$PROXY" HTTPS_PROXY="$PROXY" ALL_PROXY="$PROXY"
  export http_proxy="$PROXY" https_proxy="$PROXY" all_proxy="$PROXY"
  info "使用代理: $PROXY"
fi

# cmake>=4.0 移除了对 <3.5 的兼容；老 vcpkg 端口(如 libjpeg-turbo 2.1.5.1)
# 的 cmake_minimum_required 过旧，需此策略版本兜底。
export CMAKE_POLICY_VERSION_MINIMUM=3.5

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    err "缺少命令: $1，请先安装或加入 PATH"
  fi
}

# ---------- Step 0: 前置检查 ----------
info "Step 0: 检查前置环境 (Xcode / Git / Homebrew)"
xcode-select -p >/dev/null 2>&1 || err "未安装 Xcode Command Line Tools，请先运行: xcode-select --install"
need_cmd git
need_cmd brew
info "前置环境 OK"

# ---------- Step 1: Homebrew 工具链 ----------
info "Step 1: 通过 Homebrew 安装工具链"
# 注意文档中 wget 出现了两次，这里只需一次
brew list python3     >/dev/null 2>&1 || brew install python3
brew list create-dmg  >/dev/null 2>&1 || brew install create-dmg
brew list nasm        >/dev/null 2>&1 || brew install nasm
brew list cmake       >/dev/null 2>&1 || brew install cmake
brew list gcc         >/dev/null 2>&1 || brew install gcc
brew list wget        >/dev/null 2>&1 || brew install wget
brew list ninja       >/dev/null 2>&1 || brew install ninja
brew list pkg-config  >/dev/null 2>&1 || brew install pkg-config
# 已有独立 rustup(如 ~/.cargo/bin) 则跳过 brew 版，避免拉入 llvm@22 等重依赖
if ! command -v rustup >/dev/null 2>&1; then
  brew list rustup >/dev/null 2>&1 || brew install rustup
fi

# 修复目标文件夹不存在导致的安装失败
if [ ! -d /usr/local/include ]; then
  warn "/usr/local/include 不存在，尝试创建并授权"
  sudo mkdir -p /usr/local/include
  sudo chown "$(whoami)":admin /usr/local/include
  sudo chmod 775 /usr/local/include
fi
info "Homebrew 工具链版本:"
cmake --version | head -1
nasm -v | head -1
go version 2>/dev/null || true

# ---------- Step 2: vcpkg ----------
info "Step 2: 安装 vcpkg ($VCPKG_TAG)"
if [ ! -d "$VCPKG_ROOT" ]; then
  mkdir -p "$(dirname "$VCPKG_ROOT")"
  git clone https://github.com/microsoft/vcpkg "$VCPKG_ROOT"
fi
cd "$VCPKG_ROOT"
if [ "$(git rev-parse --abbrev-ref HEAD)" = "$VCPKG_TAG" ]; then
  info "vcpkg 已在目标 tag"
else
  git checkout "$VCPKG_TAG"
fi
if [ ! -x "$VCPKG_ROOT/vcpkg" ]; then
  ./bootstrap-vcpkg.sh -disableMetrics
fi
./vcpkg install libvpx libyuv opus aom
export VCPKG_ROOT
info "VCPKG_ROOT=$VCPKG_ROOT"

# ---------- Step 3: Rust (rustup) ----------
info "Step 3: 配置 Rust (rustup)"
if ! command -v rustup >/dev/null 2>&1; then
  info "未找到 rustup，正在初始化 (rustup-init -y)"
  # brew 安装的 rustup 需在初始化前确保 PATH
  rustup-init -y
  export PATH="$HOME/.cargo/bin:$PATH"
fi
command -v rustup >/dev/null 2>&1 || export PATH="$HOME/.cargo/bin:$PATH"
rustup default "$RUST_VERSION"
rustup component add rustfmt
info "Rust 工具链:"
rustup show

# ---------- Step 4: RustDesk 源码 + Python 依赖 ----------
info "Step 4: 拉取 RustDesk 源码 (含子模块)"
if [ ! -d "$RUSTDESK_DIR" ]; then
  mkdir -p "$(dirname "$RUSTDESK_DIR")"
  git clone --recurse-submodules https://github.com/rustdesk/rustdesk "$RUSTDESK_DIR"
else
  info "源码已存在，更新子模块"
  git -C "$RUSTDESK_DIR" submodule update --init --recursive
fi

info "Step 4.1: 安装 Python 依赖"
cd "$RUSTDESK_DIR/libs/portable/"
python3 -m pip install --upgrade pip
pip3 install -r requirements.txt

# ---------- Step 5: UI 组件 ----------
if [ "$WITH_SCITER" = "1" ]; then
  info "Step 5: 下载 Sciter 组件"
  wget -O "$RUSTDESK_DIR/libsciter.dylib" \
    https://github.com/c-smile/sciter-sdk/raw/master/bin.osx/libsciter.dylib
fi

if [ "$WITH_FLUTTER" = "1" ]; then
  info "Step 5: 安装 Flutter 工具链"
  brew tap leoafarias/fvm
  brew list fvm       >/dev/null 2>&1 || brew install fvm
  brew list cocoapods >/dev/null 2>&1 || brew install cocoapods
  fvm global "$FLUTTER_VERSION"
  export PATH="$HOME/fvm/default/bin:$PATH"
  flutter --disable-analytics
  dart --disable-analytics
  flutter doctor -v || warn "flutter doctor 有非关键项失败，请确认 Xcode 项正常"
  info "安装 flutter_rust_bridge_codegen ($FRB_VERSION)"
  cargo install flutter_rust_bridge_codegen --version "$FRB_VERSION" --features "uuid"
fi

# ---------- 收尾：环境变量持久化 ----------
ENV_LINES=$(cat <<EOF

# ---- RustDesk 构建环境 (setup_macos.sh 生成) ----
export VCPKG_ROOT="$VCPKG_ROOT"
export PATH="\$HOME/.cargo/bin:\$HOME/fvm/default/bin:\$HOME/Library/Python/3.9/bin:\$PATH"
EOF
)
if ! grep -q "RustDesk 构建环境" "$HOME/.bash_profile" 2>/dev/null; then
  printf '%s\n' "$ENV_LINES" >> "$HOME/.bash_profile"
  info "已把 export 追加到 ~/.bash_profile (新终端自动生效)"
fi
export PATH="$HOME/.cargo/bin:$HOME/fvm/default/bin:$HOME/Library/Python/3.9/bin:$PATH"

# ---------- 构建 ----------
if [ "$DO_BUILD" = "1" ]; then
  info "Step 6: 开始构建 (cwd=$RUSTDESK_DIR)"
  cd "$RUSTDESK_DIR"
  if [ "$WITH_FLUTTER" = "1" ]; then
    flutter_rust_bridge_codegen \
      --rust-input ./src/flutter_ffi.rs \
      --dart-output ./flutter/lib/generated_bridge.dart \
      --c-output ./flutter/macos/Runner/bridge_generated.h
    python3 ./build.py --flutter
  else
    python3 ./build.py
  fi
  info "构建完成，产物在 $RUSTDESK_DIR (dmg)"
else
  info "依赖就绪。构建命令:"
  echo "  cd $RUSTDESK_DIR && python3 ./build.py"
  [ "$WITH_FLUTTER" = "1" ] && echo "  (Flutter 版: 先跑 flutter_rust_bridge_codegen 再 build.py --flutter)"
fi

info "全部完成 ✔"
