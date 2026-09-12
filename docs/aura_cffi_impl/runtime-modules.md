# 运行库模块源码

> 本文件包含运行库各模块的完整 Aura 源码实现。

---

## 1. Syscalls.aura — 系统调用声明

```aura
package aura.lang.runtime

// x86_64 Linux 系统调用（syscall 号来自 arch/x86/entry/syscalls_64.tbl）
extern object Syscalls {
    @native(0)   fun read(fd: Int, buf: Long, count: Long): Long
    @native(1)   fun write(fd: Int, buf: Long, count: Long): Long
    @native(2)   fun open(path: CString, flags: Int): Int
    @native(3)   fun close(fd: Int): Int
    @native(5)   fun fstat(fd: Int, buf: Long): Long
    @native(6)   fun lseek(fd: Int, off: Long, whence: Int): Long
    @native(9)   fun mmap(addr: Long, len: Long, prot: Int, flags: Int, fd: Int, off: Long): Long
    @native(10)  fun munmap(addr: Long, len: Long): Long
    @native(21)  fun access(path: CString, mode: Int): Int
    @native(39)  fun unlink(path: CString): Int
    @native(59)  fun execve(path: CString, args: Long, env: Long): Long
    @native(60)  fun exitGroup(code: Int)
    @native(61)  fun wait4(pid: Int, status: Long, options: Int, rusage: Long): Int
    @native(228) fun clockGettime(clock: Int, ts: Long): Int
    @native(272) fun getrandom(buf: Long, len: Long, flags: Int): Long
}
```

---

## 2. Memory.aura — 内存操作

```aura
package aura.lang.runtime

// 编译器内置内存操作（直接降低到 LLVM load/store）
extern object Memory {
    @native fun read(addr: Long): Byte
    @native fun read16(addr: Long): Short
    @native fun read32(addr: Long): Int
    @native fun read64(addr: Long): Long
    @native fun write(addr: Long, v: Byte)
    @native fun write16(addr: Long, v: Short)
    @native fun write32(addr: Long, v: Int)
    @native fun write64(addr: Long, v: Long)
    @native fun copy(dst: Long, src: Long, n: Long)
    @native fun set(addr: Long, v: Byte, n: Long)
    @native fun alloc(n: Long): Long
    @native fun free(addr: Long)
}
```

---

## 3. Cpu.aura — CPU 级操作

```aura
package aura.lang.runtime

// CPU 级操作（内联汇编）
extern object Cpu {
    @native(asm = "rdtsc") fun rdtsc(): Long
    @native(asm = "mfence") fun memFence()
    @native(asm = "cpuid") fun cpuid(level: Int): Long
    @native(asm = "lock xaddq") fun atomicAdd(addr: Long, delta: Long): Long
}
```

---

## 4. memory/Allocator.aura — 内存分配器

```aura
package aura.lang.runtime.memory
import aura.lang.runtime.Syscalls
import aura.lang.runtime.Memory

object Allocator {
    var HEAP_SIZE: Long = 1024 * 1024  // 1 MB
    var HEAP: Long = Syscalls.mmap(0, HEAP_SIZE, 3, 0x22, -1, 0)
    var HEAP_POS: Long = 0
    var HEAP_USED: Long = 0

    @export fun malloc(n: Long): Long {
        val aligned: Long = (n + 7) & (~7L)  // 8 字节对齐
        if (HEAP_POS + aligned > HEAP_SIZE) {
            expandHeap()
        }
        val p: Long = HEAP + HEAP_POS
        HEAP_POS = HEAP_POS + aligned
        HEAP_USED = HEAP_USED + aligned
        return p
    }

    @export fun free(addr: Long) {
        // bump allocator：不释放
    }

    @export fun resetHeap() {
        HEAP_POS = 0
        HEAP_USED = 0
    }

    fun expandHeap() {
        val newHeap: Long = Syscalls.mmap(HEAP + HEAP_SIZE, HEAP_SIZE, 3, 0x22, -1, 0)
        if (newHeap == -1L) {
            Syscalls.exitGroup(1)
        }
        HEAP_SIZE = HEAP_SIZE * 2
    }
}
```

---

## 5. string/StrOps.aura — 字符串操作

```aura
package aura.lang.runtime.string
import aura.lang.runtime.Memory
import aura.lang.runtime.memory.Allocator

object StrOps {
    @export fun strlen(s: CString): Long {
        val sAddr: Long = s as Long
        if (sAddr == 0) { return 0 }
        var i: Long = 0
        var c: Byte = Memory.read(sAddr + i)
        while (c != 0) {
            i = i + 1
            c = Memory.read(sAddr + i)
        }
        return i
    }

    @export fun strcmp(a: CString, b: CString): Int {
        if (a == b) { return 0 }
        var i: Long = 0
        while (true) {
            val ca: Byte = Memory.read(a as Long + i)
            val cb: Byte = Memory.read(b as Long + i)
            if (ca == 0 && cb == 0) { return 0 }
            if (ca != cb) { return ca - cb }
            i = i + 1
        }
    }

    @export fun strcpy(dst: Long, src: Long): Long {
        var i: Long = 0
        var c: Byte = Memory.read(src + i)
        while (c != 0) {
            Memory.write(dst + i, c)
            i = i + 1
            c = Memory.read(src + i)
        }
        Memory.write(dst + i, 0)
        return dst
    }

    @export fun memCopy(dst: Long, src: Long, n: Long) {
        var i: Long = 0
        while (i < n) {
            Memory.write(dst + i, Memory.read(src + i))
            i = i + 1
        }
    }

    @export fun memSet(addr: Long, v: Byte, n: Long) {
        var i: Long = 0
        while (i < n) {
            Memory.write(addr + i, v)
            i = i + 1
        }
    }

    @export fun stringConcat(a: String, b: String): String {
        val aAddr: Long = a as Long
        val bAddr: Long = b as Long
        val la: Long = strlen(aAddr as CString)
        val lb: Long = strlen(bAddr as CString)
        val buf: Long = Allocator.malloc(la + lb + 1)
        memCopy(buf, aAddr, la)
        memCopy(buf + la, bAddr, lb + 1)
        return buf as String
    }
}
```

---

## 6. console/Console.aura — 控制台输出

```aura
package aura.lang.runtime.console
import aura.lang.runtime.Syscalls
import aura.lang.runtime.string.StrOps

object Console {
    @export fun println(s: String) {
        val sAddr: Long = s as Long
        val n: Long = StrOps.strlen(sAddr as CString)
        Syscalls.write(1, sAddr, n)
        Syscalls.write(1, "\n" as Long, 1)
    }

    @export fun print(s: String) {
        val sAddr: Long = s as Long
        val n: Long = StrOps.strlen(sAddr as CString)
        Syscalls.write(1, sAddr, n)
    }
}
```

---

## 7. math/MathCore.aura — 数学函数

```aura
package aura.lang.runtime.math

object MathCore {
    @export fun sqrt(x: Double): Double {
        var g: Double = x / 2.0
        var i: Int = 0
        while (i < 32) {
            g = (g + x / g) / 2.0
            i = i + 1
        }
        return g
    }

    @export fun sin(x: Double): Double {
        var term: Double = x
        var sum: Double = x
        var n: Int = 1
        var i: Int = 1
        while (i < 20) {
            n = n * 2 * (2 * i + 1)
            term = -term * x * x / (n as Double)
            sum = sum + term
            i = i + 1
        }
        return sum
    }

    @export fun cos(x: Double): Double {
        var term: Double = 1.0
        var sum: Double = 1.0
        var n: Int = 1
        var i: Int = 1
        while (i < 20) {
            n = n * 2 * (2 * i)
            term = -term * x * x / (n as Double)
            sum = sum + term
            i = i + 1
        }
        return sum
    }

    @export fun exp(x: Double): Double {
        var term: Double = 1.0
        var sum: Double = 1.0
        var n: Int = 1
        var i: Int = 1
        while (i < 30) {
            n = n * i
            term = term * x / (n as Double)
            sum = sum + term
            i = i + 1
        }
        return sum
    }

    @export fun ln(x: Double): Double {
        val z: Double = (x - 1.0) / (x + 1.0)
        var term: Double = z
        var sum: Double = z
        var n: Int = 1
        var i: Int = 1
        while (i < 50) {
            n = 2 * i + 1
            term = term * z * z
            sum = sum + term / (n as Double)
            i = i + 1
        }
        return 2.0 * sum
    }

    @export fun pow(x: Double, y: Double): Double {
        return exp(y * ln(x))
    }
}
```

---

## 8. random/XorShift.aura — 随机数

```aura
package aura.lang.runtime.random

object XorShift {
    var state: Long = 0xDEADBEEFCAFE1234L

    @export fun rand(): Int {
        var s: Long = state
        s = s ^ (s >> 12)
        s = s ^ (s << 25)
        s = s ^ (s >> 27)
        state = s
        return (s * 2685821657736338717L) as Int
    }

    @export fun srand(seed: Long) {
        state = seed | 1L
    }
}
```

---

## 9. time/Clock.aura — 时间

```aura
package aura.lang.runtime.time
import aura.lang.runtime.Syscalls
import aura.lang.runtime.Memory
import aura.lang.runtime.memory.Allocator

object Clock {
    @export fun time(): Long {
        val ts: Long = Allocator.malloc(16)
        Syscalls.clockGettime(0, ts)
        val sec: Long = Memory.read64(ts)
        Allocator.free(ts)
        return sec
    }

    @export fun millis(): Long {
        val ts: Long = Allocator.malloc(16)
        Syscalls.clockGettime(1, ts)
        val sec: Long = Memory.read64(ts)
        val nsec: Long = Memory.read64(ts + 8)
        Allocator.free(ts)
        return sec * 1000 + nsec / 1000000
    }
}
```

---

## 10. file/FileOps.aura — 文件 I/O

```aura
package aura.lang.runtime.file
import aura.lang.runtime.Syscalls
import aura.lang.runtime.memory.Allocator

object FileOps {
    @export fun read(path: CString): Long {
        val fd: Int = Syscalls.open(path, 0)
        if (fd < 0) { return -1 }
        val buf: Long = Allocator.malloc(1048576)
        var total: Long = 0
        while (true) {
            val n: Long = Syscalls.read(fd, buf + total, 1048576 - total)
            if (n <= 0) { break }
            total = total + n
        }
        Syscalls.close(fd)
        return buf
    }

    @export fun write(path: CString, data: Long, len: Long): Int {
        val fd: Int = Syscalls.open(path, 0x241)
        if (fd < 0) { return -1 }
        val r: Long = Syscalls.write(fd, data, len)
        Syscalls.close(fd)
        return r as Int
    }

    @export fun exists(path: CString): Boolean {
        val fd: Int = Syscalls.open(path, 0)
        if (fd < 0) { return false }
        Syscalls.close(fd)
        return true
    }
}
```

---

## 11. process/ProcessOps.aura — 进程管理

```aura
package aura.lang.runtime.process
import aura.lang.runtime.Syscalls
import aura.lang.runtime.Memory
import aura.lang.runtime.memory.Allocator

object ProcessOps {
    @export fun run(cmd: String): Int {
        val args: Long = Allocator.malloc(3 * 8)
        Memory.write64(args, "/bin/sh" as Long)
        Memory.write64(args + 8, "-c" as Long)
        Memory.write64(args + 16, cmd as Long)
        Syscalls.execve("/bin/sh" as Long, args, 0)
        return -1
    }

    @export fun exit(code: Int) {
        Syscalls.exitGroup(code)
    }
}
```

---

## 12. boxed/PlanA.aura — Plan A 装箱

```aura
package aura.lang.runtime.boxed
import aura.lang.runtime.Memory

object PlanA {
    @export fun intToAny(v: Long): Long {
        return (v << 1) | 1
    }

    @export fun toIntAny(v: Long): Long {
        return v >> 1
    }

    @export fun toStrAny(v: Long): Long {
        return Memory.read64(v)
    }

    @export fun isNullable(v: Long): Boolean {
        return v == 0
    }

    @export fun isInt(v: Long): Boolean {
        return (v & 1) == 1
    }
}
```

---

## 13. Runtime.aura — 入口聚合

```aura
package aura.lang.runtime

// 聚合所有模块
import aura.lang.runtime.Syscalls
import aura.lang.runtime.Memory
import aura.lang.runtime.memory.Allocator
import aura.lang.runtime.string.StrOps
import aura.lang.runtime.console.Console
import aura.lang.runtime.math.MathCore
import aura.lang.runtime.random.XorShift
import aura.lang.runtime.time.Clock
import aura.lang.runtime.file.FileOps
import aura.lang.runtime.process.ProcessOps
import aura.lang.runtime.boxed.PlanA
```

---

## 14. 跨平台 Syscalls 示例

### x86_64_windows/Syscalls.aura

```aura
package aura.lang.runtime.arch.x86_64_windows

// Windows syscall 号（来自 ntoskrnl）
extern object Syscalls {
    @native("libc:NtWriteVirtualMemory") fun write(fd: Int, buf: Long, len: Long): Long
    @native("libc:NtReadVirtualMemory") fun read(fd: Int, buf: Long, len: Long): Long
    @native("libc:NtCreateFile") fun open(path: CString, flags: Int): Int
    @native("libc:NtClose") fun close(fd: Int): Int
    @native("libc:NtAllocateVirtualMemory") fun mmap(addr: Long, len: Long, prot: Int, flags: Int, fd: Int, off: Long): Long
    @native("libc:NtFreeVirtualMemory") fun munmap(addr: Long, len: Long): Long
    @native("libc:ExitProcess") fun exitGroup(code: Int)
    @native("libc:NtQuerySystemTime") fun clockGettime(clock: Int, ts: Long): Int
}
```

### aarch64_linux/Syscalls.aura

```aura
package aura.lang.runtime.arch.aarch64_linux

// aarch64 Linux syscall 号
extern object Syscalls {
    @native(63)  fun read(fd: Int, buf: Long, count: Long): Long
    @native(64)  fun write(fd: Int, buf: Long, count: Long): Long
    @native(56)  fun open(path: CString, flags: Int): Int
    @native(57)  fun close(fd: Int): Int
    @native(28)  fun mmap(addr: Long, len: Long, prot: Int, flags: Int, fd: Int, off: Long): Long
    @native(29)  fun munmap(addr: Long, len: Long): Long
    @native(94)  fun execve(path: CString, args: Long, env: Long): Long
    @native(93)  fun exitGroup(code: Int)
    @native(403) fun clockGettime(clock: Int, ts: Long): Int
}
```
