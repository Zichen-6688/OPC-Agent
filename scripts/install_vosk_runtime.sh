#!/usr/bin/env bash
# 安装 Vosk 语音识别运行时(供 Linux 使用 vosk-stt 特性时调用)。
# 原理:从 PyPI 的 vosk 轮子中提取原生库(libvosk.so / libvosk.dylib),安装到系统库目录。
# macOS 不需要运行本脚本(系统自带语音识别)。
#
# 用法:
#   sudo bash scripts/install_vosk_runtime.sh
#
# 安装完成后构建支持语音输入的版本:
#   cd src-tauri && cargo tauri build --features vosk-stt
set -euo pipefail

PYTHON="${PYTHON:-python3}"
PREFIX="${PREFIX:-/usr/local}"
OS="$(uname -s)"

echo "==> 从 PyPI 下载 vosk 轮子(内含原生识别库)..."
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

case "$OS" in
  Linux)
    "$PYTHON" -m pip download vosk --no-deps --only-binary=:all: -d "$TMP" >/dev/null 2>&1
    WHEEL="$(ls "$TMP"/vosk-*.whl 2>/dev/null | head -1)"
    [ -n "$WHEEL" ] || { echo "!! 未找到 Linux 预编译轮子,请确认 CPU 架构受支持(x86_64 / aarch64)"; exit 1; }
    unzip -o -q "$WHEEL" -d "$TMP/wheel"
    LIB="$(find "$TMP/wheel" -name 'libvosk.so*' | head -1)"
    echo "==> 安装 $LIB -> $PREFIX/lib/"
    install -m 755 "$LIB" "$PREFIX/lib/"
    ;;
  Darwin)
    echo "==> macOS 无需手动安装 Vosk(macOS 使用系统原生语音识别)"
    echo "    如需在 macOS 强制使用 Vosk,请自行从 pip 轮子提取 libvosk.dylib 后放入 /usr/local/lib"
    exit 0
    ;;
  *)
    echo "!! 不支持的系统: $OS"; exit 1;;
esac

echo ""
echo "✔ Vosk 运行时已安装。现在可以构建语音输入版本:"
echo "  cd src-tauri && cargo tauri build --features vosk-stt"
echo ""
echo "提示:若使用 AppImage/.deb 分发,请确保目标机器同样安装了 libvosk"
echo "(将 $PREFIX/lib/libvosk.so 拷贝到目标机器 /usr/local/lib/ 即可)。"
