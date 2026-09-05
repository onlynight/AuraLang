# 第四章：性能基准报告

> Aura 性能基准测试 — VM / JIT / AOT 对比

---

## 4.1 测试环境

| 组件 | 配置 |
|------|------|
| 处理器 | Intel Core i7 (WSL2) |
| 内存 | 16 GB |
| 操作系统 | Windows 11 + WSL2 Ubuntu |
| Aura 版本 | 0.1.0 (2026-09) |
| LLVM | 23.1.0 |
| 对比基线 | Lua 5.4 |

## 4.2 基准测试结果

### 4.2.1 数学计算

| 场景 | VM 解释器 | JIT | AOT (LLVM) | AOT 加速 |
|------|----------|-----|------------|---------|
| fib(20) | 227 ms/op | 229 ms/op | 5.1 ms/op | **~44x** |
| sum(60,000) | 55 ms/op | 55 ms/op | 4.2 ms/op | **~13x** |
| count(100) | 0.5 ms/op | 0.5 ms/op | 0.08 ms/op | **~6x** |
| fact(20) | 0.3 ms/op | 0.3 ms/op | 0.05 ms/op | **~6x** |

### 4.2.2 并发基准

| 场景 | 操作 | 耗时 |
|------|------|------|
| 协程调度 | 5 协程 spawn | 0.003 ms/op |
| Actor 消息 | 5 消息 send | 0.003 ms/op |
| Channel 操作 | 5 send + 5 recv | 0.006 ms/op |
| Select 多路复用 | 2 通道 select | 0.002 ms/op |
| 监督树 | 3 子 Actor | 0.005 ms/op |
| 综合场景 | 协程+Actor+Channel | 0.005 ms/op |

### 4.2.3 词法分析性能

| 输入大小 | 扫描速度 |
|---------|---------|
| 1 MB | 60+ MB/s |
| 10 MB | 60+ MB/s |

## 4.3 JIT 未触发分析

> JIT 在基准测试中未触发热点编译，原因分析：

1. **编译器内联消灭调用点**：LLVM 优化器将小函数直接内联，热点计数点被消除
2. **入口函数不参与热点计数**：main 函数本身不被计数
3. **递归热点被 JIT 白名单拒绝**：递归函数不在 JIT 可编译白名单中

**三层叠加导致 JIT 从未派发原生码**。AOT 编译无此问题，直接生成优化后的原生代码。

### 解决方案

1. 标记热点函数为 `inline` 或 `noinline`
2. 增大 JIT 阈值
3. 扩展 JIT 白名单（支持递归函数）

## 4.4 内存管理性能

### ARC 开销

| 操作 | 开销 |
|------|------|
| retain | 3-5% |
| release | 3-5% |
| 逃逸分析 | 编译期 |
| 栈分配 | 0% |

### 内存泄漏检测

```bash
# 检测潜在泄漏
aura leak-check main.aura

# 输出示例
=== ARC 分析报告 ===
逃逸分配: 5
非逃逸分配: 12
Retain 插入: 15
Release 插入: 15
消除 Retain: 3
消除 Release: 3

✅ 内存泄漏检测: 无泄漏
```

## 4.5 优化建议

### 5.5.1 代码层优化

1. **优先栈分配**：避免不必要的 `box` 操作
2. **减少 ARC**：使用 `val` 替代 `var`，减少保留次数
3. **使用逃逸分析**：未逃逸对象可栈分配
4. **消除冗余引用**：编译器自动消除冗余 retain/release

### 5.5.2 编译层优化

```bash
# AOT 编译（推荐）
aura build main.aura --aot --opt 2

# 优化级别
# O0: 无优化，最快编译
# O1: 基础优化
# O2: 标准优化（默认，推荐）
# O3: 激进优化
# Os/Oz: 体积优化
```

### 5.5.3 架构层优化

1. **关键路径 AOT**：将热点函数编译为原生代码
2. **冷代码字节码**：使用字节码模式热更新
3. **混合模式**：核心 AOT + 脚本字节码

## 4.6 与 Lua 性能对比

| 场景 | Lua 5.4 | Aura VM | Aura AOT | AOT 加速 |
|------|---------|---------|---------|---------|
| 数学计算 | 1x | 2x | 13-44x | 6.5-22x |
| 字符串处理 | 1x | 1.5x | 5-10x | 3.3-6.7x |
| 文件 I/O | 1x | 1x | 1x | 1x |
| 网络 I/O | 1x | 1x | 1x | 1x |
| GC 暂停 | 有 | 无 | 无 | N/A |

## 4.7 基准测试代码

### 数学基准

```rust
// benches/vm_benchmarks.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark_fib(c: &mut Criterion) {
    let code = r#"
        fun fib(n: Int): Int {
            if (n <= 1) return n
            return fib(n - 1) + fib(n - 2)
        }
        fun main() { println(fib(20)) }
    "#;
    // ... benchmark code
}

criterion_group!(benches, benchmark_fib);
criterion_main!(benches);
```

### 并发基准

```rust
// benches/p10_benchmarks.rs
fn benchmark_coroutine(c: &mut Criterion) {
    let code = r#"
        fun main() {
            for (i in 0..5) {
                aura.concurrent.spawn(fun() {
                    println("coroutine $i")
                })
            }
        }
    "#;
    // ... benchmark code
}
```

## 4.8 性能总结

| 指标 | 结果 | 状态 |
|------|------|------|
| AOT 加速比 | 13x-44x | ✅ 超过目标 |
| 目标 | C 的 90% | 🟡 接近 |
| JIT 加速 | 待验证 | 🔴 需修复 |
| 词法分析 | 60+ MB/s | ✅ 超过目标 |
| ARC 开销 | 3-5% | ✅ 可接受 |
| 并发延迟 | <0.01ms | ✅ 优秀 |

---

> **完整基准测试代码**：见 `compiler/benches/` 目录