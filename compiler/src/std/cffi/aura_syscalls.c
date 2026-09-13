/**
 * Aura Syscalls — 系统调用分发层（Layer 0-B 扩展）
 *
 * 为 Phase D 的 @native(N) 注解提供底层 syscall 分发。
 * 每个函数对应一个 Aura Syscalls.aura 中的 @native(SYSCALL_NUMBER) 声明。
 *
 * 平台支持：
 *   - Linux x86_64 / aarch64: 通过 syscall() 或 Nt* API
 *   - Windows x86_64 / aarch64: 通过 Nt* API 或内核态等价操作
 *
 * 编译：
 *   clang -c aura_syscalls.c -o aura_syscalls.o
 *   链接到 AOT 输出
 */

#ifndef AURA_SYSCALLS_C
#define AURA_SYSCALLS_C

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ────────────────────────────────────────────────────────────────────────────
 * 平台检测
 * ──────────────────────────────────────────────────────────────────────────── */

#if defined(_WIN32)
  #define AURA_PLATFORM_WINDOWS 1
#elif defined(__linux__)
  #define AURA_PLATFORM_LINUX 1
#elif defined(__APPLE__)
  #define AURA_PLATFORM_MACOS 1
#endif

#if defined(__x86_64__) || defined(_M_X64)
  #define AURA_ARCH_X86_64 1
#elif defined(__aarch64__) || defined(_M_ARM64)
  #define AURA_ARCH_AARCH64 1
#endif

/* ────────────────────────────────────────────────────────────────────────────
 * 通用 syscall 分发（x86_64 Linux）
 * ──────────────────────────────────────────────────────────────────────────── */

#if defined(AURA_PLATFORM_LINUX) && defined(AURA_ARCH_X86_64)

#include <sys/syscall.h>

/** 通用 syscall 分发器 */
static inline int64_t aura_syscall(int64_t nr, ...) {
    register long r10 __asm__("r10") = 0;
    register long r8  __asm__("r8")  = 0;
    register long r9  __asm__("r9")  = 0;
    register long rsi __asm__("rsi") = 0;
    register long rdx __asm__("rdx") = 0;
    register long rcx __asm__("rcx") = 0;
    register long rax __asm__("rax") = nr;

    __asm__ volatile (
        "syscall"
        : "+a"(rax)
        : "r"(rsi), "r"(rdx), "r"(r10), "r"(r8), "r"(r9)
        : "rcx", "memory"
    );
    return rax;
}

#else /* Windows or other */

/* Windows: 使用 Nt* API 或封装内核函数 */
static inline int64_t aura_syscall(int64_t nr, ...) {
    (void)nr;
    return -1; /* 不支持的平台 */
}

#endif

/* ────────────────────────────────────────────────────────────────────────────
 * 系统调用号（x86_64 Linux）
 * ──────────────────────────────────────────────────────────────────────────── */

#define SYS_READ           0
#define SYS_WRITE          1
#define SYS_OPEN           2
#define SYS_CLOSE          3
#define SYS_FSTAT          5
#define SYS_LSEEK          6
#define SYS_MMAP           9
#define SYS_MUNMAP         10
#define SYS_ACCESS         21
#define SYS_UNLINK         39
#define SYS_EXECVE         59
#define SYS_EXIT_GROUP     60
#define SYS_WAIT4          61
#define SYS_CLOCK_GETTIME  228
#define SYS_GETRANDOM      272
#define SYS_READV          62
#define SYS_WRITEV         63
#define SYS_PIPE           32

/* ────────────────────────────────────────────────────────────────────────────
 * Syscalls.aura 导出函数
 * ──────────────────────────────────────────────────────────────────────────── */

/** Syscalls.read(fd, buf, count) */
int64_t aura_syscall_read(int64_t fd, int64_t buf, int64_t count) {
    return aura_syscall(SYS_READ, fd, buf, count);
}

/** Syscalls.write(fd, buf, count) */
int64_t aura_syscall_write(int64_t fd, int64_t buf, int64_t count) {
    return aura_syscall(SYS_WRITE, fd, buf, count);
}

/** Syscalls.open(path, flags) */
int64_t aura_syscall_open(int64_t path, int64_t flags) {
    return aura_syscall(SYS_OPEN, path, flags);
}

/** Syscalls.close(fd) */
int64_t aura_syscall_close(int64_t fd) {
    return aura_syscall(SYS_CLOSE, fd);
}

/** Syscalls.fstat(fd, buf) */
int64_t aura_syscall_fstat(int64_t fd, int64_t buf) {
    return aura_syscall(SYS_FSTAT, fd, buf);
}

/** Syscalls.lseek(fd, off, whence) */
int64_t aura_syscall_lseek(int64_t fd, int64_t off, int64_t whence) {
    return aura_syscall(SYS_LSEEK, fd, off, whence);
}

/** Syscalls.mmap(addr, len, prot, flags, fd, off) */
int64_t aura_syscall_mmap(int64_t addr, int64_t len, int64_t prot,
                          int64_t flags, int64_t fd, int64_t off) {
    return aura_syscall(SYS_MMAP, addr, len, prot, flags, fd, off);
}

/** Syscalls.munmap(addr, len) */
int64_t aura_syscall_munmap(int64_t addr, int64_t len) {
    return aura_syscall(SYS_MUNMAP, addr, len);
}

/** Syscalls.access(path, mode) */
int64_t aura_syscall_access(int64_t path, int64_t mode) {
    return aura_syscall(SYS_ACCESS, path, mode);
}

/** Syscalls.unlink(path) */
int64_t aura_syscall_unlink(int64_t path) {
    return aura_syscall(SYS_UNLINK, path);
}

/** Syscalls.execve(path, args, env) */
int64_t aura_syscall_execve(int64_t path, int64_t args, int64_t env) {
    return aura_syscall(SYS_EXECVE, path, args, env);
}

/** Syscalls.exitGroup(code) */
void aura_syscall_exit_group(int64_t code) {
#if defined(AURA_PLATFORM_LINUX)
    aura_syscall(SYS_EXIT_GROUP, code);
    /* 不会返回 */
#else
    (void)code;
    __builtin_exit((int)code);
#endif
}

/** Syscalls.wait4(pid, status, options, rusage) */
int64_t aura_syscall_wait4(int64_t pid, int64_t status,
                           int64_t options, int64_t rusage) {
    return aura_syscall(SYS_WAIT4, pid, status, options, rusage);
}

/** Syscalls.clockGettime(clock, ts) */
int64_t aura_syscall_clock_gettime(int64_t clock, int64_t ts) {
    return aura_syscall(SYS_CLOCK_GETTIME, clock, ts);
}

/** Syscalls.getrandom(buf, len, flags) */
int64_t aura_syscall_getrandom(int64_t buf, int64_t len, int64_t flags) {
    return aura_syscall(SYS_GETRANDOM, buf, len, flags);
}

/** Syscalls.readv(fd, iov, iovcnt) */
int64_t aura_syscall_readv(int64_t fd, int64_t iov, int64_t iovcnt) {
    return aura_syscall(SYS_READV, fd, iov, iovcnt);
}

/** Syscalls.writev(fd, iov, iovcnt) */
int64_t aura_syscall_writev(int64_t fd, int64_t iov, int64_t iovcnt) {
    return aura_syscall(SYS_WRITEV, fd, iov, iovcnt);
}

/** Syscalls.pipe(pipes) */
int64_t aura_syscall_pipe(int64_t pipes) {
    return aura_syscall(SYS_PIPE, pipes);
}

/* ────────────────────────────────────────────────────────────────────────────
 * 通用 syscall 分发入口（供 AOT 发射器使用）
 * ──────────────────────────────────────────────────────────────────────────── */

/** 按 syscall 号分发：call i64 @aura_syscall_dispatch(i64 N, ...) */
int64_t aura_syscall_dispatch(int64_t nr, int64_t a1, int64_t a2,
                              int64_t a3, int64_t a4, int64_t a5, int64_t a6) {
    switch (nr) {
        case SYS_READ:          return aura_syscall_read(a1, a2, a3);
        case SYS_WRITE:         return aura_syscall_write(a1, a2, a3);
        case SYS_OPEN:          return aura_syscall_open(a1, a2);
        case SYS_CLOSE:         return aura_syscall_close(a1);
        case SYS_FSTAT:         return aura_syscall_fstat(a1, a2);
        case SYS_LSEEK:         return aura_syscall_lseek(a1, a2, a3);
        case SYS_MMAP:          return aura_syscall_mmap(a1, a2, a3, a4, a5, a6);
        case SYS_MUNMAP:        return aura_syscall_munmap(a1, a2);
        case SYS_ACCESS:        return aura_syscall_access(a1, a2);
        case SYS_UNLINK:        return aura_syscall_unlink(a1);
        case SYS_EXECVE:        return aura_syscall_execve(a1, a2, a3);
        case SYS_WAIT4:         return aura_syscall_wait4(a1, a2, a3, a4);
        case SYS_CLOCK_GETTIME: return aura_syscall_clock_gettime(a1, a2);
        case SYS_GETRANDOM:     return aura_syscall_getrandom(a1, a2, a3);
        case SYS_READV:         return aura_syscall_readv(a1, a2, a3);
        case SYS_WRITEV:        return aura_syscall_writev(a1, a2, a3);
        case SYS_PIPE:          return aura_syscall_pipe(a1);
        default:                return -1;
    }
}

/* ────────────────────────────────────────────────────────────────────────────
 * Memory.aura 内置指令（编译器内置，非 syscall）
 * ──────────────────────────────────────────────────────────────────────────── */

/** Memory.alloc(n) — 通过 mmap 分配 */
int64_t aura_memory_alloc(int64_t n) {
    return aura_syscall_mmap(0, n, 3 /* RW */, 0x22 /* PRIVATE|ANON */, -1, 0);
}

/** Memory.free(addr) — 通过 munmap 释放 */
void aura_memory_free(int64_t addr) {
    /* munmap 需要长度参数，此处无法获知，使用固定页大小 4096 */
    aura_syscall_munmap(addr, 4096);
}

/* ────────────────────────────────────────────────────────────────────────────
 * Cpu.aura 内联汇编（x86_64）
 * ──────────────────────────────────────────────────────────────────────────── */

#if defined(AURA_PLATFORM_LINUX) && defined(AURA_ARCH_X86_64)

/** Cpu.rdtsc() — 读取时间戳计数器 */
int64_t aura_cpu_rdtsc(void) {
    unsigned int lo, hi;
    __asm__ volatile ("rdtsc" : "=a"(lo), "=d"(hi));
    return ((int64_t)hi << 32) | lo;
}

/** Cpu.memFence() — 内存屏障 */
void aura_cpu_mem_fence(void) {
    __asm__ volatile ("mfence" ::: "memory");
}

/** Cpu.atomicAdd(addr, delta) — 原子加法（返回旧值） */
int64_t aura_cpu_atomic_add(int64_t addr, int64_t delta) {
    int64_t old_val;
    __asm__ volatile (
        "lock xaddq %0, (%1)"
        : "=&r"(old_val)
        : "r"(addr), "r"(delta)
        : "memory"
    );
    return old_val;
}

#else

int64_t aura_cpu_rdtsc(void) { return 0; }
void aura_cpu_mem_fence(void) {}
int64_t aura_cpu_atomic_add(int64_t addr, int64_t delta) {
    (void)addr; (void)delta; return 0;
}

#endif

#ifdef __cplusplus
}
#endif

#endif /* AURA_SYSCALLS_C */
