# Hello World 示例

最简 Aura 语言项目，演示 Hello World。

## 目录结构

```text
hello/
├── aura.toml             # 项目配置
├── src/
│   └── main.aura         # 入口文件
└── test/                 # 测试目录（如有）
```

## 使用

```bash
cd hello
loom build        # 完整构建
loom test         # 编译 + 运行测试
loom run          # 编译 + 运行
```

## 源码

```aura
fun main() {
    println("Hello, World!");
}
```

## 验证结果

```text
$ loom build --dir .
项目: hello v0.1.0

═══ 构建结果 ═══
  ✓ clean — ✓ 无需清理
  ✓ resolve — ✓ 依赖解析完成
  ✓ compile-main — ✓ up-to-date (fingerprint: 867200be...)
═══ 2 执行, 1 跳过 (1 缓存命中) ═══
```
