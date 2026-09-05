# Aura 构建系统设计

> **前置阅读**：
> - `docs/多文件编译打包方案设计.md` — 多文件源码编译机制（本文档的前置环节）
> - `docs/库导出与包格式设计方案.md` — `.apkg` 制品格式与分发（本文档的后置环节）
>
> 本文档设计一套**完整的构建系统**，对标 Java 的 Maven/Gradle，涵盖从源码到制品的全生命周期。

---

## 1. 现状分析

### 1.1 已实现的基础设施

| 模块 | 文件 | 状态 | 能力 |
|------|------|------|------|
| 包清单 | `package.rs::PackageManifest` | ✅ 已实现 | name, version, entry, dependencies, dev-dependencies, library, kind |
| 版本管理 | `package.rs::Version` / `VersionConstraint` | ✅ 已实现 | SemVer 2.0.0, 8 种约束运算符 |
| 依赖声明 | `package.rs::Dependency` | ✅ 已实现 | name + version + source(Git/Path/Binary) |
| 依赖注释 | `package.rs::parse_depends()` | ✅ 已实现 | `// @depends name >= 1.0` 解析 |
| 锁文件 | `package.rs::LockFile` | ✅ 已实现 | `aura.lock`，记录解析后的精确版本 + rev |
| 缓存管理 | `package.rs::PackageCache` | ✅ 已实现 | `~/.aura/cache/packages/`，Git clone 缓存 |
| 依赖图 | `package.rs::DependencyGraph` | ✅ 已实现 | 节点 + 版本冲突检测 + 树状显示 |
| 包管理器 | `package.rs::PackageManager` | ✅ 已实现 | install, update, publish, show_deps, verify_offline |
| 源码创建 | `package.rs::create_new_package()` | ✅ 已实现 | `aura new` 创建模板项目 |
| 编译入口 | `codegen/mod.rs::compile_source()` | ✅ 已实现 | 单文件编译：`&str` → `BytecodeModule` |
| 字节码序列化 | `codegen/serialize.rs` | ✅ 已实现 | `.auc` 读写，单模块格式 |
| VM | `vm/mod.rs::Vm` | ✅ 已实现 | 字节码解释器 + JIT 编译 |
| 包格式骨架 | `apkg/mod.rs` | ⚠️ 部分实现 | 常量/错误定义，builder.rs/reader.rs 缺失 |
| CLI | `cli/src/main.rs` | ✅ 已实现 | build, run, check, install, update, publish, deps, new |

### 1.2 已实现的 CLI 命令

```text
aura build <file.aura> [--output <out>]        编译为字节码 / 原生可执行文件
aura build <file.aura> --aot [--output <exe>]  AOT 编译
aura run <file.aura>                           编译并执行
aura check <file.aura>                         语法/语义检查
aura install [--offline]                       安装依赖
aura update [--all]                            更新依赖
aura publish [--dir <path>]                    发布包
aura deps [--dir <path>] [--outdated]          显示依赖树
aura new <name> [--dir <path>]                 创建新包项目
```

### 1.3 关键缺口（对标 Maven/Gradle）

| 维度 | Maven/Gradle 能力 | **Aura（当前）** |
|------|-------------------|:----------------:|
| 构建生命周期 | clean → compile → test → package → install → deploy | **❌ 无**（仅 build/run 两个原子操作） |
| 任务系统 | 声明式任务 DAG，可组合、可并行 | **❌ 无** |
| 源码集 | sourceSets: main / test / bench / resources | **❌ 无** |
| 依赖配置 | implementation / compileOnly / runtimeOnly / testImplementation | **⚠️ 仅 dependencies + dev-dependencies** |
| 插件系统 | 声明式插件 + 约定插件 | **❌ 无** |
| 构建配置 | profiles / properties / conventions | **❌ 无**（仅扁平字段） |
| 构建缓存 | 本地缓存 + 远程缓存（Build Cache） | **❌ 无**（仅 Git clone 缓存） |
| 构建包装器 | `gradlew` / `mvnw` 可重复构建 | **❌ 无** |
| 多项目 | monorepo / composite builds | **❌ 无** |
| 仓库管理 | Maven Central / JCenter / 私有仓库 | **⚠️ 仅 Git 仓库** |
| 依赖管理 | BOM / 版本对齐 / 冲突解析策略 | **⚠️ 仅冲突检测，无解析策略** |
| IDE 集成 | project model export | **❌ 无**（仅 LSP 语法分析） |
| CI/CD | 构建脚本 + 发布流水线 | **❌ 无** |
| Watch 模式 | Gradle `--continuous` / Maven `exec:watch` | **❌ 无** |
| 并行构建 | Gradle `--parallel` | **❌ 无** |
| 输入/输出追踪 | Gradle task up-to-date checks | **❌ 无** |

---

## 2. 对标设计：构建系统能力模型

### 2.1 Maven 架构

```text
pom.xml（单一配置源）
  │
  ├── <groupId> / <artifactId> / <version>     坐标
  ├── <modules>                                多模块聚合
  ├── <dependencies> / <dependencyManagement>   依赖 + 版本管理
  ├── <build>                                  构建配置
  │   ├── <sourceDirectory>                    源码目录
  │   ├── <resources>                          资源目录
  │   └── <plugins>                            插件配置
  ├── <profiles>                               构建配置切换
  ├── <repositories>                           仓库声明
  └── <distributionManagement>                 发布仓库
  │
  └── 生命周期：clean → validate → compile → test → package → verify → install → deploy
```

**核心思想**：
- **生命周期阶段**：每个阶段绑定一组插件目标（goals）
- **插件目标**：最小执行单元（如 `compiler:compile`、`surefire:test`、`jar:jar`）
- **配置优先**：`pom.xml` 是唯一配置源，插件通过配置自定义行为
- **传递依赖**：Maven 自动解析传递依赖，`dependencyManagement` 统一版本

### 2.2 Gradle 架构

```text
build.gradle / build.gradle.kts（构建脚本）
  │
  ├── apply plugin: 'java'                      约定插件
  ├── sourceSets { main { java { srcDir } } }   源码集
  ├── dependencies {
  │     implementation '...'                    依赖配置
  │     testImplementation '...'
  │     compileOnly '...'
  │     runtimeOnly '...'
  │ }
  ├── tasks {
  │     customTask {                            自定义任务
  │       dependsOn 'compile'
  │       doLast { ... }
  │     }
  │ }
  ├── plugins {                                 插件声明
  │     id '...' version '...'
  │ }
  │
  └── 任务图（Task DAG）：并行执行 + 缓存 + 增量
```

**核心思想**：
- **任务即最小单元**：每个 `task` 有 name + actions + dependencies + inputs/outputs
- **任务图**：声明式依赖关系，引擎自动计算并行执行
- **约定插件**：`apply plugin: 'java'` 自动创建 sourceSets + 标准任务
- **Build Cache**：任务输出缓存，跨项目复用
- **Configuration Cache**：构建配置缓存，加速重复构建

### 2.3 Bazel（Buck）架构

```text
BUILD / BUILD.bazel（每个目录一个构建文件）
  │
  ├── java_library(name = "foo", srcs = [...], deps = [...])
  ├── java_binary(name = "main", srcs = [...], deps = [...])
  ├── java_test(name = "foo_test", srcs = [...], deps = [...])
  │
  └── 远程缓存 + 远程执行 + 沙箱隔离
```

**核心思想**：
- **目录级构建文件**：每个目录的 `BUILD` 文件声明该目录的构建规则
- **规则即函数**：`java_library(...)` 是 Starlark 函数，返回规则实例
- **严格增量**：action 级别的输入/输出追踪
- **远程执行**：分布式编译 + 远程缓存

### 2.4 Cargo 架构

```text
Cargo.toml（单一配置源）
  │
  ├── [package] name / version / authors / license
  ├── [lib] path = "src/lib.rs"
  ├── [[bin]] name = "..." path = "src/..."
  ├── [dependencies] / [dev-dependencies] / [build-dependencies]
  ├── [features] feature1 = ["dep1", "dep2"]
  ├── [workspace] members = [...]
  │
  └── cargo build / test / run / publish / doc / fmt / clippy
```

**核心思想**：
- **约定目录结构**：`src/lib.rs`、`src/main.rs`、`src/bin/*.rs`、`tests/*.rs`、`benches/*.rs`
- **单一配置文件**：`Cargo.toml` 声明所有信息
- **cargo 命令 = 任务**：`cargo build`、`cargo test` 是内置任务
- **workspace**：多项目聚合，共享依赖

### 2.5 能力对比

| 能力 | Maven | Gradle | Bazel | Cargo | **Aura（目标）** |
|------|:-----:|:------:|:-----:|:-----:|:----------------:|
| 单一配置文件 | pom.xml | build.gradle | BUILD | Cargo.toml | **aura.toml** |
| 构建生命周期 | ✅ 8 阶段 | ✅ 任务 DAG | ✅ 规则 | ✅ 命令 | **✅ 任务 DAG** |
| 任务系统 | ✅ goals | ✅ tasks | ✅ rules | ✅ commands | **✅ tasks** |
| 源码集 | ✅ | ✅ sourceSets | ✅ srcs | ✅ 约定 | **✅ source-sets** |
| 依赖配置 | ✅ | ✅ configurations | ✅ deps | ✅ [dependencies] | **✅ configurations** |
| 插件系统 | ✅ plugins | ✅ plugins | ✅ rules | ⚠️ proc macros | **✅ plugins** |
| 构建缓存 | ⚠️ | ✅ Build Cache | ✅ 远程缓存 | ✅ fingerprint | **✅ 两级缓存** |
| 构建包装器 | ✅ mvnw | ✅ gradlew | ✅ bazelisk | ❌ | **✅ aura wrapper** |
| 多项目 | ✅ modules | ✅ settings | ✅ packages | ✅ workspace | **✅ workspace** |
| 仓库管理 | ✅ repositories | ✅ repositories | ✅ registry | ✅ registry | **✅ registries** |
| 依赖管理 | ✅ BOM | ✅ platform | ✅ versions | ✅ workspace | **✅ BOM + 版本锁定** |
| IDE 集成 | ✅ | ✅ | ✅ Bazel IDE | ✅ rust-analyzer | **✅ IDE model** |
| CI/CD | ✅ | ✅ | ✅ | ✅ | **✅ aura ci** |
| Watch 模式 | ⚠️ exec:watch | ✅ --continuous | ⚠️ | ❌ | **✅ --watch** |
| 并行构建 | ❌ | ✅ --parallel | ✅ | ⚠️ | **✅ --parallel** |
| 输入/输出追踪 | ❌ | ✅ | ✅ | ✅ | **✅ fingerprint** |

### 2.6 配置方案决策：纯 TOML vs 代码配置

#### 候选方案

| 方案 | 对标 | 配置文件 | 性质 |
|------|------|----------|------|
| **A. 纯 TOML** | Cargo.toml / pyproject.toml | `aura.toml` | 声明式数据 |
| **B. Aura 源码** | build.gradle.kts / BUILD.bazel | `build.aura` | 命令式程序 |
| **C. 混合（TOML + 可选 build.aura）** | Cargo.toml + build.rs | 两者 | 分层 |

#### 方案对比

| 维度 | **A. 纯 TOML** | **B. Aura 源码** | **C. 混合** |
|------|:--------------:|:----------------:|:-----------:|
| 引导循环 | ✅ 无 | ❌ 有（需编译器解析 build.aura） | ⚠️ 可控（build.aura 可选） |
| 安全性 | ✅ 高（纯数据，无代码执行） | ❌ 低（可执行任意代码） | ⚠️ TOML 安全，build.aura 可选沙箱 |
| 解析速度 | ✅ 极快（~0.1ms） | ❌ 慢（~50-200ms） | ✅ TOML 快 |
| 可静态验证 | ✅ 可做 schema 校验 | ❌ 不可（需执行） | ⚠️ TOML 可验证 |
| 可重现性 | ✅ 高（确定性数据） | ❌ 低（代码可依赖环境） | ⚠️ TOML 可重现 |
| Diff 友好 | ✅ 是（纯文本，无缩进敏感） | ❌ 否（缩进敏感，格式噪音） | ✅ TOML 部分 |
| 表达能力 | ❌ 弱（无条件/循环/函数） | ✅ 强（完整语言能力） | ✅ 按需 |
| 代码生成 | ❌ 否 | ✅ 是 | ✅ 可选 |
| 插件开发 | ⚠️ 需 ABI 接口 | ✅ 同语言 | ⚠️ 分层 |
| IDE 支持 | ✅ 简单（JSON Schema） | ✅ 完整（LSP 全功能） | ✅ 分层 |
| 学习曲线 | ✅ 极低（对标 Cargo.toml） | ⚠️ 中等（新 DSL） | ✅ 渐进 |
| 构建速度 | ✅ 快 | ❌ 慢 | ✅ 多数情况快 |
| 版本耦合 | ✅ 无 | ❌ 有（配置版本 vs 编译器版本） | ⚠️ build.aura 可选 |

#### 决策：选择方案 A（纯 TOML）

**理由**：

1. **零引导循环**：TOML 解析不需要编译器参与，编译器构建自身时也不会遇到"先有鸡还是先有蛋"的问题
2. **安全边界清晰**：TOML 是纯数据，无代码执行风险，无需沙箱机制
3. **解析性能优秀**：构建配置解析耗时 < 1ms，不影响构建速度
4. **可静态验证**：CI 可在编译前检查 `aura.toml` 合法性（`aura check-config`）
5. **可重现性高**：相同 TOML 输入 → 相同构建输出，不同环境构建结果一致
6. **对标成熟系统**：Cargo.toml / pyproject.toml / bunfig.toml 均证明纯 TOML 足以支撑完整的构建系统
7. **表达式缺口可用设计弥补**：
   - 条件逻辑 → `[profiles]` 表（类 Maven profiles）
   - 循环/迭代 → `[source-sets]` + `[[tasks]]` 数组（TOML 原生支持）
   - 数据变换 → 插件 `.so` 在运行时处理（非配置层）
   - 代码生成 → `[[tasks]]` 调用外部脚本
8. **避免过度设计**：90% 的项目用 TOML 足够；不引入 10% 场景的代码能力，保持系统简洁

**不选择的方案**：

| 方案 | 不选理由 |
|------|----------|
| B. 纯 Aura 源码 | 引导循环 + 安全边界崩塌 + 解析慢 + 可重现性差，代价大于收益 |
| C. 混合（TOML + build.aura） | 引入两套配置路径增加复杂度；引导循环仍然存在（当 build.aura 存在时）；安全边界不清晰 |

**替代方案（弥补 TOML 表达能力不足）**：

| TOML 缺口 | 替代方案 | 说明 |
|-----------|----------|------|
| 条件逻辑 | `[profiles]` 表 | 类 Maven profiles，通过 `--profile` 切换 |
| 循环/迭代 | TOML 数组 + `[[tasks]]` | 如 `[[tasks.deploy]]` 可声明多个部署目标 |
| 数据变换 | 插件 `.so` | 插件在运行时读取 TOML 并变换数据 |
| 代码生成 | `[[tasks]]` + 外部命令 | `command = "aura codegen --input ..."` |
| 复用配置 | `[workspace]` 继承 | Workspace 成员继承根配置的默认值 |
| 访问编译器 API | 插件 `.so` ABI | 插件直接链接编译器内部 API |

---

## 3. Aura 构建系统架构

### 3.1 架构总览

```text
┌─────────────────────────────────────────────────────────────────────┐
│                         CLI 层                                      │
│  aura build | test | run | clean | package | publish | ci | watch  │
├─────────────────────────────────────────────────────────────────────┤
│                       任务引擎层                                     │
│  TaskGraph → Scheduler → Executor → CacheLookup → ArtifactManager   │
├─────────────────────────────────────────────────────────────────────┤
│                       构建配置层                                     │
│  Manifest(aura.toml) → SourceSet → Dependencies → Profiles → Plugins│
├─────────────────────────────────────────────────────────────────────┤
│                       插件系统层                                     │
│  ConventionPlugins → BuildPlugins → ExternalPlugins                 │
├─────────────────────────────────────────────────────────────────────┤
│                       依赖管理层                                     │
│  Resolver → Repository → Registry → LockFile → Cache                │
├─────────────────────────────────────────────────────────────────────┤
│                       编译/运行层                                    │
│  Frontend → HIR → MIR → Bytecode → Linker → VM/JIT/AOT              │
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 核心设计原则

1. **单一配置源（纯 TOML）**：`aura.toml` 是唯一配置入口，采用纯 TOML 声明式配置，不引入 `build.aura` / `aurafile` / `BUILD` 等代码配置或额外文件。详见 §2.6 配置方案决策。
2. **约定优于配置**：目录结构提供默认行为（`src/`、`test/`、`bench/`），配置可覆盖
3. **任务即最小单元**：每个可组合的构建步骤是一个任务，形成 DAG
4. **生命周期可裁剪**：标准生命周期（clean → build → test → package）可跳过阶段
5. **增量优先**：fingerprint 驱动的增量构建，支持本地 + 远程缓存
6. **插件可扩展**：约定插件提供默认行为，外部 `.so` 插件可自定义构建逻辑（弥补 TOML 无代码执行的缺口）
7. **安全边界清晰**：TOML 是纯数据，无代码执行风险；插件通过 ABI 接口隔离，非任意代码执行
8. **与 VM 对齐**：构建系统的最终产物是 `.apkg`，直接对接 VM 加载

---

## 4. 构建配置文件（aura.toml）

### 4.1 完整规范

```toml
# ══════════════════════════════════════════════════════════════════════
# 包元数据（已有，保留）
# ══════════════════════════════════════════════════════════════════════

schema-version = "2.0"        # 清单格式版本

name = "my-app"
version = "1.0.0"
description = "示例项目"
authors = ["Alice <alice@example.com>"]
license = "MIT"
repository = "https://github.com/user/my-app"

# 入口（应用必填；库可省略）
entry = "src/main.aura"

# 导出符号列表
exports = ["main"]

# ══════════════════════════════════════════════════════════════════════
# 包类型（已有，保留）
# ══════════════════════════════════════════════════════════════════════

library = false               # true = 库包（无 entry）
kind = "bytecode"             # bytecode | hybrid | native

compiler-min-version = "0.3.0"
compiler-max-version = "1.0"

# ══════════════════════════════════════════════════════════════════════
# 依赖声明（扩展：按配置分组）
# ══════════════════════════════════════════════════════════════════════

# 编译期 + 运行期依赖（实现 + 运行时）
[dependencies]
aura-json = "^1.0"
aura-http = ">=2.0"

# 仅编译期依赖（类似 compileOnly）
[compile-dependencies]
aura-test-macros = "^0.5"

# 仅运行期依赖（类似 runtimeOnly）
[runtime-dependencies]
aura-logger = "^2.0"

# 测试依赖（仅 test 任务使用）
[dev-dependencies]
aura-test = "^1.0"
aura-bench = "^0.3"

# ══════════════════════════════════════════════════════════════════════
# 构建配置（新增 [build] 表）
# ══════════════════════════════════════════════════════════════════════

[build]

# ── 源码集（类 Gradle sourceSets）──
[source-sets.main]
source-dirs = ["src"]
resource-dirs = ["resources"]
include = ["**/*.aura"]
exclude = ["**/*.test.aura", "**/vendor/**"]

[source-sets.test]
source-dirs = ["test"]
resource-dirs = ["test/resources"]
include = ["**/*.aura"]
depends-on = ["main"]        # 测试可访问主源码

[source-sets.bench]
source-dirs = ["bench"]
resource-dirs = ["bench/resources"]
include = ["**/*.aura"]
depends-on = ["main"]

# ── 编译选项 ──
opt-level = 2                # 0/1/2/3
debug = true                 # 调试信息
target = "x86_64-pc-windows-msvc"  # AOT 目标三元组
emit-signatures = true       # 生成类型签名
emit-package = false         # 打包为 .apkg

# ── 输出 ──
out-dir = "target/build"     # 构建产物输出目录
cache-dir = "target/cache"   # 构建缓存目录

# ── 别名映射（类似 tsconfig paths）──
[build.alias]
"@app" = "src"
"@lib" = "lib"

# ══════════════════════════════════════════════════════════════════════
# 插件配置（新增 [plugins] 表）
# ══════════════════════════════════════════════════════════════════════

[plugins]
# 启用内置插件
aura-stdlib = true           # 标准库插件（自动注册 std 模块）
aura-test-harness = true     # 测试框架插件
aura-doc-gen = false         # 文档生成插件

# 外部插件（Phase 3+）
# "aura-format" = { path = "plugins/format.plugin", version = "^0.1" }

# ══════════════════════════════════════════════════════════════════════
# 构建配置切换（新增 [profiles] 表，类 Maven profiles）
# ══════════════════════════════════════════════════════════════════════

[profiles.debug]
activate = "default"         # 默认激活
build.opt-level = 0
build.debug = true

[profiles.release]
build.opt-level = 3
build.debug = false
build.emit-package = true

[profiles.ci]
activate = false
build.opt-level = 2
build.debug = false
build.emit-package = true

# ══════════════════════════════════════════════════════════════════════
# 仓库声明（新增 [repositories] 表，类 Maven repositories）
# ══════════════════════════════════════════════════════════════════════

[repositories]
# 中央仓库（内置）
central = "https://registry.aura-lang.dev"

# 自定义仓库
# "my-company" = "https://registry.example.com"
# "local" = { path = "~/.aura/registry" }

# 发布仓库
[repositories.publish]
registry = "central"
# token = "${AURA_REGISTRY_TOKEN}"

# ══════════════════════════════════════════════════════════════════════
# Workspace（多项目，新增 [workspace] 表，类 Cargo workspace）
# ══════════════════════════════════════════════════════════════════════

[workspace]
# 单项目模式（默认）：无 [workspace] 表
# 多项目模式：
members = [
  "libs/core",
  "libs/utils",
  "apps/server",
  "apps/cli",
]
resolver = "2"              # 依赖解析策略
default-members = ["."]      # 默认构建成员

# ══════════════════════════════════════════════════════════════════════
# 资源配置（已有，保留）
# ══════════════════════════════════════════════════════════════════════

[resources]
include = ["**/*.json", "**/*.html"]
exclude = ["**/*.test.*"]

# ══════════════════════════════════════════════════════════════════════
# 制品选项（已有，保留）
# ══════════════════════════════════════════════════════════════════════

[package]
format = "apkg"
include-sources = false
include-docs = false
include-native = true
native-targets = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]
aot-opt-level = 2
```

### 4.2 配置文件优先级

```text
1. 命令行参数（最高优先级）
   aura build --opt 0 --target arm64

2. 激活的 profile
   aura build --profile release

3. 项目配置（aura.toml）
   [build] opt-level = 2

4. 约定默认值（最低优先级）
   opt-level = 2, debug = true, source-dirs = ["src"]
```

---

## 5. 构建生命周期

### 5.1 标准生命周期

```text
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│  clean   │ → │ resolve  │ → │  compile │ → │   test   │ → │ package  │
└──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘
                                                                │
┌──────────┐   ┌──────────┐                                     │
│  install │ ← │  verify  │ ←──────────────────────────────────┘
└──────────┘   └──────────┘
```

### 5.2 生命周期阶段定义

| 阶段 | 命令 | 任务 | 说明 |
|------|------|------|------|
| **clean** | `aura clean` | `clean` | 清理构建产物（`target/`） |
| **resolve** | `aura resolve` | `resolve-dependencies` | 解析依赖 + 下载 + 锁文件更新 |
| **compile** | `aura compile` | `compile-main`, `compile-test`, `compile-bench` | 编译各源码集 |
| **test** | `aura test` | `run-tests` | 执行测试 |
| **package** | `aura package` | `package-lib`, `package-app` | 打包为 `.apkg` |
| **verify** | `aura verify` | `verify-package` | 验证制品完整性 |
| **install** | `aura install` | `install-local` | 安装到本地注册表 |
| **deploy** | `aura publish` | `publish-remote` | 发布到远程仓库 |
| **run** | `aura run` | `compile-main` → `execute` | 编译 + 运行 |
| **build** | `aura build` | 全生命周期 | 从 clean 到 package |

### 5.3 生命周期执行流程

```text
aura build
  │
  ├─ 1. clean          → 清理 target/
  ├─ 2. resolve        → 解析依赖（Git clone / 下载 .apkg）
  ├─ 3. compile-main   → 编译 src/ 源码集
  ├─ 4. compile-test   → 编译 test/ 源码集（如果存在）
  ├─ 5. compile-bench  → 编译 bench/ 源码集（如果存在）
  ├─ 6. run-tests      → 执行测试
  ├─ 7. package        → 打包为 .apkg
  ├─ 8. verify         → 验证制品
  └─ 9. install        → 安装到本地缓存
```

### 5.4 生命周期裁剪

```bash
# 仅编译，不测试
aura build --skip test

# 仅编译 + 测试
aura build --only compile,test

# 从中间阶段开始（跳过 clean）
aura build --from resolve

# 运行完整 CI 流水线
aura ci --profile ci
```

---

## 6. 任务系统

### 6.1 任务模型

```rust
/// 构建任务定义
struct TaskDefinition {
    /// 任务名称（如 "compile-main"）
    name: String,
    /// 任务描述
    description: String,
    /// 任务类型（决定执行逻辑）
    kind: TaskKind,
    /// 依赖任务（DAG 前置条件）
    depends_on: Vec<String>,
    /// 任务输入（用于增量检查）
    inputs: TaskInputs,
    /// 任务输出（产物）
    outputs: TaskOutputs,
    /// 任务配置（来自 aura.toml）
    config: TaskConfig,
}

enum TaskKind {
    /// 内置任务：编译源码集
    Compile(SourceSetId),
    /// 内置任务：运行测试
    Test,
    /// 内置任务：打包制品
    Package,
    /// 内置任务：清理产物
    Clean,
    /// 内置任务：解析依赖
    Resolve,
    /// 插件任务（由插件注册）
    Plugin(PluginTaskId),
}

struct TaskInputs {
    /// 源文件列表（用于 fingerprint 计算）
    files: Vec<PathBuf>,
    /// 编译选项（opt-level, target, features）
    options: HashMap<String, String>,
    /// 依赖任务的 fingerprint 哈希
    dep_fingerprints: HashMap<String, String>,
}

struct TaskOutputs {
    /// 产物文件列表
    files: Vec<PathBuf>,
    /// 产物目录
    dir: PathBuf,
}
```

### 6.2 任务执行引擎

```text
TaskGraph
  │
  ├─ 1. 构建任务图
  │     └─ 解析 aura.toml → 展开任务 → 建立 depends_on 边
  │
  ├─ 2. 拓扑排序
  │     └─ 检测循环依赖 → 报错
  │     └─ 确定执行顺序
  │
  ├─ 3. 增量检查
  │     └─ 对每个任务：计算当前 fingerprint vs 缓存 fingerprint
  │     └─ 标记需要执行的任务（up-to-date / out-of-date）
  │
  ├─ 4. 并行调度
  │     └─ 无依赖关系且都 out-of-date 的任务 → 并行执行
  │     └─ 有依赖关系的任务 → 顺序执行（前序完成后才能开始）
  │
  ├─ 5. 执行
  │     └─ 按拓扑顺序执行任务
  │     └─ 记录输出产物 + 更新 fingerprint 缓存
  │
  └─ 6. 缓存更新
        └─ 任务输出 → 本地缓存（target/cache/）
        └─ 可选：→ 远程缓存（Build Cache）
```

### 6.3 内置任务清单

| 任务名 | 类型 | 依赖 | 说明 |
|--------|------|------|------|
| `clean` | Clean | - | 清理 `target/` 目录 |
| `resolve` | Resolve | - | 解析依赖 + 下载 + 锁文件 |
| `compile-main` | Compile(main) | `resolve` | 编译主源码集 |
| `compile-test` | Compile(test) | `compile-main` | 编译测试源码集 |
| `compile-bench` | Compile(bench) | `compile-main` | 编译基准源码集 |
| `run-tests` | Test | `compile-test` | 执行测试 |
| `package` | Package | `compile-main` | 打包为 `.apkg` |
| `verify` | Verify | `package` | 验证制品完整性 |
| `install` | Install | `verify` | 安装到本地注册表 |
| `publish` | Deploy | `install` | 发布到远程仓库 |
| `run` | Execute | `compile-main` | 运行应用 |
| `watch` | Watch | `resolve` | 监听源码变化，增量重编 |
| `doc` | Plugin(doc-gen) | `compile-main` | 生成文档 |
| `fmt` | Plugin(fmt) | - | 格式化源码 |
| `check` | Check | - | 仅语法/语义检查 |

### 6.4 任务组合示例

```toml
# 自定义任务（类 Gradle tasks 块）
[[tasks.custom-deploy]]
name = "custom-deploy"
description = "部署到测试环境"
depends-on = ["package", "verify"]
command = "aura publish --target test-env"

[[tasks.release]]
name = "release"
description = "完整发布流程"
depends-on = ["clean", "resolve", "compile-main", "run-tests", "package", "verify", "install", "publish"]
```

---

## 7. 源码集

### 7.1 源码集模型

```rust
struct SourceSet {
    /// 源码集名称（main / test / bench）
    name: String,
    /// 源码目录列表
    source_dirs: Vec<PathBuf>,
    /// 资源目录列表
    resource_dirs: Vec<PathBuf>,
    /// 包含模式
    include: Vec<String>,
    /// 排除模式
    exclude: Vec<String>,
    /// 依赖的其他源码集（如 test 依赖 main）
    depends_on: Vec<String>,
    /// 发现的模块列表（编译时填充）
    modules: Vec<ModuleInfo>,
}
```

### 7.2 默认源码集

| 源码集 | 源码目录 | 资源目录 | 依赖 | 用途 |
|--------|----------|----------|------|------|
| `main` | `src/` | `resources/` | - | 主程序 / 库代码 |
| `test` | `test/` | `test/resources/` | `main` | 测试代码 |
| `bench` | `bench/` | `bench/resources/` | `main` | 性能基准代码 |

### 7.3 源码集与模块树

```text
source-set "main"
  ├── src/main.aura          → module: "main"
  ├── src/utils.aura         → module: "utils"
  └── src/math/
      ├── mod.aura           → module: "math"
      ├── vector.aura        → module: "math.vector"
      └── matrix.aura        → module: "math.matrix"

source-set "test"
  ├── test/utils_test.aura   → module: "test.utils_test"
  └── test/math_test.aura    → module: "test.math_test"
```

### 7.4 源码集依赖解析

```text
test 源码集 import main 模块：
  test/utils_test.aura
    └── import utils          → 解析到 main 源码集的 utils 模块
    └── import math.vector    → 解析到 main 源码集的 math.vector 模块

解析规则：
  1. 先查当前源码集（test）的模块
  2. 再查 depends-on 源码集（main）的模块
  3. 再查标准库（aura.std.*）
  4. 再查依赖包（.apkg）
```

---

## 8. 依赖配置

### 8.1 依赖配置类型

| 配置名 | 用途 | 传递性 | 类 Maven/Gradle 对应 |
|--------|------|--------|---------------------|
| `dependencies` | 编译 + 运行 | ✅ 传递 | `implementation` / `compile` |
| `compile-dependencies` | 仅编译期 | ❌ 不传递 | `compileOnly` |
| `runtime-dependencies` | 仅运行期 | ❌ 不传递 | `runtimeOnly` |
| `dev-dependencies` | 测试 + 基准 | ❌ 不传递 | `testImplementation` / `testCompile` |
| `build-dependencies` | 构建脚本 | ❌ 不传递 | `buildscript` / `[build-dependencies]` |

### 8.2 依赖解析策略

```text
1. 版本求解
   └─ 对每个依赖，解析版本约束 → 确定精确版本
   
2. 冲突解决
   └─ 同一依赖被多个路径引用 → 选择最高版本
   └─ 冲突不可自动解决 → 报错提示

3. 传递依赖展开
   └─ 递归解析每个依赖的 dependencies
   └─ 生成完整的依赖图（DependencyGraph）

4. 平台过滤
   └─ 检查 [target] 字段，排除不兼容平台的依赖
   └─ 示例：native 依赖仅在 hybrid/native 包时下载
```

### 8.3 依赖锁定（BOM 机制）

```toml
# 在项目根 aura.toml 中声明依赖版本锁定（类 Maven dependencyManagement）
[dependency-management]
aura-json = "^1.2"          # 所有 aura-json 依赖统一为 ^1.2
aura-http = ">=2.0"
aura-test = "^1.0"

# Workspace 级别的统一版本管理
[workspace.dependency-management]
aura-json = "^1.2"
aura-http = ">=2.0"
```

**效果**：当多个项目依赖 `aura-json` 时，统一解析为 `^1.2` 兼容范围内的最新版本。

---

## 9. 插件系统

### 9.1 插件模型

```rust
/// 构建插件接口
trait BuildPlugin {
    /// 插件名称
    fn name(&self) -> &str;
    
    /// 插件版本
    fn version(&self) -> &str;
    
    /// 插件类型：约定插件（自动激活）或显式插件
    fn kind(&self) -> PluginKind;
    
    /// 初始化：注册任务、约定、默认配置
    fn configure(&self, context: &mut PluginContext) -> Result<(), PluginError>;
    
    /// 执行任务（插件自定义任务）
    fn execute(&self, task: &TaskDefinition, context: &PluginContext) -> Result<TaskResult, PluginError>;
}

enum PluginKind {
    /// 约定插件：自动激活，提供默认行为
    Convention,
    /// 显式插件：需手动启用
    Explicit,
    /// 外部插件：编译为 .so/.dll 的插件
    External,
}

/// 插件上下文（提供给插件的 API）
struct PluginContext {
    /// 项目配置
    manifest: PackageManifest,
    /// 源码集列表
    source_sets: Vec<SourceSet>,
    /// 依赖图
    dependency_graph: DependencyGraph,
    /// 构建环境（缓存路径、目标平台等）
    environment: BuildEnvironment,
    /// 注册任务
    tasks: Vec<TaskDefinition>,
}
```

### 9.2 内置插件

| 插件名 | 类型 | 自动激活 | 功能 |
|--------|------|----------|------|
| `aura-stdlib` | Convention | ✅ | 自动注册标准库模块（io/math/string/json 等） |
| `aura-test-harness` | Convention | ✅ | 注册 `run-tests` 任务，注入测试框架 |
| `aura-doc-gen` | Explicit | ❌ | 生成 API 文档（Markdown + HTML） |
| `aura-format` | Explicit | ❌ | 源码格式化 |
| `aura-aot` | Explicit | ❌ | AOT 编译（LLVM 后端） |
| `aura-watch` | Convention | ✅ | 文件监听 + 增量重编 |
| `aura-ci` | Explicit | ❌ | CI/CD 流水线脚本 |

### 9.3 插件启用方式

```toml
# aura.toml
[plugins]
# 约定插件自动激活，无需声明
# 显式插件需手动启用：
aura-doc-gen = true
aura-format = true
aura-aot = true

# 外部插件（Phase 3+）：
# "aura-custom" = { path = "plugins/custom.plugin", version = "^0.1" }
```

### 9.4 插件任务注册示例

```rust
impl BuildPlugin for DocGenPlugin {
    fn name(&self) -> &str { "aura-doc-gen" }
    fn version(&self) -> &str { "1.0.0" }
    fn kind(&self) -> PluginKind { PluginKind::Explicit }
    
    fn configure(&self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        // 注册 "doc" 任务
        ctx.tasks.push(TaskDefinition {
            name: "doc".to_string(),
            description: "生成 API 文档".to_string(),
            kind: TaskKind::Plugin(TaskKindId::DocGen),
            depends_on: vec!["compile-main".to_string()],
            inputs: ctx.source_sets[0].all_files(),
            outputs: TaskOutputs { files: vec![], dir: ctx.environment.doc_dir.clone() },
            config: TaskConfig::default(),
        });
        Ok(())
    }
    
    fn execute(&self, task: &TaskDefinition, ctx: &PluginContext) -> Result<TaskResult, PluginError> {
        // 生成文档逻辑
        ...
    }
}
```

---

## 10. 构建缓存

### 10.1 两级缓存架构

```text
┌─────────────────────────────────────────────┐
│              任务执行                         │
│  1. 计算 fingerprint                         │
│  2. 查本地缓存（target/cache/）               │
│  3. 查远程缓存（Build Cache）                 │
│  4. 未命中 → 执行任务 → 写入两级缓存          │
└─────────────────────────────────────────────┘
```

### 10.2 本地缓存

```text
target/cache/
├── fingerprints.json           # 任务 fingerprint 缓存
├── artifacts/                  # 任务输出产物缓存
│   ├── compile-main/
│   │   ├── main.auc
│   │   ├── utils.auc
│   │   └── math/vector.auc
│   ├── compile-test/
│   │   └── utils_test.auc
│   └── package/
│       └── my-app-1.0.0.apkg
└── metadata.json               # 缓存元数据（版本、时间戳、大小）
```

### 10.3 远程缓存（Build Cache）

```text
# aura.toml
[build]
cache-remote = "https://cache.aura-lang.dev"  # 远程缓存服务器
cache-remote-enabled = true                    # 启用远程缓存
cache-remote-shared = false                    # 是否共享缓存（CI 用）
```

**缓存键**：
```text
cache_key = SHA256(
  任务名称
  + 任务输入指纹（源文件哈希 + 选项）
  + 编译器版本
  + 目标平台
  + 插件版本
)
```

### 10.4 Fingerprint 计算

```rust
fn compute_fingerprint(task: &TaskDefinition) -> String {
    let mut hasher = Sha256::new();
    
    // 1. 任务名称
    hasher.update(task.name.as_bytes());
    
    // 2. 源文件内容哈希
    for file in &task.inputs.files {
        let content = fs::read(file)?;
        let hash = hash_bytes(&content);
        hasher.update(hash);
    }
    
    // 3. 编译选项
    for (k, v) in &task.inputs.options {
        hasher.update(k.as_bytes());
        hasher.update(v.as_bytes());
    }
    
    // 4. 依赖任务的 fingerprint
    for (name, fp) in &task.inputs.dep_fingerprints {
        hasher.update(name.as_bytes());
        hasher.update(fp.as_bytes());
    }
    
    // 5. 编译器版本
    hasher.update(compiler_version().as_bytes());
    
    // 6. 插件版本
    for plugin in active_plugins() {
        hasher.update(plugin.name().as_bytes());
        hasher.update(plugin.version().as_bytes());
    }
    
    hex::encode(hasher.finalize())
}
```

### 10.5 缓存失效策略

| 触发条件 | 失效范围 |
|----------|----------|
| 源文件内容变化 | 该文件所属模块 + 所有依赖该模块的任务 |
| 编译选项变化 | 所有编译任务 |
| 编译器版本变化 | 所有任务 |
| 依赖版本变化 | 所有依赖该包的任务 |
| 插件版本变化 | 插件注册的所有任务 |
| `--clean` | 全部缓存 |
| `--no-cache` | 禁用缓存（但不清理） |

---

## 11. 构建包装器（Build Wrapper）

### 11.1 设计

```text
my-project/
├── aura-wrapper/
│   ├── aura-wrapper.jar          # 包装器 JAR（Rust 编译）
│   ├── aura-wrapper.cfg          # 配置：编译器版本、下载 URL
│   └── aura.bat / aura.sh        # 启动脚本
├── aura.toml
├── src/
└── ...

# 使用方式：
./aura-wrapper build          # 等价于 aura build
./aura-wrapper test           # 等价于 aura test
./aura-wrapper run            # 等价于 aura run
```

### 11.2 工作原理

```text
1. 用户执行 ./aura-wrapper build
2. aura-wrapper.cfg 指定编译器版本（如 0.3.0）
3. 检查本地是否已安装该版本编译器
   - 已安装 → 直接使用
   - 未安装 → 从 aura-lang.dev 下载 → 解压到 ~/.aura/wrapper/
4. 执行 aura build（使用指定版本的编译器）
```

### 11.3 aura-wrapper.cfg 格式

```toml
# 指定编译器版本
distribution-url = "https://github.com/aura-lang/aura/releases/download/v0.3.0/aura-0.3.0.zip"

# 本地缓存目录
wrapper-cache-dir = "~/.aura/wrapper"

# 校验和（SHA-256）
checksum = "abc123def456..."

# 安装超时
timeout = 60
```

### 11.4 与 Gradle Wrapper / Maven Wrapper 的对比

| 维度 | Gradle Wrapper | Maven Wrapper | **Aura Wrapper** |
|------|----------------|---------------|------------------|
| 配置文件 | `gradle-wrapper.properties` | `.mvn/wrapper/maven-wrapper.properties` | `aura-wrapper.cfg` |
| 脚本 | `gradlew` / `gradlew.bat` | `mvnw` / `mvnw.bat` | `aura-wrapper` / `aura-wrapper.bat` |
| 下载内容 | Gradle 发行版 | Maven 发行版 | Aura 编译器 + VM |
| 缓存位置 | `~/.gradle/wrapper/dists/` | `~/.m2/wrapper/dists/` | `~/.aura/wrapper/` |
| 可重复构建 | ✅ | ✅ | ✅ |

---

## 12. 多项目管理（Workspace）

### 12.1 单项目模式（默认）

```text
my-project/
├── aura.toml          # 项目配置
├── src/
├── test/
└── target/            # 构建产物
```

### 12.2 多项目模式（Workspace）

```text
mono-repo/
├── aura.toml          # Workspace 根配置
├── libs/
│   ├── core/
│   │   ├── aura.toml  # 库项目（library = true）
│   │   └── src/
│   └── utils/
│       ├── aura.toml  # 库项目
│       └── src/
├── apps/
│   ├── server/
│   │   ├── aura.toml  # 应用项目
│   │   └── src/
│   └── cli/
│       ├── aura.toml  # CLI 项目
│       └── src/
├── target/            # 共享构建产物目录
└── aura.lock          # 共享锁文件
```

**Workspace aura.toml**：
```toml
[workspace]
members = [
  "libs/core",
  "libs/utils",
  "apps/server",
  "apps/cli",
]
default-members = ["."]    # 默认构建所有成员

# Workspace 级别的依赖管理（BOM）
[workspace.dependency-management]
aura-json = "^1.2"
aura-http = ">=2.0"

# Workspace 级别的编译选项（默认值）
[workspace.build]
opt-level = 2
target = "x86_64-pc-windows-msvc"
```

**子项目 aura.toml**（如 `libs/core/aura.toml`）：
```toml
name = "aura-core"
version = "1.0.0"
library = true

[dependencies]
aura-json = "^1.0"    # 版本被 Workspace 的 dependency-management 覆盖为 ^1.2

[build]
# 子项目可覆盖 Workspace 默认值
opt-level = 3
```

### 12.3 Workspace 构建

```bash
# 构建所有成员
aura build

# 构建指定成员
aura build --member libs/core

# 构建指定成员 + 其依赖
aura build --member apps/server --with-deps

# 运行所有成员的测试
aura test

# 运行指定成员的测试
aura test --member libs/core
```

### 12.4 Workspace 依赖解析

```text
Workspace 解析顺序：
  1. Workspace dependency-management 版本锁定
  2. 子项目 dependencies 声明
  3. 外部依赖（Git / Registry）

共享缓存：
  - target/ 目录在 Workspace 根
  - 各子项目共享 fingerprint 缓存
  - 修改 libs/core → 仅重编依赖 core 的子项目
```

---

## 13. 仓库管理

### 13.1 仓库类型

| 类型 | 用途 | 示例 |
|------|------|------|
| `registry` | 中心化制品注册表 | `https://registry.aura-lang.dev` |
| `git` | Git 仓库（源码） | `https://github.com/user/lib.git` |
| `path` | 本地路径（源码） | `../local-lib` |
| `file` | 本地 .apkg 文件 | `./vendor/lib.apkg` |
| `local` | 本地安装目录 | `~/.aura/registry/` |

### 13.2 仓库解析优先级

```text
1. 本地安装目录（~/.aura/registry/）
   └─ 已安装的 .apkg 制品
   
2. 本地路径（path 依赖）
   └─ 直接引用本地项目
   
3. 仓库（registry 依赖）
   └─ 从远程注册表下载 .apkg
   
4. Git（源码依赖）
   └─ Git clone → 本地编译
   
5. 文件（.apkg 文件依赖）
   └─ 直接引用 .apkg 文件
```

### 13.3 注册表协议

```text
Registry API（REST）：

GET  /v1/packages/{name}                     → 包信息 + 版本列表
GET  /v1/packages/{name}/versions/{version}  → 版本详情 + 下载 URL
POST /v1/packages/{name}                      → 发布新包（需认证）
GET  /v1/packages/{name}/search?q=keyword    → 搜索包

响应格式（JSON）：
{
  "name": "aura-json",
  "latest": "1.2.3",
  "versions": [
    {
      "version": "1.2.3",
      "checksum": "sha256:abc...",
      "download": "https://registry.aura-lang.dev/v1/packages/aura-json/1.2.3/aura-json.apkg",
      "released": "2024-01-15T10:00:00Z"
    }
  ],
  "metadata": {
    "description": "JSON 解析与序列化",
    "license": "MIT"
  }
}
```

### 13.4 本地注册表

```text
~/.aura/registry/
├── aura-json/
│   ├── 1.2.3/
│   │   ├── aura-json.apkg      # 制品文件
│   │   └── metadata.json       # 包元数据
│   └── index.json              # 本地索引
├── aura-http/
│   └── ...
└── index.json                   # 全局索引
```

---

## 14. CI/CD 集成

### 14.1 `aura ci` 命令

```bash
# 完整 CI 流水线
aura ci --profile ci

# 自定义流水线
aura ci --steps clean,resolve,compile,test,package,verify,publish

# 仅运行测试
aura ci test
```

### 14.2 CI 配置（.aura-ci.yml）

```yaml
# .aura-ci.yml（可选：CI 配置文件）
version: "1.0"

# 触发条件
triggers:
  push:
    branches: ["main", "release/**"]
  pull_request:
    branches: ["main"]

# 构建环境
env:
  AURA_PROFILE: "ci"
  AURA_CACHE_REMOTE: "https://cache.aura-lang.dev"

# 流水线步骤
steps:
  - name: "安装依赖"
    command: "aura resolve"
    
  - name: "编译"
    command: "aura compile"
    cache: true
    
  - name: "测试"
    command: "aura test"
    parallel: true
    
  - name: "打包"
    command: "aura package"
    
  - name: "发布"
    command: "aura publish"
    condition: "on_success"

# 通知
notifications:
  slack:
    webhook: "${SLACK_WEBHOOK_URL}"
    on_failure: true
```

### 14.3 GitHub Actions 集成

```yaml
# .github/workflows/aura-ci.yml
name: Aura CI

on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Aura
        run: |
          curl -fsSL https://install.aura-lang.dev | bash
      
      - name: Resolve dependencies
        run: aura resolve
      
      - name: Compile
        run: aura compile
      
      - name: Test
        run: aura test
      
      - name: Package
        run: aura package
      
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: aura-package
          path: target/build/*.apkg
```

### 14.4 CI 最佳实践

```text
1. 缓存优化
   └─ 启用远程缓存，CI 间共享编译产物
   
2. 并行测试
   └─ aura test --parallel 并行执行测试模块
   
3. 增量构建
   └─ CI 间共享 target/cache/，仅重编变化的模块
   
4. 构建矩阵
   └─ 多平台：aura build --target x86_64-linux-gnu
   └─ 多版本：aura build --compiler-version 0.3.0
   
5. 制品归档
   └─ aura package → 上传到 CI artifact
```

---

## 15. IDE 集成

### 15.1 项目模型导出

```text
aura ide export
  │
  └─ 生成 aura-project.json（IDE 可读的项目模型）

aura-project.json:
{
  "version": 1,
  "compilerVersion": "0.3.0",
  "rootDir": "D:/Code/AuraLang/my-app",
  "sourceSets": {
    "main": {
      "sourceDirs": ["src/"],
      "modules": [
        { "name": "main", "path": "src/main.aura", "entry": true },
        { "name": "utils", "path": "src/utils.aura" },
        { "name": "math", "path": "src/math/mod.aura" },
        { "name": "math.vector", "path": "src/math/vector.aura" }
      ]
    },
    "test": {
      "sourceDirs": ["test/"],
      "modules": [
        { "name": "test.utils_test", "path": "test/utils_test.aura" }
      ]
    }
  },
  "dependencies": [
    { "name": "aura-json", "version": "1.2.3", "path": "~/.aura/cache/..." }
  ],
  "tasks": [
    "clean", "resolve", "compile-main", "compile-test", "run-tests", "package"
  ]
}
```

### 15.2 VS Code 扩展集成

```text
vscode-extension/
├── package.json          # 扩展配置
├── src/
│   ├── client.ts         # LSP 客户端
│   ├── projectModel.ts   # 项目模型解析
│   ├── tasks.ts          # 构建任务集成
│   └── diagnostics.ts    # 诊断同步
└── ...

# VS Code tasks.json 自动注入：
{
  "version": "2.0.0",
  "tasks": [
    { "label": "Aura: Build", "type": "aura", "command": "build" },
    { "label": "Aura: Test", "type": "aura", "command": "test" },
    { "label": "Aura: Run", "type": "aura", "command": "run" },
    { "label": "Aura: Watch", "type": "aura", "command": "watch" }
  ]
}
```

---

## 16. CLI 命令完整规范

### 16.1 命令层级

```text
aura
├── build               # 完整构建（clean → compile → test → package）
├── compile             # 仅编译
├── test                # 编译 + 运行测试
├── run                 # 编译 + 运行
├── package             # 打包为 .apkg
├── clean               # 清理构建产物
├── resolve             # 解析依赖
├── install             # 安装依赖 / 安装到本地注册表
├── publish             # 发布到远程仓库
├── verify              # 验证制品完整性
├── deps                # 显示依赖树
├── new                 # 创建新项目
├── ci                  # CI 流水线
├── watch               # 监听源码变化，增量重编
├── wrapper             # 构建包装器管理
├── doc                 # 生成文档
├── fmt                 # 格式化源码
├── check               # 语法/语义检查
├── tokens              # 输出词法分析
├── ast                 # 输出 AST
├── disasm              # 反汇编 .auc
├── eval                # 执行代码片段
├── repl                # 交互式 REPL
├── leak-check          # 内存泄漏检测
├── lsp                 # 启动 LSP 服务器
├── version             # 显示版本
└── help                # 显示帮助
```

### 16.2 通用参数

```text
--profile <name>        # 激活的 profile（debug/release/ci）
--dir <path>            # 项目目录（默认当前目录）
--member <name>         # 指定 Workspace 成员
--with-deps             # 包含依赖的成员
--target <triple>       # 目标平台三元组
--opt <level>           # 优化级别（0/1/2/3）
--debug                 # 启用调试信息
--no-cache              # 禁用缓存
--clean                 # 清理后重编
--parallel <n>          # 并行任务数（默认 CPU 核心数）
--dry-run               # 仅显示将要执行的任务，不执行
--verbose               # 详细输出
--json                  # JSON 格式输出
```

### 16.3 典型工作流

```bash
# 1. 创建项目
aura new my-app
cd my-app

# 2. 日常开发
aura run                          # 编译 + 运行（增量）
aura build --watch                # 监听模式
aura test                         # 运行测试

# 3. 发布流程
aura build --profile release      # 发布配置构建
aura verify                       # 验证制品
aura publish                      # 发布到仓库

# 4. CI 流水线
aura ci --profile ci              # 完整 CI

# 5. 多项目管理
aura build --member libs/core     # 构建指定库
aura build --member apps/server --with-deps  # 构建应用 + 依赖

# 6. 构建包装器
aura wrapper install              # 安装当前项目指定版本
./aura-wrapper build              # 使用指定版本构建
```

---

## 17. 与现有方案的关系

### 17.1 三层架构

```text
┌─────────────────────────────────────────────────────────┐
│                   构建系统层（本文档）                     │
│  生命周期 / 任务系统 / 源码集 / 依赖配置 / 插件 / 缓存     │
│  CLI: aura build / test / run / ci / watch / wrapper     │
├─────────────────────────────────────────────────────────┤
│                   多文件编译层（已有方案）                 │
│  源码发现 / 模块树 / import 解析 / 增量编译 / ModuleRegistry│
│  CLI: aura build --single / compile_source()             │
├─────────────────────────────────────────────────────────┤
│                   包格式层（已有方案）                     │
│  .apkg 容器 / .auc 标准格式 / 校验和 / 签名 / ModuleRegistry│
│  CLI: aura package / inspect / sign / verify              │
└─────────────────────────────────────────────────────────┘
```

### 17.2 依赖关系

```text
构建系统层（本文档）
  │
  ├── 依赖 → 多文件编译层
  │     └─ 构建系统的 "compile" 任务调用多文件编译的 compile_project()
  │
  ├── 依赖 → 包格式层
  │     └─ 构建系统的 "package" 任务调用包格式的 PackageBuilder
  │
  └── 被依赖 → IDE 集成 / CI/CD / 插件系统
        └─ 构建系统提供项目模型 + 任务 API
```

---

## 18. 实施路线图

### Phase B1：构建配置基础设施（1 周）

| 任务 | 内容 | 预估 |
|------|------|------|
| B1.1 | 扩展 `PackageManifest` 添加 `[build]` / `[plugins]` / `[profiles]` / `[repositories]` / `[workspace]` 表 | 1d |
| B1.2 | 实现 `SourceSet` 数据结构 + 默认源码集 | 1d |
| B1.3 | 实现依赖配置分组（compile-dependencies, runtime-dependencies 等） | 0.5d |
| B1.4 | 实现配置优先级解析（CLI > profile > project > default） | 1d |
| B1.5 | 实现 `aura.toml` 完整解析 + 验证 | 1d |
| B1.6 | 单元测试 | 0.5d |

**里程碑**：`aura.toml` 完整规范可解析，配置优先级正确。

### Phase B2：任务引擎（1.5 周）

| 任务 | 内容 | 预估 |
|------|------|------|
| B2.1 | 实现 `TaskDefinition` / `TaskGraph` 数据结构 | 1d |
| B2.2 | 实现拓扑排序 + 循环依赖检测 | 1d |
| B2.3 | 实现任务执行引擎（顺序 + 并行调度） | 2d |
| B2.4 | 实现内置任务（clean, resolve, compile, test, package, verify, run） | 2d |
| B2.5 | 实现 fingerprint 计算 + 增量检查 | 1d |
| B2.6 | 集成测试 | 1d |

**里程碑**：`aura build` 按 DAG 顺序执行任务，支持增量构建。

### Phase B3：构建缓存（1 周）

| 任务 | 内容 | 预估 |
|------|------|------|
| B3.1 | 实现本地缓存（fingerprint + artifacts） | 1d |
| B3.2 | 实现缓存失效策略 | 0.5d |
| B3.3 | 实现远程缓存协议（HTTP） | 1d |
| B3.4 | 实现缓存上传/下载 + 共享策略 | 1d |
| B3.5 | `--no-cache` / `--clean` 参数 | 0.5d |
| B3.6 | 集成测试 | 0.5d |

**里程碑**：增量构建正确跳过未变化任务，远程缓存可用。

### Phase B4：插件系统（1.5 周）

| 任务 | 内容 | 预估 |
|------|------|------|
| B4.1 | 实现 `BuildPlugin` trait + `PluginContext` | 1d |
| B4.2 | 实现约定插件加载（aura-stdlib, aura-test-harness） | 1d |
| B4.3 | 实现显式插件加载（aura-doc-gen, aura-format） | 1d |
| B4.4 | 实现外部插件加载（.so/.dll） | 1.5d |
| B4.5 | 插件任务执行 + 结果聚合 | 1d |
| B4.6 | 集成测试 | 1d |

**里程碑**：插件可注册任务，约定插件自动激活。

### Phase B5：构建包装器 + Workspace（1 周）

| 任务 | 内容 | 预估 |
|------|------|------|
| B5.1 | 实现 `aura-wrapper` 启动脚本 + 配置解析 | 1d |
| B5.2 | 实现编译器版本检查 + 自动下载 | 1d |
| B5.3 | 实现 Workspace 模式（members + default-members） | 1.5d |
| B5.4 | 实现 Workspace 依赖解析 + 共享缓存 | 1d |
| B5.5 | `--member` / `--with-deps` 参数 | 0.5d |
| B5.6 | 集成测试 | 0.5d |

**里程碑**：Wrapper 可重复构建，Workspace 多项目正确解析。

### Phase B6：仓库管理 + CI/CD（1 周）

| 任务 | 内容 | 预估 |
|------|------|------|
| B6.1 | 实现注册表协议（HTTP client） | 1d |
| B6.2 | 实现本地注册表管理 | 0.5d |
| B6.3 | 实现 `aura ci` 命令 + .aura-ci.yml 解析 | 1.5d |
| B6.4 | 实现 BOM 版本锁定 | 0.5d |
| B6.5 | GitHub Actions 集成示例 | 0.5d |
| B6.6 | 集成测试 | 0.5d |

**里程碑**：远程仓库可下载/发布，CI 流水线可运行。

### Phase B7：IDE 集成 + 开发体验（1 周）

| 任务 | 内容 | 预估 |
|------|------|------|
| B7.1 | 实现 `aura-project.json` 导出 | 1d |
| B7.2 | VS Code 扩展任务集成 | 1.5d |
| B7.3 | `aura --watch` 文件监听 | 1.5d |
| B7.4 | 编译日志优化（彩色、进度、错误高亮） | 1d |
| B7.5 | `aura wrapper install` 集成 | 0.5d |
| B7.6 | 文档 + 示例 | 0.5d |

**里程碑**：完整的 IDE 集成 + 开发体验。

### 总工期

| Phase | 周期 | 关键产出 |
|-------|------|----------|
| B1 | 1 周 | 构建配置基础设施 |
| B2 | 1.5 周 | 任务引擎 |
| B3 | 1 周 | 构建缓存 |
| B4 | 1.5 周 | 插件系统 |
| B5 | 1 周 | 构建包装器 + Workspace |
| B6 | 1 周 | 仓库管理 + CI/CD |
| B7 | 1 周 | IDE 集成 + 开发体验 |

**总计：8 周**（可与多文件编译方案 Phase M1-M5 并行推进，共享基础设施）

---

## 19. 关键设计决策

| # | 决策 | 选择 | 理由 |
|---|------|------|------|
| D1 | 配置语言 | **纯 TOML（非代码配置）** | 零引导循环 + 安全边界清晰 + 解析快 + 可静态验证 + 可重现性高。不选 `build.aura` 类代码配置（引导循环、安全风险、解析慢）。详见 §2.6。 |
| D2 | 配置文件 | 单一 `aura.toml` | 对标 Cargo.toml，不引入 BUILD/aurafile |
| D3 | 任务模型 | 任务 DAG（类 Gradle） | 比 Maven 生命周期更灵活，支持并行 |
| D4 | 源码集 | 声明式（类 Gradle sourceSets） | 比 Rust 约定更灵活，支持自定义 |
| D5 | 依赖配置 | 分组（implementation/compileOnly/...） | 类 Gradle，比 Cargo 的单一 [dependencies] 更精确 |
| D6 | 构建缓存 | 两级（本地 + 远程） | 类 Gradle Build Cache，CI 间共享 |
| D7 | 构建包装器 | `aura-wrapper` | 类 Gradle Wrapper，可重复构建 |
| D8 | 多项目 | Workspace 模式（类 Cargo） | 单一配置文件，不引入 settings.gradle |
| D9 | 插件系统 | trait 接口 + 外部 .so | 弥补 TOML 无代码执行的缺口，可扩展 |
| D10 | 配置优先级 | CLI > profile > project > default | 类 Maven profiles，直观 |
| D11 | 仓库协议 | REST API（类 npm registry） | 标准 HTTP，易实现易扩展 |

---

## 20. 附录：与其他构建系统的细节对照

### 20.1 Maven pom.xml vs aura.toml

| Maven 元素 | aura.toml 等价 |
|------------|----------------|
| `<groupId>` / `<artifactId>` / `<version>` | `name` / `version` |
| `<modules>` | `[workspace] members` |
| `<dependencies>` | `[dependencies]` |
| `<dependencyManagement>` | `[workspace.dependency-management]` |
| `<build><sourceDirectory>` | `[build] [source-sets.main] source-dirs` |
| `<build><plugins>` | `[plugins]` |
| `<profiles>` | `[profiles]` |
| `<repositories>` | `[repositories]` |
| `<distributionManagement>` | `[repositories.publish]` |
| `mvn clean` | `aura clean` |
| `mvn compile` | `aura compile` |
| `mvn test` | `aura test` |
| `mvn package` | `aura package` |
| `mvn install` | `aura install` |
| `mvn deploy` | `aura publish` |

### 20.2 Gradle build.gradle vs aura.toml

| Gradle 声明 | aura.toml 等价 |
|-------------|----------------|
| `apply plugin: 'java'` | 约定插件自动激活 |
| `sourceSets { main { java { srcDir } } }` | `[build] [source-sets.main] source-dirs` |
| `dependencies { implementation '...' }` | `[dependencies]` |
| `dependencies { compileOnly '...' }` | `[compile-dependencies]` |
| `dependencies { runtimeOnly '...' }` | `[runtime-dependencies]` |
| `dependencies { testImplementation '...' }` | `[dev-dependencies]` |
| `tasks { custom { ... } }` | `[[tasks.custom]]` |
| `--parallel` | `--parallel` |
| `--build-cache` | `--cache-remote` |
| `settings.gradle` | `[workspace]` |

### 20.3 Cargo.toml vs aura.toml

| Cargo 元素 | aura.toml 等价 |
|------------|----------------|
| `[package] name/version` | `name` / `version` |
| `[lib] path` | `[build] [source-sets.main]` + `entry` |
| `[[bin]] path` | `entry` |
| `[dependencies]` | `[dependencies]` |
| `[dev-dependencies]` | `[dev-dependencies]` |
| `[build-dependencies]` | `[build-dependencies]` |
| `[features]` | `[plugins]` |
| `[workspace] members` | `[workspace] members` |
| `cargo build` | `aura build` |
| `cargo test` | `aura test` |
| `cargo run` | `aura run` |
| `cargo publish` | `aura publish` |

---

**文档状态**：方案设计（未实施）
**前置依赖**：多文件编译方案 Phase M1-M2（源码发现 + 多模块字节码）
**后续依赖**：无（构建系统是最终集成层）
**下一步**：评审通过后进入 Phase B1（构建配置基础设施）实施。
