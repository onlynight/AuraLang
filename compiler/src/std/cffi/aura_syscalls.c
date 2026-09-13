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
#include <stdlib.h>
#include <string.h>
#include <setjmp.h>

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
    exit((int)code);
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
#if defined(AURA_PLATFORM_LINUX) || defined(AURA_PLATFORM_MACOS)
    return aura_syscall_mmap(0, n, 3 /* RW */, 0x22 /* PRIVATE|ANON */, -1, 0);
#else
    /* Windows 等无 mmap 的平台：退回 CRT malloc（与 aura_memory_free 对偶）。
       旧实现直接走 aura_syscall 桩，恒返回 -1，导致 Aura 侧 `native fun
       MemoryAlloc` 在 Windows 上完全不可用（AOT 产物一写入即崩溃）。 */
    return (int64_t)(uintptr_t)malloc((size_t)n);
#endif
}

/** Memory.free(addr) — 通过 munmap 释放 */
void aura_memory_free(int64_t addr) {
#if defined(AURA_PLATFORM_LINUX) || defined(AURA_PLATFORM_MACOS)
    /* munmap 需要长度参数，此处无法获知，使用固定页大小 4096 */
    aura_syscall_munmap(addr, 4096);
#else
    if (addr != 0) {
        free((void *)(uintptr_t)addr);
    }
#endif
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

/* ────────────────────────────────────────────────────────────────────────────
 * Phase D.2 P3: 并发运行时（pthread / Win32 线程原语）
 * ──────────────────────────────────────────────────────────────────────────── */

#if defined(AURA_PLATFORM_LINUX) || defined(AURA_PLATFORM_MACOS)

#include <pthread.h>

/** Thread.create(fn, arg) — 创建新线程，返回线程句柄 */
int64_t aura_thread_create(int64_t (*fn)(void *), int64_t arg) {
    pthread_t tid;
    if (pthread_create(&tid, NULL, (void *(*)(void *))fn, (void *)arg) != 0) {
        return -1;
    }
    return (int64_t)(uintptr_t)tid;
}

/** Thread.join(id) — 等待线程结束 */
void aura_thread_join(int64_t id) {
    if (id <= 0) return;
    pthread_join((pthread_t)(uintptr_t)id, NULL);
}

/** Mutex.new() — 创建互斥锁，返回句柄 */
int64_t aura_mutex_new(void) {
    pthread_mutex_t *mtx = malloc(sizeof(pthread_mutex_t));
    if (mtx) pthread_mutex_init(mtx, NULL);
    return (int64_t)(uintptr_t)mtx;
}

/** Mutex.lock(id) — 加锁 */
void aura_mutex_lock(int64_t id) {
    if (id > 0) pthread_mutex_lock((pthread_mutex_t *)(uintptr_t)id);
}

/** Mutex.unlock(id) — 解锁 */
void aura_mutex_unlock(int64_t id) {
    if (id > 0) pthread_mutex_unlock((pthread_mutex_t *)(uintptr_t)id);
}

/** Mutex.destroy(id) — 销毁互斥锁 */
void aura_mutex_destroy(int64_t id) {
    if (id > 0) {
        pthread_mutex_t *mtx = (pthread_mutex_t *)(uintptr_t)id;
        pthread_mutex_destroy(mtx);
        free(mtx);
    }
}

/** CondVar.new() — 创建条件变量，返回句柄 */
int64_t aura_condvar_new(void) {
    pthread_cond_t *cv = malloc(sizeof(pthread_cond_t));
    if (cv) pthread_cond_init(cv, NULL);
    return (int64_t)(uintptr_t)cv;
}

/** CondVar.wait(id, mutexId) — 等待条件变量（自动解锁 mutex） */
void aura_condvar_wait(int64_t id, int64_t mutexId) {
    if (id > 0 && mutexId > 0) {
        pthread_cond_wait((pthread_cond_t *)(uintptr_t)id,
                          (pthread_mutex_t *)(uintptr_t)mutexId);
    }
}

/** CondVar.signal(id) — 唤醒一个等待者 */
void aura_condvar_signal(int64_t id) {
    if (id > 0) pthread_cond_signal((pthread_cond_t *)(uintptr_t)id);
}

/** CondVar.broadcast(id) — 唤醒所有等待者 */
void aura_condvar_broadcast(int64_t id) {
    if (id > 0) pthread_cond_broadcast((pthread_cond_t *)(uintptr_t)id);
}

/** CondVar.destroy(id) — 销毁条件变量 */
void aura_condvar_destroy(int64_t id) {
    if (id > 0) {
        pthread_cond_t *cv = (pthread_cond_t *)(uintptr_t)id;
        pthread_cond_destroy(cv);
        free(cv);
    }
}

#elif defined(_WIN32)

#include <windows.h>

typedef DWORD (*ThreadFn)(LPVOID);

/** Thread.create(fn, arg) — 创建新线程（Win32） */
int64_t aura_thread_create(int64_t (*fn)(void *), int64_t arg) {
    HANDLE h = CreateThread(NULL, 0, (LPTHREAD_START_ROUTINE)fn, (LPVOID)arg, 0, NULL);
    if (h == NULL) return -1;
    return (int64_t)(uintptr_t)h;
}

/** Thread.join(id) — 等待线程结束 */
void aura_thread_join(int64_t id) {
    if (id <= 0) return;
    WaitForSingleObject((HANDLE)(uintptr_t)id, INFINITE);
    CloseHandle((HANDLE)(uintptr_t)id);
}

/** Mutex.new() — 创建临界区 */
int64_t aura_mutex_new(void) {
    CRITICAL_SECTION *cs = malloc(sizeof(CRITICAL_SECTION));
    if (cs) InitializeCriticalSection(cs);
    return (int64_t)(uintptr_t)cs;
}

/** Mutex.lock(id) — 进入临界区 */
void aura_mutex_lock(int64_t id) {
    if (id > 0) EnterCriticalSection((CRITICAL_SECTION *)(uintptr_t)id);
}

/** Mutex.unlock(id) — 离开临界区 */
void aura_mutex_unlock(int64_t id) {
    if (id > 0) LeaveCriticalSection((CRITICAL_SECTION *)(uintptr_t)id);
}

/** Mutex.destroy(id) — 删除临界区 */
void aura_mutex_destroy(int64_t id) {
    if (id > 0) {
        CRITICAL_SECTION *cs = (CRITICAL_SECTION *)(uintptr_t)id;
        DeleteCriticalSection(cs);
        free(cs);
    }
}

/** CondVar.new() — 创建事件对象（简化实现） */
int64_t aura_condvar_new(void) {
    HANDLE ev = CreateEvent(NULL, FALSE, FALSE, NULL);
    return (int64_t)(uintptr_t)ev;
}

/** CondVar.wait(id, mutexId) — 等待事件（Win32 简化实现） */
void aura_condvar_wait(int64_t id, int64_t mutexId) {
    if (mutexId > 0) LeaveCriticalSection((CRITICAL_SECTION *)(uintptr_t)mutexId);
    if (id > 0) WaitForSingleObject((HANDLE)(uintptr_t)id, INFINITE);
    if (mutexId > 0) EnterCriticalSection((CRITICAL_SECTION *)(uintptr_t)mutexId);
}

/** CondVar.signal(id) — 设置事件 */
void aura_condvar_signal(int64_t id) {
    if (id > 0) SetEvent((HANDLE)(uintptr_t)id);
}

/** CondVar.broadcast(id) — 设置事件（Win32 等价） */
void aura_condvar_broadcast(int64_t id) {
    if (id > 0) SetEvent((HANDLE)(uintptr_t)id);
}

/** CondVar.destroy(id) — 关闭事件 */
void aura_condvar_destroy(int64_t id) {
    if (id > 0) CloseHandle((HANDLE)(uintptr_t)id);
}

#else

/* 非 Linux/macOS/Windows 平台：返回空实现 */
int64_t aura_thread_create(int64_t (*fn)(void *), int64_t arg) {
    (void)fn; (void)arg; return -1;
}
void aura_thread_join(int64_t id) { (void)id; }
int64_t aura_mutex_new(void) { return 0; }
void aura_mutex_lock(int64_t id) { (void)id; }
void aura_mutex_unlock(int64_t id) { (void)id; }
void aura_mutex_destroy(int64_t id) { (void)id; }
int64_t aura_condvar_new(void) { return 0; }
void aura_condvar_wait(int64_t id, int64_t mutexId) { (void)id; (void)mutexId; }
void aura_condvar_signal(int64_t id) { (void)id; }
void aura_condvar_broadcast(int64_t id) { (void)id; }
void aura_condvar_destroy(int64_t id) { (void)id; }

#endif /* pthread / Win32 */

/* ────────────────────────────────────────────────────────────────────────────
 * Phase D.2 P4: SHA256 密码学哈希（C 层实现，Aura 侧 @native 调用）
 * ──────────────────────────────────────────────────────────────────────────── */

#include <stdio.h>

/* SHA256 轮常量 K[0..63] */
static const uint32_t sha256_k[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
};

static inline uint32_t rotr32(uint32_t x, unsigned n) {
    return (x >> n) | (x << (32 - n));
}

/* SHA256 压缩函数：处理 512 位块 */
static void sha256_compress(uint32_t h[8], const uint8_t block[64]) {
    uint32_t w[64];
    for (int i = 0; i < 16; i++) {
        w[i] = ((uint32_t)block[i*4] << 24) |
               ((uint32_t)block[i*4+1] << 16) |
               ((uint32_t)block[i*4+2] << 8) |
               ((uint32_t)block[i*4+3]);
    }
    for (int i = 16; i < 64; i++) {
        uint32_t s0 = rotr32(w[i-15], 7) ^ rotr32(w[i-15], 18) ^ (w[i-15] >> 3);
        uint32_t s1 = rotr32(w[i-2], 17) ^ rotr32(w[i-2], 19) ^ (w[i-2] >> 10);
        w[i] = w[i-16] + s0 + w[i-7] + s1;
    }

    uint32_t a = h[0], b = h[1], c = h[2], d = h[3];
    uint32_t e = h[4], f = h[5], g = h[6], hh = h[7];

    for (int i = 0; i < 64; i++) {
        uint32_t S1 = rotr32(e, 6) ^ rotr32(e, 11) ^ rotr32(e, 25);
        uint32_t ch = (e & f) ^ (~e & g);
        uint32_t temp1 = hh + S1 + ch + sha256_k[i] + w[i];
        uint32_t S0 = rotr32(a, 2) ^ rotr32(a, 13) ^ rotr32(a, 22);
        uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
        uint32_t temp2 = S0 + maj;
        hh = g; g = f; f = e; e = d + temp1;
        d = c; c = b; b = a; a = temp1 + temp2;
    }

    h[0] += a; h[1] += b; h[2] += c; h[3] += d;
    h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
}

/**
 * aura_sha256(text, out) — 计算 SHA256，结果写入 out（64 字节十六进制字符串）
 * 返回写入的字节数（不含 NUL）
 */
int aura_sha256(const char *text, char *out) {
    uint32_t h[8] = {
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19
    };

    int orig_len = (int)strlen(text);
    uint64_t bit_len = (uint64_t)orig_len * 8;

    /* 计算 padding 长度 */
    int pad_len = (56 - ((orig_len + 1) % 64)) % 64 + 64;
    int total_len = orig_len + 1 + pad_len;

    /* 分配 padding 缓冲区 */
    uint8_t *padded = malloc(total_len);
    if (!padded) return -1;
    memset(padded, 0, total_len);

    /* 复制原始数据 */
    memcpy(padded, text, orig_len);
    padded[orig_len] = 0x80;

    /* 写入 64 位长度（大端序） */
    for (int i = 0; i < 8; i++) {
        padded[orig_len + 1 + pad_len - 8 + i] = (bit_len >> (56 - i * 8)) & 0xFF;
    }

    /* 处理每个 512 位块 */
    for (int off = 0; off < total_len; off += 64) {
        sha256_compress(h, (const uint8_t *)(padded + off));
    }

    free(padded);

    /* 输出 32 字节哈希（大端序十六进制） */
    int out_len = 0;
    for (int i = 0; i < 8; i++) {
        out[out_len++] = (char)('0' + (h[i] >> 28) & 0xF);
        out[out_len++] = (char)('0' + (h[i] >> 24) & 0xF);
        out[out_len++] = (char)('0' + (h[i] >> 20) & 0xF);
        out[out_len++] = (char)('0' + (h[i] >> 16) & 0xF);
        out[out_len++] = (char)('0' + (h[i] >> 12) & 0xF);
        out[out_len++] = (char)('0' + (h[i] >> 8) & 0xF);
        out[out_len++] = (char)('0' + (h[i] >> 4) & 0xF);
        out[out_len++] = (char)('0' + (h[i] & 0xF));
    }
    return out_len;
}

/* ────────────────────────────────────────────────────────────────────────────
 * Phase D: setjmp/longjmp 异常桥（try/catch）
 * ──────────────────────────────────────────────────────────────────────────── */

#include <setjmp.h>

/** 全局异常值（longjmp 只能传递一个 int，异常对象通过此全局变量传递） */
void *aura_exception_value = NULL;

/** jmp_buf 栈（支持嵌套 try/catch） */
#define AURA_MAX_JMP_DEPTH 64
static jmp_buf aura_jmp_stack[AURA_MAX_JMP_DEPTH];
static int aura_jmp_depth = 0;

/** aura_setjmp(buf) — 保存当前执行上下文，返回 0（首次调用）或非零（longjmp 返回） */
int aura_setjmp(void *buf) {
    if (aura_jmp_depth < AURA_MAX_JMP_DEPTH) {
        memcpy(aura_jmp_stack[aura_jmp_depth], buf, sizeof(jmp_buf));
    }
    jmp_buf *jbp = (jmp_buf *)buf;
    return setjmp(*jbp);
}

/** aura_longjmp(buf, val) — 跳转到最近的 setjmp 上下文 */
void aura_longjmp(void *buf, int val) {
    (void)buf; /* 使用栈顶的 jmp_buf */
    if (aura_jmp_depth > 0) {
        longjmp(aura_jmp_stack[aura_jmp_depth - 1], val);
    }
    /* 无活跃 try 块时，打印异常并退出 */
    exit(1);
}

#ifdef __cplusplus
}
#endif

#endif /* AURA_SYSCALLS_C */
