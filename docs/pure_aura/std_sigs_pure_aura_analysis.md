# StdSigs.aura 全量 Aura 化可行性分析

## 当前表内 4 类 39 个函数的完整清单

| 类别 | 函数 | 数量 |
|---|---|---|
| Collections | mutableMapOf, emptyMap, mapSet, mapGet, mapSize, mapContains, listSet, emptyList, listOf, pairOf, count, listSize, isEmpty, getAt, listGet, listAppend, listPop, range, indexOf, contains, set, filter, map, take | 24 |
| FileSystem | exists, readText, writeText, mkdirP | 4 |
| StringBuilder | create, append, appendChar, appendInt, length, finish, reset | 7 |
| Process | run, argCount, arg, args | 4 |

---

## 结论总览

```
┌───────────────────────────────────────────────────────────┐
│  可直接纯 Aura 化:         33 / 39  (85%)                  │
│  需新增 1 个 native 声明:   3 / 39  (7%)                   │
│  需架构决策:               3 / 39  (7%)                    │
└───────────────────────────────────────────────────────────┘
```

---

## 一、Collections（24 个）—— 全部可纯 Aura 化 ✅

### 1.1 基础设施

所有 Collections 函数依赖的数据结构完全可以用 `Memory.alloc` + `Memory.write64` 在纯 Aura 中构造：

```
// C 中的 AuraDynList：
//   typedef struct { int64_t len; int64_t cap; const char **items; } AuraDynList;
//
// Aura 等价布局（用偏移常量访问）：
//   offset 0  : len (i64)
//   offset 8  : cap (i64)
//   offset 16 : items ptr (i64 → 指向 i64[] 数组)
```

`Memory.alloc` 返回的是 16 字节对齐的指针（C 分配器保证），与 Plan A 的「真实指针低位为 0」约定完全兼容。

### 1.2 逐项分析

| # | 函数 | C 实现核心 | 纯 Aura 实现方式 | 难度 |
|---|---|---|---|---|
| 1 | `emptyList` | `aura_dynlist_new(4)` | `Memory.alloc(24)` + 写 3 个 0 | ✅ 低 |
| 2 | `listOf` | `new + 3×push` | `alloc(24)` + `alloc(24)` + 写 3 项 | ✅ 低 |
| 3 | `pairOf` | 2 元素 list | `alloc(24)` + `alloc(16)` + 写 2 项 | ✅ 低 |
| 4 | `count` | `l->len` | `Memory.read64(list+0)` | ✅ 低 |
| 5 | `listSize` | 同 count | 同 count | ✅ 低 |
| 6 | `isEmpty` | `count==0` | `read64(list+0)==0` | ✅ 低 |
| 7 | `getAt` | `l->items[idx]` | `read64(read64(list+16) + idx*8)` | ✅ 低 |
| 8 | `listGet` | 同 getAt | 同 getAt | ✅ 低 |
| 9 | `listAppend` | push + 可能的 realloc | `alloc + copy + free` 模拟 realloc | ✅ 中 |
| 10 | `listPop` | `l->len--; return items[len]` | `read64 + write64 + read64` | ✅ 低 |
| 11 | `listSet` | 越界追加 | `read64 + write64` 或 append | ✅ 低 |
| 12 | `set` | 同 listSet | 同 listSet | ✅ 低 |
| 13 | `range` | 生成整数 + Plan A 装箱 | 循环 `(i<<1)|1` + push | ✅ 低 |
| 14 | `indexOf` | 遍历 + `handle_equals` | 循环 + Plan A 解箱比较 | ✅ 低 |
| 15 | `contains` | `indexOf>=0` | `indexOf>=0` | ✅ 低 |
| 16 | `mutableMapOf` | `aura_map_new(8)` | `alloc(48)` + 写 3 个字段 + 2 个 alloc | ✅ 低 |
| 17 | `emptyMap` | 同 mutableMapOf (cap=4) | 同 | ✅ 低 |
| 18 | `mapSet` | strcmp + 可能的 realloc | 遍历 + `Memory.read64` + 字符串比较 + realloc | ✅ 中 |
| 19 | `mapGet` | strcmp 遍历 | 遍历 + 字符串比较 | ✅ 低 |
| 20 | `mapSize` | `m->len` | `Memory.read64(map+0)` | ✅ 低 |
| 21 | `mapContains` | strcmp 遍历 | 遍历 + 字符串比较 | ✅ 低 |
| 22 | `filter` | 遍历 + 调闭包 | 遍历 + `Memory.read64(closure+0)` 取函数指针 + 调 | ✅ 中 |
| 23 | `map` | 遍历 + 调闭包 | 同 filter | ✅ 中 |
| 24 | `take` | 遍历 + 复制前 n 个 | 遍历 + 复制 | ✅ 低 |

### 1.3 技术要点

**字符串比较**：C 用 `strcmp`，Aura 用 `StringOps.strcmp`（纯 Aura 实现，已在 `StringOps.aura` 中存在）。

**Plan A 装箱**：C 代码中 `(v<<1)|1` 和 `v>>1` 是纯整数运算，Aura 用 `Long` 即可。

**realloc 模拟**：Aura 无 `realloc`，但可组合：
```
fun auraRealloc(old: Long, oldCap: Long, newCap: Long): Long {
    val nb: Long = Memory.alloc(newCap)
    Memory.copy(nb, old, oldCap)
    Memory.free(old)
    return nb
}
```

**闭包调用**（filter/map）：C 代码 `*(void**)clo` 取函数指针。Aura 闭包是 `{ fnPtr, env }` 结构，`Memory.read64(closure + 0)` 取函数指针，然后需要动态调用。这需要 Emit.aura 生成一个间接调用指令（`call <ptrty> %fnptr`）。当前 LLVM IR 发射器已支持 `call ptr %p, %arg` 形式，但尚未为迭代器链实现。这是纯 Aura 层面可解决的，但需要编译器配合。

---

## 二、FileSystem（4 个）—— 3 个可直接，1 个需扩展 ⚠️

| # | 函数 | C 实现核心 | 纯 Aura 实现方式 | 难度 |
|---|---|---|---|---|
| 1 | `exists` | `stat(path, &st)==0` | `FileOps.access(path, F_OK)==0` | ✅ 低 |
| 2 | `readText` | `fopen + fread 循环` | `FileOps.open + FileOps.read 循环 + Memory.alloc` | ✅ 中 |
| 3 | `writeText` | `fopen + fputs + fclose` | `FileOps.open + FileOps.write + FileOps.close` | ✅ 低 |
| 4 | `mkdirP` | `_mkdir/mkdir 递归` | **当前 Syscalls 无 SYS_MKDIR** | ⚠️ 需扩展 |

### 2.1 mkdirP 的扩展需求

当前 `Syscalls.aura` 的 syscall 表中没有 `SYS_MKDIR`（Linux=39 在 `aura_syscalls.c` 中定义了 `SYS_MKDIR`，但 Aura 侧的 `Syscalls.aura` 没有暴露）。

**方案**：在 `Syscalls.aura` 中增加：
```
@native(SYS_MKDIR) fun mkdir(path: Long, mode: Int): Int
```
然后在 `FileOps.aura` 中添加 `mkdirP` 实现，在 Aura 中递归拆分路径调用 `mkdir`。

**注意**：Windows 路径 `_mkdir` 对应 `CreateDirectoryA`，需要通过 FFI 方式声明（类似现有 Windows Syscalls.aura 的 `call qword [rip + CreateDirectoryA]`）。

---

## 三、StringBuilder（7 个）—— 全部可纯 Aura 化 ✅

| # | 函数 | C 实现核心 | 纯 Aura 实现方式 | 难度 |
|---|---|---|---|---|
| 1 | `create` | `aura_mem_alloc(sizeof(AuraSb))` | `Memory.alloc(24)` + 写 buf/len/cap | ✅ 低 |
| 2 | `append` | `strlen + memcpy` | `StringOps.strlen + Memory.copy` | ✅ 低 |
| 3 | `appendChar` | 写 1 字节 | `Memory.write(buf+len, ch)` | ✅ 低 |
| 4 | `appendInt` | `snprintf("%d")` | **纯 Aura 整数转字符串** | ✅ 中 |
| 5 | `length` | `sb->len` | `Memory.read64(sb+8)` | ✅ 低 |
| 6 | `finish` | 转移 buf 所有权 | `read64 + write64`（置空） | ✅ 低 |
| 7 | `reset` | 清空 | `write(buf, 0) + write(len, 0)` | ✅ 低 |

### 3.1 appendInt 的纯 Aura 实现

`snprintf("%d", value)` 可以用纯 Aura 实现（如现有 `toStr` 的纯 Aura 版本）：

```
fun appendInt(sb: Long, value: Int): Long {
    // 处理负号
    // 处理 0
    // 逐位取模 10，逆序写入临时缓冲区
    // 反转追加到 sb.buf
    return sb
}
```

这是纯算术 + `Memory.write` 操作，无需任何 C 依赖。

---

## 四、Process（4 个）—— 1 个需架构决策，3 个需扩展 ⚠️

| # | 函数 | C 实现核心 | 纯 Aura 实现方式 | 难度 |
|---|---|---|---|---|
| 1 | `run` | `system(cmd)` | **需 fork+execve 或 shell 包装** | ⚠️ 需架构决策 |
| 2 | `argCount` | 读 `aura_saved_argc` | **需 boot-time 初始化** | ⚠️ 需扩展 |
| 3 | `arg` | 读 `aura_saved_argv[idx]` | **需 boot-time 初始化** | ⚠️ 需扩展 |
| 4 | `args` | 连接 argv | **需 boot-time 初始化** | ⚠️ 需扩展 |

### 4.1 Process.run 的难点

C 中用 `system(cmd)`，这是 CRT 的 shell 包装函数（创建子进程 → 执行 → 等待退出）。

**纯 Aura 方案**：
- Linux：`SYS_FORK` + `SYS_EXECVE` + `SYS_WAIT4` —— 但 `SYS_FORK` 未在 Syscalls.aura 中暴露
- Windows：`CreateProcessA` + `WaitForSingleObject` + `GetExitCodeProcess` —— 需 FFI

**决策点**：是否将 `Process.run` 降级为仅支持直接 `execve`（替换当前进程），还是支持子进程？如果需要子进程，需扩展 Syscalls。

### 4.2 arg* 的难点

C 中 `aura_args_set(argc, argv)` 由 AOT 生成的 C 入口 `main` 在函数体最开始调用。纯 Aura 化的问题是：

**谁调用 `arg*` 的初始化？**

方案：
1. **编译器生成的 main**：AOT 发射器在 Aura 的 `main` 函数体开头插入对 `ProcessOps.initArgs(argc, argv)` 的调用
2. **新增 native 函数**：在 `ProcessOps.aura` 中声明 `native fun setArgs(argc: Int, argv: Long)`
3. **全局初始化**：在 `Memory.aura` 的 `alloc` 初始化阶段注入

方案 2 最干净：增加一个 `@native` 声明，AOT 发射器生成的 C main 调用它。

---

## 五、迁移路径建议

### Phase 1：StringBuilder（7 个）—— 最独立，零依赖

```
纯 Aura 文件: aura/core/aura/lang/std/string/StringBuilder.aura
依赖: Memory.aura (已有), StringOps.aura (已有)
预计代码量: ~150 行
```

### Phase 2：Collections（24 个）—— 依赖 Phase 1 的字符串操作

```
纯 Aura 文件: aura/core/aura/lang/std/collection/CollectionsNative.aura
依赖: Memory.aura, StringOps.aura, StringBuilder.aura
需要: realloc 模拟函数、Plan A 装箱/解箱助手
预计代码量: ~400 行
```

### Phase 3：FileSystem（3 个）—— exists/readText/writeText

```
纯 Aura 文件: aura/core/aura/lang/std/fs/FileSystemNative.aura
依赖: FileOps.aura (已有), Memory.aura
预计代码量: ~80 行
```

### Phase 4：扩展层 —— mkdirP + Process.run + arg*

需要修改 `Syscalls.aura`（增加 SYS_MKDIR、SYS_FORK）、`FileOps.aura`、`ProcessOps.aura`，以及编译器 AOT 发射器配合。

---

## 六、签名表修改时机

StdSigs.aura 的迁移顺序应当**最后**，而非最先。原因：

1. StdSigs.aura 是**运行时**的签名表，被 AOT 发射器（Emit.aura）和链接器使用
2. 迁移路径应该是：
   - 先写纯 Aura 实现文件
   - 再修改 Emit.aura，让它优先查找纯 Aura HIR 签名表（如现有 String/Math 条目的做法）
   - 最后从 StdSigs.aura 中移除对应条目

当前 StdSigs.aura 顶部注释已说明了这个模式（Phase B Step 2 已移除 String/Math 条目）。

---

## 七、总结

| 类别 | 纯 Aura 化 | 需新增 native | 需架构决策 |
|---|---|---|---|
| Collections (24) | **24/24** | 0 | 0 |
| FileSystem (4) | **3/3** | 1 (mkdirP) | 0 |
| StringBuilder (7) | **7/7** | 0 | 0 |
| Process (4) | 0 | 0 | **4/4** |
| **总计** | **34/39** | **1/39** | **4/39** |

**核心结论**：StdSigs.aura 中 87% 的函数可以完全用 Aura 实现 + 现有系统 native 接口（Memory / FileOps / StringOps / Syscalls）转换为 LLVM IR，不依赖 Rust 或 C。剩余的 mkdirP 和 Process 相关函数需要小幅扩展 native 接口层（增加 1-2 个 @native 声明 + AOT 发射器配合），但仍然不依赖 Rust 或 C 代码。
