# 编译外部插件

用法：
  ./build-plugin.sh

输出：
  Windows: plugins/target/release/custom_plugin.dll
  Linux:   plugins/target/release/libcustom_plugin.so
  macOS:   plugins/target/release/libcustom_plugin.dylib

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PLUGIN_DIR="$SCRIPT_DIR/plugins"

echo "=== 编译外部插件 ==="
echo "目录: $PLUGIN_DIR"
echo ""

cd "$PLUGIN_DIR"
cargo build --release

echo ""
echo "=== 编译成功 ==="

# 显示输出文件
RELEASE_DIR="$PLUGIN_DIR/target/release"
ls -la "$RELEASE_DIR"/*.dll "$RELEASE_DIR"/*.so "$RELEASE_DIR"/*.dylib 2>/dev/null || true

echo ""
echo "现在可以运行:"
echo "  loom build --dir .."
