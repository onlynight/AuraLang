# 欢迎使用 Aura 语言 🌟

> **NovaOS 的下一代嵌入式系统脚本语言** — 面向资源受限环境的零开销脚本

---

## 什么是 Aura？

**Aura**（光环）是专为 NovaOS 嵌入式系统设计的系统级脚本语言：

- **执行层**：AOT + JIT 混合编译 — 接近 C 性能
- **内存层**：引用计数（ARC）— 无 GC 停顿，确定释放
- **集成层**：零开销 FFI — 直接调用 C/Raylib，无桥接层
- **生态层**：声明式包管理 — 去中心化依赖

## 核心理念

> **"为嵌入式而生 — 零 GC、零桥接、零依赖"**

### 设计原则

1. **系统优先**：面向嵌入式系统与设备端，而非通用应用
2. **内存确定性**：ARC 引用计数，无 GC 暂停，适合实时场景
3. **零开销集成**：FFI 直调 C 函数，无中间层开销
4. **类型安全**：空安全、类型安全、编译期检查
5. **显式优于隐式**：内存分配、错误处理、类型转换显式声明
6. **轻量部署**：单二进制部署，无运行时依赖

## 快速开始

### 安装

```bash
# 从源码构建
git clone https://github.com/aura-lang/auralang.git
cd auralang
cargo build --release

# 或使用 cargo install
cargo install --git https://github.com/aura-lang/auralang aura-cli
```

### 第一个程序

创建 `hello.aura`：

```aura
fun main() {
    println("Hello, Aura!")
}
```

运行：

```bash
aura run hello.aura
```

### 编写包

```bash
aura new my-project
cd my-project
aura run main.aura
```

## 文档结构

- [第一章：语言教程](./chapter-01.md) — 从入门到精通
- [第二章：标准库 API](../docs/api/index.md) — 19 个模块，200+ 函数
- [第三章：示例项目](./chapter-02.md) — 游戏、工具、服务
- [第四章：Lua 迁移指南](./chapter-03.md) — 从 Lua 到 Aura
- [第五章：性能基准报告](./chapter-04.md) — 性能对比与优化建议

## 许可证

Aura 采用 [Apache-2.0](../LICENSE) 许可证。