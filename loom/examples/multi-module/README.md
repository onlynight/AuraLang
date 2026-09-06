# 多模块示例

演示多文件编译 + 模块树 + import 解析。

## 目录结构

```text
multi-module/
├── aura.toml
├── src/
│   ├── main.aura         # 入口（import utils.* + math.*）
│   ├── utils.aura        # 字符串工具函数
│   └── math/
│       ├── mod.aura      # 模块入口（re-export）
│       ├── vector.aura   # 2D 向量运算
│       └── matrix.aura   # 2x2 矩阵运算
└── test/
    └── utils_test.aura   # 工具函数测试
```

## 模块依赖图

```text
main.aura
  ├── import utils.*     → utils.aura
  └── import math.*      → math/mod.aura
                              ├── import math.vector.*  → math/vector.aura
                              └── import math.matrix.*  → math/matrix.aura
```

## 使用

```bash
cd multi-module
loom build        # 编译 5 个源文件（main + utils + math/mod + math/vector + math/matrix）
loom test         # 编译 + 运行 1 个测试文件
```

## 验证结果

```text
$ loom build --dir .
项目: multi-module v0.1.0
插件: 2 个 (aura-stdlib, aura-test-harness)

═══ 构建结果 ═══
  ✓ clean — ✓ 无需清理
  ✓ resolve — ✓ 依赖解析完成（0 个依赖）
  ✓ compile-main — ✓ 编译 main 源码集: 5 个文件
  ✓ compile-test — ✓ 编译 test 源码集: 1 个文件
  ✓ run-tests — ✓ 测试执行: 1 个测试文件
═══ 5 执行, 0 跳过, 总计 2ms ═══
```
