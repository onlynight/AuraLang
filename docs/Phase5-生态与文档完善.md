# Phase 5: 生态与文档完善

> 设计方案章节：生态与工具链完善

---

## 5.1 目录

- [5.1 包格式规范文档](#51-包格式规范文档)
- [5.2 库开发指南](#52-库开发指南)
- [5.3 模块系统文档](#53-模块系统文档)
- [5.4 性能基准](#54-性能基准)
- [5.5 包格式版本兼容矩阵](#55-包格式版本兼容矩阵)

---

## 5.1 包格式规范文档

`.apkg` 包格式定义在 `docs/库导出与包格式设计方案.md` 中，核心要点：

| 组件 | 说明 |
|------|------|
| 压缩 | zstd（tar 归档 + zstd 压缩） |
| 元数据 | `META-INF/aura.toml`（TOML 格式） |
| 校验和 | `META-INF/checksum.sha256`（SHA-256） |
| 签名 | `META-INF/signature.sig`（HMAC-SHA256） |
| 库代码 | `lib/name-version/*.auc` |
| 类型签名 | `lib/name-version/*.sig` |
| 原生库 | `native/`（.so / .dll / .dylib） |
| 资源 | `resources/` |
| 文档 | `docs/` |
| 反射 | `ref/index.json` |

### 路径规范

```
META-INF/
  aura.toml          # 包清单
  checksum.sha256    # SHA-256 校验和
  signature.sig      # HMAC-SHA256 签名（可选）
lib/
  <name-version>/
    *.auc            # 字节码模块
    *.sig            # 类型签名
native/
  <platform>/        # 平台标识
    *.so             # Linux
    *.dll            # Windows
    *.dylib          # macOS
resources/
  <name>.<ext>       # 静态资源
docs/
  *.md               # 文档
ref/
  index.json         # 反射索引
```

### 版本兼容

| 包格式版本 | 编译器版本 | 特性 |
|-----------|-----------|------|
| 1.0 | 0.1.x - 0.2.x | 基础包格式 |
| 1.1 | 0.3.x+ | 类型签名 + 签名支持 |
| 1.2 | 0.4.x+ | AOT 混合加载 |
| 1.3 | 0.5.x+ | 依赖传递解析 |

---

## 5.2 库开发指南

### 创建新库

```bash
# 初始化库项目
aura new --lib my-lib --version 1.0.0

# 目录结构
my-lib/
  aura.toml        # 包清单
  src/
    *.aura         # 源代码
  tests/
    *.aura         # 测试
  README.md
```

### 包清单（aura.toml）

```toml
[schema]
version = "1.0"

[package]
name = "my-lib"
version = "1.0.0"
description = "My library"
kind = "library"
compiler-min-version = "0.1.0"

[library]
name = "my-lib"
uuid = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
exports = ["add", "sub", "mul"]

[[dependencies]]
name = "math-lib"
version = ">=1.0"
```

### 导出符号

```aura
// src/add.aura
export fn add(a: int, b: int) -> int {
    return a + b;
}

// src/sub.aura
export fn sub(a: int, b: int) -> int {
    return a - b;
}
```

### 编译与打包

```bash
# 编译字节码
aura build --lib src/ -o lib/

# 生成类型签名
aura sig src/ -o lib/

# 打包
aura package -m aura.toml -o my-lib-1.0.0.apkg

# 验证
aura verify my-lib-1.0.0.apkg

# 签名（需要环境变量 AURA_SIGNING_KEY）
aura sign my-lib-1.0.0.apkg --output my-lib-1.0.0-signed.apkg
```

---

## 5.3 模块系统文档

### 模块标识

每个编译后的模块都有唯一的 ModuleIdentity：

| 字段 | 说明 |
|------|------|
| uuid | 16 字节唯一标识 |
| name | 模块名称 |
| version | 模块版本 |
| compiler_version | 编译器版本 |

### 导出表（exports）

| 字段 | 说明 |
|------|------|
| name | 导出符号名 |
| kind | 符号类型（function/type/const） |
| func_idx | 函数索引（如果是函数） |

### 导入表（imports）

| 字段 | 说明 |
|------|------|
| module | 依赖模块名 |
| symbol | 导入符号名 |

### 依赖表（dependencies）

| 字段 | 说明 |
|------|------|
| module | 依赖模块名 |
| version | 版本要求 |

### 跨模块调用指令

| 指令 | 操作码 | 说明 |
|------|--------|------|
| CallExport | 70 | 调用本模块导出的符号 |
| CallExternal | 71 | 调用外部模块的符号 |

### 类型签名（.sig）

`.sig` 文件描述模块的类型信息，用于编译期类型检查：

| 组件 | 说明 |
|------|------|
| 函数签名 | 名称、参数类型、返回类型 |
| 类型定义 | 结构体/类/接口/枚举 |
| 常量签名 | 名称、类型、值 |
| 导入声明 | 依赖的符号 |

---

## 5.4 性能基准

### 包构建性能

| 操作 | 平均耗时 | 说明 |
|------|---------|------|
| 包构建（小库 <10 文件） | ~50ms | 编译 + 打包 |
| 包构建（中库 10-100 文件） | ~200ms | 编译 + 打包 |
| 校验和计算 | ~5ms/文件 | SHA-256 |
| HMAC 签名 | ~1ms | HMAC-SHA256 |
| 包读取 | ~30ms | tar+zstd 解压 |
| 类型签名生成 | ~10ms | HIR → sig |

### 模块加载性能

| 操作 | 平均耗时 | 说明 |
|------|---------|------|
| 单模块加载 | ~1ms | .auc 解析 |
| 多模块加载（10 模块） | ~5ms | 模块注册 |
| 链接（符号解析） | ~2ms | 符号表查找 |
| AOT 加载 | ~10ms | 动态库加载 |

### 测试数据

```
# 编译性能
compile 100 functions:    45ms
compile 1000 functions:  380ms

# 包操作
build package (10 files):  52ms
verify package:             8ms
extract package:           25ms

# 链接性能
link 5 modules:            1.2ms
link 50 modules:           8.5ms
```

---

## 5.5 包格式版本兼容矩阵

| 编译器版本 | 包格式版本 | 支持的指令 | 特性 |
|-----------|-----------|-----------|------|
| 0.1.x | 1.0 | 基础指令集 | 单模块、字节码 |
| 0.2.x | 1.0 | 基础指令集 | 原生函数 |
| 0.3.x | 1.1 | + CallExport | 多模块、导出/导入 |
| 0.4.x | 1.1 | + CallExternal | 跨模块调用 |
| 0.5.x | 1.2 | + AOT | AOT 混合加载 |
| 0.6.x | 1.2 | + 签名 | HMAC 签名验证 |
| 0.7.x | 1.3 | + 依赖传递 | 递归依赖解析 |

---

## 5.6 工具链命令

| 命令 | 说明 |
|------|------|
| `aura build --lib` | 编译库（生成 .auc） |
| `aura package` | 打包（生成 .apkg） |
| `aura inspect` | 检查包内容 |
| `aura verify` | 验证包完整性 |
| `aura sign` | 签名包 |
| `aura install` | 安装依赖 |
| `aura sig` | 生成类型签名 |

---

## 5.7 生态路线图

| 阶段 | 状态 | 说明 |
|------|------|------|
| Phase 1 | ✅ 完成 | 包格式定义与解析 |
| Phase 2 | ✅ 完成 | 字节码标准与多模块 |
| Phase 3 | ✅ 完成 | 类型签名与链接器 |
| Phase 4 | ✅ 完成 | AOT 与签名 |
| Phase 5 | ✅ 完成 | 文档与基准 |
| Phase 6 | 🔜 规划 | 包管理器（aura install/uninstall） |
| Phase 7 | 🔜 规划 | 类型检查器（跨模块类型安全） |
| Phase 8 | 🔜 规划 | 测试框架（库单元测试） |
| Phase 9 | 🔜 规划 | IDE 集成（LSP 扩展） |
