/*
 * =============================================================================
 * aura_syscalls.c — Phase D: Syscall 分发层 & 并发运行时
 * =============================================================================
 *
 * AuraLang 编译器的 AOT (Ahead-of-Time) 后端支持。
 * 本文件为 @native 注解提供平台相关的系统调用实现，
 * 以及并发原语（Mutex / Atomic / RwLock / Condvar / Barrier / Thread）
 * 和运行时辅助函数（setjmp/longjmp 异常桥、argv 注入等）。
 *
 * 编译命令: clang -c aura_syscalls.c -o aura_syscalls.obj -I <cffi_header_dir>
 * 链接目标: 与 aura_std_cffi.o 一并链接到 AOT 可执行文件。
 *
 * 主要功能分区:
 *   1. Phase D: Syscall 分发层 (aura_syscall_*)
 *   2. Memory.aura 内置指令 (aura_memory_alloc/free)
 *   3. Cpu.aura 内联汇编 (aura_cpu_*)
 *   4. 并发原语 (aura_mutex_*, aura_atomic_*, aura_rwlock_*, etc.)
 *   5. 运行时支持 (aura_setjmp, aura_longjmp, aura_args_set, etc.)
 *
 * =============================================================================
 * Platform:
 *   - Windows x86_64 (MSVC/LLVM) — primary target, uses Win32 API
 *   - POSIX (Linux/macOS)         — fallback via #else branches
 * =============================================================================
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <setjmp.h>

#ifdef _WIN32
#  include <windows.h>
#  include <process.h>
#  include <intrin.h>
#  include <stdio.h>
#  include <errno.h>
#  pragma comment(lib, "crypt32.lib")
#  pragma comment(lib, "advapi32.lib")
#else
#  include <unistd.h>
#  include <fcntl.h>
#  include <sys/stat.h>
#  include <sys/mman.h>
#  include <sys/time.h>
#  include <sys/random.h>
#  include <sys/uio.h>
#  include <sys/wait.h>
#  include <pthread.h>
#  include <semaphore.h>
#  include <errno.h>
#  include <time.h>
#  include <sched.h>
#endif

/* =============================================================================
 * Forward declarations of the Aura header (for aura_syscall_* prototypes)
 * ============================================================================= */
#include "aura_std_cffi.h"

/* =============================================================================
 * Global state — referenced by aura_std_cffi.c and emit.rs
 * ============================================================================= */

/* 异常值全局变量 (Phase D: setjmp/longjmp 异常桥) */
/* 对应 LLVM IR: @aura_exception_value = external global i8* */
volatile void *aura_exception_value = NULL;

/* 命令行参数全局变量 — 由 aura_args_set 注入 */
int aura_argc_global = 0;
char **aura_argv_global = NULL;

/* =============================================================================
 * Platform-specific helper: fd ↔ HANDLE 转换
 *
 * 在 Windows 上，文件描述符 (fd) 与 HANDLE 需要通过
 * _get_osfhandle 互转。Aura 的 syscall 接口统一使用 int64_t fd，
 * 但底层 Win32 API 使用 HANDLE。
 * ============================================================================= */

#ifdef _WIN32

/* fd 到 HANDLE 的映射表（简易实现：fd 0,1,2 映射到标准流，其余用表） */
#define AURA_FD_MAX 64
static HANDLE aura_fd_to_handle[ AURA_FD_MAX ];

static HANDLE aura_get_std_handle(int64_t fd) {
    if (fd == 0) return GetStdHandle(STD_INPUT_HANDLE);
    if (fd == 1) return GetStdHandle(STD_OUTPUT_HANDLE);
    if (fd == 2) return GetStdHandle(STD_ERROR_HANDLE);
    /* 文件描述符映射表 */
    if (fd >= 0 && fd < AURA_FD_MAX) {
        return aura_fd_to_handle[fd];
    }
    return INVALID_HANDLE_VALUE;
}

static int64_t aura_handle_to_fd(HANDLE h) {
    for (int i = 0; i < AURA_FD_MAX; i++) {
        if (aura_fd_to_handle[i] == h) return i;
    }
    return -1;
}

#else /* POSIX: fd 直接就是整数 */

#define aura_get_std_handle(fd) ((void*)(fd))

#endif /* _WIN32 */

/* =============================================================================
 * Section 1: Phase D Syscall 分发层
 *
 * 每个 aura_syscall_* 函数通过 int64_t 参数接口调用平台 API。
 * 返回值约定: 0 = 成功，<0 = 错误 (POSIX 风格)。
 * ============================================================================= */

/* ---------------------------------------------------------------------------
 * aura_syscall_read(fd, buf, count)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_read(int64_t fd, int64_t buf, int64_t count) {
#ifdef _WIN32
    HANDLE h = aura_get_std_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -1;
    DWORD bytes_read = 0;
    if (!ReadFile(h, (void *)buf, (DWORD)count, &bytes_read, NULL)) {
        return -1;
    }
    return (int64_t)bytes_read;
#else
    return (int64_t)read((int)fd, (void *)buf, (size_t)count);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_write(fd, buf, count)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_write(int64_t fd, int64_t buf, int64_t count) {
#ifdef _WIN32
    HANDLE h = aura_get_std_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -1;
    DWORD bytes_written = 0;
    if (!WriteFile(h, (const void *)buf, (DWORD)count, &bytes_written, NULL)) {
        return -1;
    }
    return (int64_t)bytes_written;
#else
    return (int64_t)write((int)fd, (const void *)buf, (size_t)count);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_open(path, flags)
 *
 * flags 约定（POSIX 风格，高 16 位可指定权限）:
 *   O_RDONLY = 0, O_WRONLY = 1, O_RDWR = 2
 *   O_CREAT  = 0x40, O_TRUNC = 0x80
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_open(int64_t path, int64_t flags) {
#ifdef _WIN32
    if (!path) return -1;
    const char *p = (const char *)path;
    DWORD access = GENERIC_READ | GENERIC_WRITE;
    DWORD share = FILE_SHARE_READ | FILE_SHARE_WRITE;
    DWORD disp = OPEN_EXISTING;

    /* 解析 POSIX flags */
    int posix_flags = (int)(flags & 0xFFFF);
    if (posix_flags & 0x40) { /* O_CREAT */
        disp = OPEN_ALWAYS;
        if (!(posix_flags & 0x80)) { /* !O_TRUNC */
            /* 追加模式 */
        }
    } else if (posix_flags == 0) { /* O_RDONLY */
        access = GENERIC_READ;
    } else if (posix_flags == 1) { /* O_WRONLY */
        access = GENERIC_WRITE;
    } else if (posix_flags == 2) { /* O_RDWR */
        access = GENERIC_READ | GENERIC_WRITE;
    }

    HANDLE h = CreateFileA(
        p,
        access,
        share,
        NULL,
        disp,
        FILE_ATTRIBUTE_NORMAL,
        NULL
    );
    if (h == INVALID_HANDLE_VALUE) return -1;

    /* 注册到 fd 映射表 */
    for (int i = 3; i < AURA_FD_MAX; i++) {
        if (aura_fd_to_handle[i] == NULL) {
            aura_fd_to_handle[i] = h;
            return i;
        }
    }
    CloseHandle(h);
    return -1;
#else
    return (int64_t)open((const char *)path, (int)flags);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_close(fd)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_close(int64_t fd) {
#ifdef _WIN32
    if (fd >= 0 && fd < AURA_FD_MAX) {
        HANDLE h = aura_fd_to_handle[fd];
        if (h && h != INVALID_HANDLE_VALUE) {
            CloseHandle(h);
            aura_fd_to_handle[fd] = NULL;
        }
    }
    return 0;
#else
    return (int64_t)close((int)fd);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_fstat(fd, buf)
 *
 * buf 指向用户提供的 stat 结构。简化实现：只填充 st_size 字段（假设偏移 0 = st_mode, 偏移 8 = st_size）。
 * 对于 Aura 的用途，通常只需知道文件是否存在及大小。
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_fstat(int64_t fd, int64_t buf) {
#ifdef _WIN32
    HANDLE h = aura_get_std_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -1;
    BY_HANDLE_FILE_INFORMATION info;
    if (!GetFileSize(h, NULL) && !GetFileType(h)) return -1;

    /* 简化: 通过 GetFileSize 获取大小 */
    LARGE_INTEGER size = { 0 };
    if (!GetFileSizeEx(h, &size)) return -1;

    /* 如果 buf 非空，填入大小（偏移 0） */
    if (buf) {
        int64_t *b = (int64_t *)buf;
        b[0] = size.QuadPart; /* st_size */
    }
    return 0;
#else
    struct stat st;
    if (fstat((int)fd, &st) < 0) return -1;
    if (buf) {
        struct stat *dst = (struct stat *)buf;
        memcpy(dst, &st, sizeof(struct stat));
    }
    return 0;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_lseek(fd, off, whence)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_lseek(int64_t fd, int64_t off, int64_t whence) {
#ifdef _WIN32
    HANDLE h = aura_get_std_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -1;
    LARGE_INTEGER dist;
    dist.QuadPart = off;
    DWORD method = FILE_BEGIN;
    if (whence == 1) method = FILE_CURRENT;
    else if (whence == 2) method = FILE_END;
    LARGE_INTEGER result;
    if (!SetFilePointerEx(h, dist, &result, method)) return -1;
    return (int64_t)result.QuadPart;
#else
    return (int64_t)lseek((int)fd, (off_t)off, (int)whence);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_mmap(addr, len, prot, flags, fd, off)
 *
 * prot 约定:
 *   1 = PROT_READ, 2 = PROT_WRITE, 3 = PROT_READ|PROT_WRITE
 * flags 约定:
 *   MAP_PRIVATE = 2, MAP_SHARED = 1
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_mmap(int64_t addr, int64_t len, int64_t prot,
                          int64_t flags, int64_t fd, int64_t off) {
#ifdef _WIN32
    (void)addr; (void)flags;

    DWORD alloc_flags = MEM_COMMIT;
    DWORD access_flags = 0;
    if ((prot & 1)) access_flags |= PAGE_READONLY;
    if ((prot & 2)) access_flags |= PAGE_READWRITE;

    /* 如果 fd == -1 或 0 (anonymous mapping) */
    HANDLE hFile = NULL;
    if (fd >= 0) {
        hFile = aura_get_std_handle(fd);
        if (hFile == INVALID_HANDLE_VALUE) return -1;
    }

    void *result = VirtualAlloc(
        NULL,
        (SIZE_T)len,
        alloc_flags,
        access_flags
    );
    (void)hFile; /* VirtualAlloc 不支持文件映射，简化实现 */
    (void)off;
    return (int64_t)(uintptr_t)result;
#else
    int prot_flags = 0;
    if ((prot & 1)) prot_flags |= PROT_READ;
    if ((prot & 2)) prot_flags |= PROT_WRITE;

    int map_flags = MAP_PRIVATE; /* 默认私有映射 */
    if (flags & 1) map_flags = MAP_SHARED;

    int mmap_fd = (fd == 0) ? -1 : (int)fd;
    if (mmap_fd == 0) mmap_fd = -1; /* anonymous */

    void *result = mmap(
        (void *)addr,
        (size_t)len,
        prot_flags,
        map_flags,
        mmap_fd,
        (off_t)off
    );
    if (result == MAP_FAILED) return -1;
    return (int64_t)(uintptr_t)result;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_munmap(addr, len)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_munmap(int64_t addr, int64_t len) {
#ifdef _WIN32
    if (!VirtualFree((void *)addr, 0, MEM_RELEASE)) {
        return -1;
    }
    (void)len;
    return 0;
#else
    if (munmap((void *)addr, (size_t)len) < 0) return -1;
    return 0;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_access(path, mode)
 *
 * mode 约定: F_OK = 0, R_OK = 4, W_OK = 2, X_OK = 1
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_access(int64_t path, int64_t mode) {
#ifdef _WIN32
    if (!path) return -1;
    const char *p = (const char *)path;
    DWORD attribs = GetFileAttributesA(p);
    if (attribs == INVALID_FILE_ATTRIBUTES) return -1;

    int64_t m = mode;
    if (m == 0) { /* F_OK: exists? */
        return 0;
    }
    /* 简化: Windows 上权限检查有限，文件存在且非目录即可读写 */
    if (attribs & FILE_ATTRIBUTE_DIRECTORY) {
        return -1; /* 目录不允许作为文件读写 */
    }
    return 0;
#else
    return (int64_t)access((const char *)path, (int)mode);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_unlink(path)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_unlink(int64_t path) {
#ifdef _WIN32
    if (!path) return -1;
    if (!DeleteFileA((const char *)path)) return -1;
    return 0;
#else
    return (int64_t)unlink((const char *)path);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_execve(path, args, env)
 *
 * 注意: Windows 上不支持 execve (替换进程映像)。
 * 简化实现: 调用 system() 执行命令，然后退出。
 * args 和 env 是 char** 数组（以 NULL 结尾）。
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_execve(int64_t path, int64_t args, int64_t env) {
#ifdef _WIN32
    if (!path) return -1;
    const char *p = (const char *)path;

    /* 简化: 通过 CreateProcessA 启动新进程，不替换当前进程映像 */
    STARTUPINFOA si = { sizeof(si) };
    PROCESS_INFORMATION pi = { 0 };

    /* 构建命令字符串 */
    char cmd_buf[4096];
    snprintf(cmd_buf, sizeof(cmd_buf), "\"%s\"", p);

    if (args) {
        const char **argp = (const char **)args;
        for (int i = 0; argp[i]; i++) {
            int used = (int)strlen(cmd_buf);
            if (used + 1 < (int)sizeof(cmd_buf)) {
                cmd_buf[used] = ' ';
                cmd_buf[used + 1] = '\0';
            }
            int len = (int)strlen(argp[i]);
            if (used + len + 2 < (int)sizeof(cmd_buf)) {
                strncat(cmd_buf, argp[i], sizeof(cmd_buf) - strlen(cmd_buf) - 1);
            }
        }
    }

    (void)env;
    (void)si;

    if (!CreateProcessA(NULL, cmd_buf, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) {
        return -1;
    }
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);
    return 0;
#else
    const char *p = (const char *)path;
    char *const *a = (char *const *)args;
    char *const *e = (char *const *)env;
    return (int64_t)execve(p, a, e);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_exit_group(code)
 *
 * 必须真正终止进程。
 * --------------------------------------------------------------------------- */
void aura_syscall_exit_group(int64_t code) {
#ifdef _WIN32
    ExitProcess((unsigned int)code);
#else
    _exit((int)code);
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_wait4(pid, status, options, rusage)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_wait4(int64_t pid, int64_t status,
                           int64_t options, int64_t rusage) {
#ifdef _WIN32
    if (pid > 0) {
        HANDLE h = (HANDLE)(uintptr_t)pid;
        DWORD result = WaitForSingleObject(h, INFINITE);
        if (result != WAIT_OBJECT_0) return -1;
        if (status) {
            int *s = (int *)status;
            GetExitCodeProcess(h, (DWORD *)s);
        }
    }
    (void)options;
    (void)rusage;
    return 0;
#else
    int wstatus;
    struct rusage usage;
    memset(&usage, 0, sizeof(usage));
    int w = wait4((pid_t)pid, &wstatus, (int)options, &usage);
    if (w < 0) return -1;
    if (status) {
        int *s = (int *)status;
        *s = wstatus;
    }
    if (rusage) {
        struct rusage *ru = (struct rusage *)rusage;
        memcpy(ru, &usage, sizeof(struct rusage));
    }
    return 0;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_clock_gettime(clock, ts)
 *
 * clock 约定: CLOCK_REALTIME = 0, CLOCK_MONOTONIC = 1
 * ts 指向 struct timespec { tv_sec, tv_nsec }
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_clock_gettime(int64_t clock, int64_t ts) {
#ifdef _WIN32
    FILETIME ft;
    ULARGE_INTEGER uli;
    (void)clock;

    if (clock == 1) {
        /* CLOCK_MONOTONIC: 使用 QueryPerformanceCounter */
        LARGE_INTEGER freq, count;
        if (!QueryPerformanceFrequency(&freq)) return -1;
        if (!QueryPerformanceCounter(&count)) return -1;
        /* 转换为秒和纳秒 */
        int64_t secs = count.QuadPart / freq.QuadPart;
        int64_t nsecs = ((count.QuadPart % freq.QuadPart) * 1000000000LL) / freq.QuadPart;
        if (ts) {
            int64_t *t = (int64_t *)ts;
            t[0] = secs;
            t[1] = nsecs;
        }
        return 0;
    } else {
        /* CLOCK_REALTIME */
        GetSystemTimeAsFileTime(&ft);
        uli.LowPart = ft.dwLowDateTime;
        uli.HighPart = ft.dwHighDateTime;
        /* FILETIME 是从 1601-01-01 开始，转换为 Unix 时间 (从 1970-01-01) */
        int64_t unix_100ns = uli.QuadPart - 116444736000000000LL;
        int64_t secs = unix_100ns / 10000000;
        int64_t nsecs = (unix_100ns % 10000000) * 100;
        if (ts) {
            int64_t *t = (int64_t *)ts;
            t[0] = secs;
            t[1] = nsecs;
        }
        return 0;
    }
#else
    struct timespec t;
    if (clock_gettime((clockid_t)clock, &t) < 0) return -1;
    if (ts) {
        int64_t *d = (int64_t *)ts;
        d[0] = t.tv_sec;
        d[1] = t.tv_nsec;
    }
    return 0;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_getrandom(buf, len, flags)
 *
 * 使用 CryptGenRandom (Windows) 或 getrandom (Linux) / arc4random_buf (macOS)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_getrandom(int64_t buf, int64_t len, int64_t flags) {
#ifdef _WIN32
    (void)flags;
    HCRYPTPROV hProv = 0;
    if (!CryptAcquireContext(&hProv, NULL, NULL, PROV_RSA_AES, 0)) {
        CryptAcquireContext(&hProv, NULL, NULL, PROV_RSA_FULL, CRYPT_NEWKEYSET);
    }
    if (!hProv) return -1;
    if (!CryptGenRandom(hProv, (DWORD)len, (BYTE *)buf)) {
        CryptReleaseContext(hProv, 0);
        return -1;
    }
    CryptReleaseContext(hProv, 0);
    return (int64_t)len;
#else
    int64_t got = 0;
    while (got < len) {
        ssize_t r = getrandom((char *)buf + got, (size_t)(len - got), 0);
        if (r <= 0) return -1;
        got += r;
    }
    return got;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_readv(fd, iov, iovcnt)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_readv(int64_t fd, int64_t iov, int64_t iovcnt) {
#ifdef _WIN32
    HANDLE h = aura_get_std_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -1;

    struct iovec {
        void *iov_base;
        size_t iov_len;
    };

    int64_t total = 0;
    for (int64_t i = 0; i < iovcnt; i++) {
        struct iovec *v = (struct iovec *)iov + i;
        DWORD bytes_read = 0;
        if (!ReadFile(h, v->iov_base, (DWORD)v->iov_len, &bytes_read, NULL)) {
            return -1;
        }
        total += bytes_read;
        if (bytes_read < v->iov_len) break; /* 未读满，停止 */
    }
    return total;
#else
    struct iovec {
        void *iov_base;
        size_t iov_len;
    };
    int64_t r = (int64_t)readv((int)fd, (struct iovec *)iov, (int)iovcnt);
    return r;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_writev(fd, iov, iovcnt)
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_writev(int64_t fd, int64_t iov, int64_t iovcnt) {
#ifdef _WIN32
    HANDLE h = aura_get_std_handle(fd);
    if (h == INVALID_HANDLE_VALUE) return -1;

    struct iovec {
        void *iov_base;
        size_t iov_len;
    };

    int64_t total = 0;
    for (int64_t i = 0; i < iovcnt; i++) {
        struct iovec *v = (struct iovec *)iov + i;
        DWORD bytes_written = 0;
        if (!WriteFile(h, v->iov_base, (DWORD)v->iov_len, &bytes_written, NULL)) {
            return -1;
        }
        total += bytes_written;
        if (bytes_written < v->iov_len) break;
    }
    return total;
#else
    struct iovec {
        void *iov_base;
        size_t iov_len;
    };
    int64_t r = (int64_t)writev((int)fd, (struct iovec *)iov, (int)iovcnt);
    return r;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_pipe(pipes)
 *
 * pipes 是一个指向 [2]int64_t 的数组（读端, 写端）
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_pipe(int64_t pipes) {
#ifdef _WIN32
    SECURITY_ATTRIBUTES sa = { sizeof(sa) };
    sa.bInheritHandle = TRUE;

    HANDLE read_h = NULL;
    HANDLE write_h = NULL;
    if (!CreatePipe(&read_h, &write_h, &sa, 0)) return -1;

    if (pipes) {
        int64_t *p = (int64_t *)pipes;
        /* 注册到 fd 映射表 */
        for (int i = 3; i < AURA_FD_MAX; i++) {
            if (aura_fd_to_handle[i] == NULL) {
                aura_fd_to_handle[i] = read_h;
                p[0] = i;
                break;
            }
        }
        for (int i = 3; i < AURA_FD_MAX; i++) {
            if (aura_fd_to_handle[i] == NULL) {
                aura_fd_to_handle[i] = write_h;
                p[1] = i;
                break;
            }
        }
    }
    return 0;
#else
    int p[2];
    if (pipe(p) < 0) return -1;
    if (pipes) {
        int64_t *out = (int64_t *)pipes;
        out[0] = p[0];
        out[1] = p[1];
    }
    return 0;
#endif
}

/* ---------------------------------------------------------------------------
 * aura_syscall_dispatch(nr, a1, a2, a3, a4, a5, a6)
 *
 * 通用 syscall 分发入口。按 syscall 号（Linux x86_64 ABI）分发到
 * 上述各个 aura_syscall_* 函数。
 *
 * Syscall numbers (Linux x86_64):
 *   0  = read,      1  = write,     2  = open,      3  = close
 *   4  = stat,      5  = fstat,     6  = lseek,     9  = mmap
 *   10 = mprotect,  11 = munmap,    13 = access,    14 = unlink
 *   59 = execve,    60 = exit,      61 = wait4,     98 = clone
 *   201 = exit_group, 222 = pipe,   230 = clock_gettime
 *   257 = getrandom, 275 = readv,   276 = writev
 * --------------------------------------------------------------------------- */
int64_t aura_syscall_dispatch(int64_t nr, int64_t a1, int64_t a2,
                              int64_t a3, int64_t a4, int64_t a5, int64_t a6) {
    switch (nr) {
        case 0:  return aura_syscall_read(a1, a2, a3);
        case 1:  return aura_syscall_write(a1, a2, a3);
        case 2:  return aura_syscall_open(a1, a2);
        case 3:  return aura_syscall_close(a1);
        case 4:  /* stat - 暂不支持 */
        case 5:  return aura_syscall_fstat(a1, a2);
        case 6:  return aura_syscall_lseek(a1, a2, a3);
        case 9:  return aura_syscall_mmap(a1, a2, a3, a4, a5, a6);
        case 11: return aura_syscall_munmap(a1, a2);
        case 13: return aura_syscall_access(a1, a2);
        case 14: return aura_syscall_unlink(a1);
        case 59: return aura_syscall_execve(a1, a2, a3);
        case 60:
        case 201:
            aura_syscall_exit_group(a1);
            return 0; /* unreachable */
        case 61: return aura_syscall_wait4(a1, a2, a3, a4);
        case 222: return aura_syscall_pipe(a1);
        case 230: return aura_syscall_clock_gettime(a1, a2);
        case 257: return aura_syscall_getrandom(a1, a2, a3);
        case 275: return aura_syscall_readv(a1, a2, a3);
        case 276: return aura_syscall_writev(a1, a2, a3);
        default:
            return -1; /* unknown syscall */
    }
}

/* =============================================================================
 * Section 2: Memory.aura 内置指令
 *
 * Memory.alloc(n) / Memory.free(addr)
 * 通过 mmap/munmap 实现，返回 i8* (int64_t)。
 * ============================================================================= */

int64_t aura_memory_alloc(int64_t n) {
    if (n <= 0) return 0;
    /* 分配 16 字节对齐的内存 */
    int64_t aligned = (n + 15) & ~(int64_t)15;
    int64_t addr = aura_syscall_mmap(0, aligned, 3, 2, -1, 0);
    /* prot=3 (RW), flags=2 (PRIVATE), fd=-1 (anonymous) */
    if (addr <= 0) return 0;
    /* 清零 */
    memset((void *)addr, 0, (size_t)aligned);
    return addr;
}

void aura_memory_free(int64_t addr) {
    if (addr == 0) return;
    /* 简化: 不知道原始大小，VirtualFree 用 0 表示释放全部 */
#ifdef _WIN32
    VirtualFree((void *)addr, 0, MEM_RELEASE);
#else
    /* POSIX: 同样需要知道大小，简化实现 */
    /* 这里使用一个 hack: 如果 addr 是页面倍数，尝试释放整个页面 */
    int64_t page_size = 4096;
    int64_t aligned = (addr + page_size - 1) & ~(page_size - 1);
    munmap((void *)aligned, (size_t)page_size);
#endif
}

/* =============================================================================
 * Section 3: Cpu.aura 内联汇编
 *
 * Cpu.rdtsc() / Cpu.memFence() / Cpu.atomicAdd(addr, delta)
 * ============================================================================= */

int64_t aura_cpu_rdtsc(void) {
#ifdef _WIN32
    /* MSVC/LLVM: 使用内联汇编或 __rdtsc intrinsic */
    return (int64_t)__rdtsc();
#elif defined(__x86_64__) || defined(__i386__)
    unsigned int lo, hi;
    __asm__ __volatile__("rdtsc" : "=a"(lo), "=d"(hi));
    return ((int64_t)hi << 32) | lo;
#else
    /* 非 x86: 退回到系统时间 */
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000000LL + ts.tv_nsec;
#endif
}

void aura_cpu_mem_fence(void) {
#ifdef _WIN32
    MemoryBarrier();
#elif defined(__x86_64__) || defined(__i386__)
    __asm__ __volatile__("mfence" ::: "memory");
#elif defined(__aarch64__)
    __asm__ __volatile__("dmb ish" ::: "memory");
#else
    __asm__ __volatile__("" ::: "memory"); /* compiler barrier */
#endif
}

int64_t aura_cpu_atomic_add(int64_t addr, int64_t delta) {
    volatile int64_t *p = (volatile int64_t *)(uintptr_t)addr;
#ifdef _WIN32
    /* InterlockedAdd64 返回旧值 */
    return (int64_t)InterlockedAdd64(p, (LONG64)delta);
#else
    /* GCC/Clang: __sync_fetch_and_add */
    return __sync_fetch_and_add(p, delta);
#endif
}

/* =============================================================================
 * Section 4: 并发运行时原语
 *
 * 这些函数由 aura_std_cffi.c 通过 extern 声明调用，
 * 为 Aura 的并发原语 (Mutex / Atomic / RwLock / Condvar / Barrier / Thread)
 * 提供平台相关实现。
 *
 * 返回值约定:
 *   - 非负值 = 成功，返回平台句柄/ID
 *   - -1 = 失败
 * ============================================================================= */

/* ---------------------------------------------------------------------------
 * Mutex (互斥锁)
 * --------------------------------------------------------------------------- */

int64_t aura_mutex_new(void) {
#ifdef _WIN32
    CRITICAL_SECTION *cs = (CRITICAL_SECTION *)malloc(sizeof(CRITICAL_SECTION));
    if (!cs) return -1;
    InitializeCriticalSection(cs);
    return (int64_t)(uintptr_t)cs;
#else
    pthread_mutex_t *m = (pthread_mutex_t *)malloc(sizeof(pthread_mutex_t));
    if (!m) return -1;
    if (pthread_mutex_init(m, NULL) != 0) { free(m); return -1; }
    return (int64_t)(uintptr_t)m;
#endif
}

void aura_mutex_lock(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    CRITICAL_SECTION *cs = (CRITICAL_SECTION *)(uintptr_t)id;
    EnterCriticalSection(cs);
#else
    pthread_mutex_lock((pthread_mutex_t *)(uintptr_t)id);
#endif
}

void aura_mutex_unlock(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    CRITICAL_SECTION *cs = (CRITICAL_SECTION *)(uintptr_t)id;
    LeaveCriticalSection(cs);
#else
    pthread_mutex_unlock((pthread_mutex_t *)(uintptr_t)id);
#endif
}

int aura_mutex_trylock(int64_t id) {
    if (id <= 0) return 0;
#ifdef _WIN32
    CRITICAL_SECTION *cs = (CRITICAL_SECTION *)(uintptr_t)id;
    return TryEnterCriticalSection(cs) != 0 ? 1 : 0;
#else
    return pthread_mutex_trylock((pthread_mutex_t *)(uintptr_t)id) == 0 ? 1 : 0;
#endif
}

void aura_mutex_destroy(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    CRITICAL_SECTION *cs = (CRITICAL_SECTION *)(uintptr_t)id;
    DeleteCriticalSection(cs);
    free(cs);
#else
    pthread_mutex_t *m = (pthread_mutex_t *)(uintptr_t)id;
    pthread_mutex_destroy(m);
    free(m);
#endif
}

/* ---------------------------------------------------------------------------
 * Atomic (原子操作)
 * --------------------------------------------------------------------------- */

int64_t aura_atomic_load(volatile int64_t *addr) {
    return __atomic_load_n(addr, __ATOMIC_SEQ_CST);
}

void aura_atomic_store(volatile int64_t *addr, int64_t val) {
    __atomic_store_n(addr, val, __ATOMIC_SEQ_CST);
}

int64_t aura_atomic_add(volatile int64_t *addr, int64_t delta) {
    return aura_cpu_atomic_add((int64_t)(uintptr_t)addr, delta);
}

int64_t aura_atomic_sub(volatile int64_t *addr, int64_t delta) {
#ifdef _WIN32
    return (int64_t)InterlockedAdd64(addr, -(LONG64)delta);
#else
    return __sync_fetch_and_sub(addr, delta);
#endif
}

int aura_atomic_cas(volatile int64_t *addr, int64_t expected, int64_t desired) {
#ifdef _WIN32
    LONG64 old = InterlockedCompareExchange64(addr, (LONG64)desired, (LONG64)expected);
    return old == expected ? 1 : 0;
#else
    return __sync_val_compare_and_swap(addr, expected, desired) == expected ? 1 : 0;
#endif
}

/* ---------------------------------------------------------------------------
 * RwLock (读写锁)
 *
 * Windows: 使用 SRWLOCK
 * POSIX: 使用 pthread_rwlock_t
 * --------------------------------------------------------------------------- */

int64_t aura_rwlock_new(void) {
#ifdef _WIN32
    SRWLOCK *l = (SRWLOCK *)malloc(sizeof(SRWLOCK));
    if (!l) return -1;
    InitializeSRWLock(l);
    return (int64_t)(uintptr_t)l;
#else
    pthread_rwlock_t *l = (pthread_rwlock_t *)malloc(sizeof(pthread_rwlock_t));
    if (!l) return -1;
    if (pthread_rwlock_init(l, NULL) != 0) { free(l); return -1; }
    return (int64_t)(uintptr_t)l;
#endif
}

void aura_rwlock_read_lock(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    SRWLOCK *l = (SRWLOCK *)(uintptr_t)id;
    AcquireSRWLockShared(l);
#else
    pthread_rwlock_rdlock((pthread_rwlock_t *)(uintptr_t)id);
#endif
}

void aura_rwlock_write_lock(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    SRWLOCK *l = (SRWLOCK *)(uintptr_t)id;
    AcquireSRWLockExclusive(l);
#else
    pthread_rwlock_wrlock((pthread_rwlock_t *)(uintptr_t)id);
#endif
}

void aura_rwlock_read_unlock(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    SRWLOCK *l = (SRWLOCK *)(uintptr_t)id;
    ReleaseSRWLockShared(l);
#else
    pthread_rwlock_unlock((pthread_rwlock_t *)(uintptr_t)id);
#endif
}

void aura_rwlock_write_unlock(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    SRWLOCK *l = (SRWLOCK *)(uintptr_t)id;
    ReleaseSRWLockExclusive(l);
#else
    pthread_rwlock_unlock((pthread_rwlock_t *)(uintptr_t)id);
#endif
}

void aura_rwlock_destroy(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    free((void *)(uintptr_t)id);
#else
    pthread_rwlock_destroy((pthread_rwlock_t *)(uintptr_t)id);
    free((void *)(uintptr_t)id);
#endif
}

/* ---------------------------------------------------------------------------
 * Condvar (条件变量)
 *
 * Windows: 使用条件变量结构体 + WakeConditionVariable / WaitOnConditionVariable
 * POSIX: 使用 pthread_cond_t
 *
 * 注意: Win32 的条件变量需要与 CRITICAL_SECTION 配对使用。
 * 这里简化实现：Condvar 内部持有 mutex ID。
 * --------------------------------------------------------------------------- */

#ifdef _WIN32
typedef struct {
    CONDITION_VARIABLE cv;
} AuraCondvar;
#else
typedef pthread_cond_t AuraCondvar;
#endif

int64_t aura_condvar_new(void) {
#ifdef _WIN32
    AuraCondvar *c = (AuraCondvar *)malloc(sizeof(AuraCondvar));
    if (!c) return -1;
    return (int64_t)(uintptr_t)c;
#else
    pthread_cond_t *c = (pthread_cond_t *)malloc(sizeof(pthread_cond_t));
    if (!c) return -1;
    if (pthread_cond_init(c, NULL) != 0) { free(c); return -1; }
    return (int64_t)(uintptr_t)c;
#endif
}

void aura_condvar_wait(int64_t cv_id, int64_t mutex_id) {
    if (cv_id <= 0 || mutex_id <= 0) return;
#ifdef _WIN32
    AuraCondvar *c = (AuraCondvar *)(uintptr_t)cv_id;
    CRITICAL_SECTION *m = (CRITICAL_SECTION *)(uintptr_t)mutex_id;
    /* 使用 SleepConditionVariableCS (Win8+) 或 WaitOnAddress (Win8+) */
    /* 简化: 使用 SleepConditionVariableCS 需要链接 kernel32.lib */
    /* 这里使用一个简单的忙等替代（非阻塞，适用于短等待场景） */
    /* 更优实现: 使用 Win8 的条件变量 API */
    SleepConditionVariableCS(&c->cv, m, INFINITE);
#else
    pthread_mutex_t *m = (pthread_mutex_t *)(uintptr_t)mutex_id;
    pthread_cond_t *c = (pthread_cond_t *)(uintptr_t)cv_id;
    pthread_cond_wait(c, m);
#endif
}

void aura_condvar_signal(int64_t cv_id) {
    if (cv_id <= 0) return;
#ifdef _WIN32
    AuraCondvar *c = (AuraCondvar *)(uintptr_t)cv_id;
    WakeConditionVariable(&c->cv);
#else
    pthread_cond_signal((pthread_cond_t *)(uintptr_t)cv_id);
#endif
}

void aura_condvar_broadcast(int64_t cv_id) {
    if (cv_id <= 0) return;
#ifdef _WIN32
    AuraCondvar *c = (AuraCondvar *)(uintptr_t)cv_id;
    WakeAllConditionVariable(&c->cv);
#else
    pthread_cond_broadcast((pthread_cond_t *)(uintptr_t)cv_id);
#endif
}

void aura_condvar_destroy(int64_t cv_id) {
    if (cv_id <= 0) return;
#ifdef _WIN32
    free((void *)(uintptr_t)cv_id);
#else
    pthread_cond_destroy((pthread_cond_t *)(uintptr_t)cv_id);
    free((void *)(uintptr_t)cv_id);
#endif
}

/* ---------------------------------------------------------------------------
 * Barrier (屏障)
 *
 * Windows: 使用自旋锁 + 计数模拟
 * POSIX: 使用 pthread_barrier_t
 * --------------------------------------------------------------------------- */

typedef struct {
    int64_t count;
    int64_t arrived;
    int64_t gen;
#ifdef _WIN32
    CRITICAL_SECTION cs;
    CONDITION_VARIABLE cv;
#else
    pthread_mutex_t mu;
    pthread_cond_t cv;
#endif
} AuraBarrier;

int64_t aura_barrier_new(int64_t count) {
    if (count <= 0) return -1;
    AuraBarrier *b = (AuraBarrier *)malloc(sizeof(AuraBarrier));
    if (!b) return -1;
    b->count = count;
    b->arrived = 0;
    b->gen = 0;
#ifdef _WIN32
    InitializeCriticalSection(&b->cs);
#else
    pthread_mutex_init(&b->mu, NULL);
    pthread_cond_init(&b->cv, NULL);
#endif
    return (int64_t)(uintptr_t)b;
}

int64_t aura_barrier_wait(int64_t id) {
    if (id <= 0) return -1;
    AuraBarrier *b = (AuraBarrier *)(uintptr_t)id;

#ifdef _WIN32
    EnterCriticalSection(&b->cs);
    b->arrived++;
    if (b->arrived >= b->count) {
        b->arrived = 0;
        b->gen++;
        /* Wake all other waiting threads */
        WakeAllConditionVariable(&b->cv);
        LeaveCriticalSection(&b->cs);
        /* The last thread to arrive returns non-zero (gen+1) */
        return b->gen + 1;
    }
    /* Wait for the barrier to be released (gen changes) */
    int64_t gen_before = b->gen;
    while (b->gen == gen_before) {
        SleepConditionVariableCS(&b->cv, &b->cs, INFINITE);
    }
    LeaveCriticalSection(&b->cs);
    return 0;
#else
    pthread_mutex_lock(&b->mu);
    b->arrived++;
    if (b->arrived >= b->count) {
        b->arrived = 0;
        b->gen++;
        pthread_cond_broadcast(&b->cv);
        pthread_mutex_unlock(&b->mu);
        return b->gen + 1;
    }
    int64_t gen_before = b->gen;
    while (b->gen == gen_before) {
        pthread_cond_wait(&b->cv, &b->mu);
    }
    pthread_mutex_unlock(&b->mu);
    return 0;
#endif
}

void aura_barrier_destroy(int64_t id) {
    if (id <= 0) return;
    AuraBarrier *b = (AuraBarrier *)(uintptr_t)id;
#ifdef _WIN32
    DeleteCriticalSection(&b->cs);
    free(b);
#else
    pthread_mutex_destroy(&b->mu);
    pthread_cond_destroy(&b->cv);
    free(b);
#endif
}

/* ---------------------------------------------------------------------------
 * Thread (线程)
 *
 * 注意: aura_thread_create 接收 fn_id 和 arg，创建新线程。
 * fn_id 是 Aura 函数在内部注册表中的索引。
 * 实际的线程函数通过 thread_dispatch 分派。
 *
 * 简化实现: 使用平台 API 创建线程，线程入口调用一个全局分派函数。
 * --------------------------------------------------------------------------- */

/* 线程分派函数指针类型 */
typedef int64_t (*AuraThreadFunc)(int64_t arg);

/* 线程函数表 — 由 AOT 发射器填充，或运行时动态注册 */
#define AURA_THREAD_FN_MAX 64
static AuraThreadFunc aura_thread_fns[AURA_THREAD_FN_MAX];
static int64_t aura_thread_fn_count = 0;

/* 线程参数结构 */
typedef struct {
    int64_t fn_id;
    int64_t arg;
} AuraThreadParam;

#ifdef _WIN32
static DWORD WINAPI aura_thread_entry(LPVOID param) {
    AuraThreadParam *p = (AuraThreadParam *)param;
    int64_t fn_id = p->fn_id;
    int64_t arg = p->arg;
    free(p);

    if (fn_id >= 0 && fn_id < AURA_THREAD_FN_MAX && aura_thread_fns[fn_id]) {
        aura_thread_fns[fn_id](arg);
    }
    return 0;
}
#else
static void *aura_thread_entry(void *param) {
    AuraThreadParam *p = (AuraThreadParam *)param;
    int64_t fn_id = p->fn_id;
    int64_t arg = p->arg;
    free(p);

    if (fn_id >= 0 && fn_id < AURA_THREAD_FN_MAX && aura_thread_fns[fn_id]) {
        aura_thread_fns[fn_id](arg);
    }
    return NULL;
}
#endif

int64_t aura_thread_create(int64_t fn_id, int64_t arg) {
    AuraThreadParam *p = (AuraThreadParam *)malloc(sizeof(AuraThreadParam));
    if (!p) return -1;
    p->fn_id = fn_id;
    p->arg = arg;

#ifdef _WIN32
    HANDLE h = CreateThread(NULL, 0, aura_thread_entry, p, 0, NULL);
    if (!h) { free(p); return -1; }
    return (int64_t)(uintptr_t)h;
#else
    pthread_t tid;
    if (pthread_create(&tid, NULL, aura_thread_entry, p) != 0) {
        free(p);
        return -1;
    }
    return (int64_t)(uintptr_t)tid;
#endif
}

void aura_thread_join(int64_t id) {
    if (id <= 0) return;
#ifdef _WIN32
    HANDLE h = (HANDLE)(uintptr_t)id;
    WaitForSingleObject(h, INFINITE);
    CloseHandle(h);
#else
    pthread_t tid = (pthread_t)(uintptr_t)id;
    pthread_join(tid, NULL);
#endif
}

void aura_thread_sleep(int64_t ms) {
    if (ms <= 0) return;
#ifdef _WIN32
    Sleep((DWORD)ms);
#else
    struct timespec ts;
    ts.tv_sec = ms / 1000;
    ts.tv_nsec = (ms % 1000) * 1000000L;
    nanosleep(&ts, NULL);
#endif
}

int64_t aura_thread_id(void) {
#ifdef _WIN32
    return (int64_t)GetCurrentThreadId();
#else
    return (int64_t)(uintptr_t)pthread_self();
#endif
}

int64_t aura_thread_available_parallelism(void) {
#ifdef _WIN32
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    /* 返回逻辑处理器数量 */
    return (int64_t)si.dwNumberOfProcessors;
#else
    long n = sysconf(_SC_NPROCESSORS_ONLN);
    if (n < 1) n = 1;
    return (int64_t)n;
#endif
}

/* 辅助: 注册线程函数（供 AOT 发射器或用户代码调用） */
int64_t aura_thread_register_fn(AuraThreadFunc fn) {
    if (aura_thread_fn_count >= AURA_THREAD_FN_MAX) return -1;
    aura_thread_fns[aura_thread_fn_count] = fn;
    return aura_thread_fn_count++;
}

/* =============================================================================
 * Section 5: 运行时支持函数
 *
 * 这些函数由 emit.rs 生成的 LLVM IR 调用，
 * 为 AOT 可执行文件提供 setjmp/longjmp 异常桥和 argv 注入。
 * ============================================================================= */

/* ---------------------------------------------------------------------------
 * aura_setjmp(buf) — 保存栈上下文
 *
 * 对应 LLVM: %result = call i32 @aura_setjmp(i8* %jmp_buf)
 * 返回 0 = 正常调用路径，非 0 = 从 longjmp 返回
 * --------------------------------------------------------------------------- */
int32_t aura_setjmp(jmp_buf buf) {
    return setjmp(buf);
}

/* ---------------------------------------------------------------------------
 * aura_longjmp(buf, val) — 跳转到保存的栈上下文
 *
 * 对应 LLVM: call void @aura_longjmp(i8* %jmp_buf, i32 %val)
 * 永不返回（除非 val == 0，但约定 val != 0）
 * --------------------------------------------------------------------------- */
void aura_longjmp(jmp_buf buf, int32_t val) {
    longjmp(buf, val ? val : 1);
}

/* ---------------------------------------------------------------------------
 * aura_args_set(argc, argv) — 注入宿主进程 argv
 *
 * 对应 LLVM: call void @aura_args_set(i32 %argc, i8** %argv)
 * 由 AOT 发射的 C 入口 main 在函数体最开始调用。
 * --------------------------------------------------------------------------- */
void aura_args_set(int32_t argc, char **argv) {
    aura_argc_global = argc;
    aura_argv_global = argv;
}

/* =============================================================================
 * Section 6: 辅助函数 — 供 aura_std_cffi.c 内部使用
 *
 * 这些函数可能被 aura_std_cffi.c 或其他模块引用。
 * ============================================================================= */

/* strdup 替代（Windows MSVC 没有 strdup） */
char *aura_strdup(const char *s) {
    if (!s) return NULL;
    size_t len = strlen(s) + 1;
    char *p = (char *)malloc(len);
    if (!p) return NULL;
    memcpy(p, s, len);
    return p;
}

/* =============================================================================
 * End of aura_syscalls.c
 * ============================================================================= */
