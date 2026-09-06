# 最小应用示例

最简 aura-loom 项目，演示基本构建流程。

## 目录结构

```text
basic-app/
├── aura.toml             # 项目配置
├── src/
│   └── main.aura         # 入口文件
└── test/
    └── main_test.aura    # 测试文件
```

## 使用

```bash
cd basic-app
loom build        # 完整构建（clean → resolve → compile → test）
loom test         # 编译 + 运行测试
loom run          # 编译 + 运行
```

## 验证结果

```text
$ loom build --dir .
项目: basic-app v0.1.0
插件: 2 个 (aura-stdlib, aura-test-harness)

═══ 构建结果 ═══
  ✓ clean — ✓ 无需清理
  ✓ resolve — ✓ 依赖解析完成（0 个依赖）
  ✓ compile-main — ✓ 编译 main 源码集: 1 个文件
  ✓ compile-test — ✓ 编译 test 源码集: 1 个文件
  ✓ run-tests — ✓ 测试执行: 1 个测试文件
═══ 5 执行, 0 跳过, 总计 1ms ═══
```
