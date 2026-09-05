# 欢迎使用 Aura 语言 🌟

> **NovaOS 的下一代系统级脚本语言** — LuaJIT 的完美继任者

---

## 什么是 Aura？

**Aura**（光环）是专为 NovaOS 设计的系统级脚本语言：

- **语法层**：100% Kotlin 兼容 — 零学习成本
- **执行层**：AOT + JIT 混合编译 — 接近 C 性能
- **生态层**：声明式包管理 — 去中心化依赖
- **集成层**：零开销 FFI — 无缝调用 C/Raylib

## 核心理念

> **"语法100% Kotlin，底层100%原生"**

### 设计原则

1. **Kotlin兼容**：类型系统、语法特性完全对齐 Kotlin
2. **性能优先**：栈分配、零成本抽象、无 GC
3. **安全可控**：空安全、类型安全、内存安全（ARC）
4. **显式优于隐式**：内存分配、错误处理、类型转换显式声明
5. **系统脚本化**：既有脚本的开发效率，又有系统级的执行效率

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

Aura 采用 [MIT](LICENSE) 许可证。