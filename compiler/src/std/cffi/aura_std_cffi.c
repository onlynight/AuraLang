/**
 * Aura std C FFI — 标准库 C ABI 实现
 *
 * 供 AOT 编译后端链接使用。
 * 实现方式：每个函数对应 Aura 的 std 函数，使用 C ABI 导出。
 *
 * 编译：
 *   clang -c aura_std_cffi.c -o aura_std_cffi.o
 */

// 抑制 Windows 安全函数警告
#ifdef _WIN32
#ifndef _CRT_SECURE_NO_WARNINGS
#define _CRT_SECURE_NO_WARNINGS
#endif
#endif

#include "aura_std_cffi.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <time.h>
#include <ctype.h>
#include <sys/stat.h>
#include <errno.h>

// 跨平台时间获取
#ifdef _WIN32
#include <windows.h>
#else
#include <sys/time.h>
#endif

// ─────────────────────────────────────────────────────────────────────────────
// Prelude（17 个全局内置）
// ─────────────────────────────────────────────────────────────────────────────

void aura_println(const char *s) {
    if (s) {
        printf("%s\n", s);
        fflush(stdout);
    } else {
        printf("\n");
        fflush(stdout);
    }
}

void aura_print(const char *s) {
    if (s) {
        printf("%s", s);
        fflush(stdout);
    }
}

void aura_puts(const char *s) {
    if (s) {
        puts(s);
        fflush(stdout);
    }
}

int64_t aura_abs(int64_t x) {
    return x < 0 ? -x : x;
}

double aura_sqrt(double x) {
    return sqrt(x);
}

double aura_pow(double base, double exp) {
    return pow(base, exp);
}

int64_t aura_to_int(double x) {
    return (int64_t)x;
}

double aura_to_float(int64_t x) {
    return (double)x;
}

const char *aura_to_str(int64_t x) {
    static char buf[64];
    snprintf(buf, sizeof(buf), "%lld", (long long)x);
    return buf;
}

double aura_clock(void) {
#ifdef _WIN32
    // Windows: 使用 QueryPerformanceCounter
    LARGE_INTEGER freq, count;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&count);
    return (double)count.QuadPart / (double)freq.QuadPart * 1000000.0;
#else
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (double)tv.tv_sec * 1000000.0 + (double)tv.tv_usec;
#endif
}

int64_t aura_strlen(const char *s) {
    return s ? (int64_t)strlen(s) : 0;
}

// ─────────────────────────────────────────────────────────────────────────────
// aura.io — 标准输入输出
// ─────────────────────────────────────────────────────────────────────────────

const char *aura_io_readLine(void) {
    static char buf[4096];
    if (fgets(buf, sizeof(buf), stdin)) {
        // 去掉末尾换行
        size_t len = strlen(buf);
        if (len > 0 && buf[len - 1] == '\n') {
            buf[len - 1] = '\0';
        }
        return buf;
    }
    return NULL;
}

int aura_io_fileExists(const char *path) {
    struct stat buffer;
    return (stat(path, &buffer) == 0) ? 1 : 0;
}

const char *aura_io_fileRead(const char *path) {
    static char buf[65536];
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    size_t n = fread(buf, 1, sizeof(buf) - 1, f);
    buf[n] = '\0';
    fclose(f);
    return buf;
}

void aura_io_fileWrite(const char *path, const char *content) {
    FILE *f = fopen(path, "wb");
    if (f) {
        fputs(content ? content : "", f);
        fclose(f);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// aura.math — 数学函数
// ─────────────────────────────────────────────────────────────────────────────

double aura_math_sin(double x) { return sin(x); }
double aura_math_cos(double x) { return cos(x); }
double aura_math_tan(double x) { return tan(x); }
double aura_math_asin(double x) { return asin(x); }
double aura_math_acos(double x) { return acos(x); }
double aura_math_atan(double x) { return atan(x); }
double aura_math_log(double x) { return log(x); }
double aura_math_exp(double x) { return exp(x); }

double aura_math_min(double a, double b) { return a < b ? a : b; }
double aura_math_max(double a, double b) { return a > b ? a : b; }
int64_t aura_math_ceil(double x) { return (int64_t)ceil(x); }
int64_t aura_math_floor(double x) { return (int64_t)floor(x); }

const double aura_math_PI = 3.14159265358979323846;
const double aura_math_E = 2.71828182845904523536;

// ─────────────────────────────────────────────────────────────────────────────
// aura.string — 字符串操作
// ─────────────────────────────────────────────────────────────────────────────

int aura_string_contains(const char *s, const char *sub) {
    if (!s || !sub) return 0;
    return strstr(s, sub) != NULL ? 1 : 0;
}

int64_t aura_string_length(const char *s) {
    return s ? (int64_t)strlen(s) : 0;
}

int64_t aura_string_charCodeAt(const char *s, int64_t idx) {
    if (!s || idx < 0 || (size_t)idx >= strlen(s)) return -1;
    return (unsigned char)s[idx];
}

const char *aura_string_charAt(const char *s, int64_t idx) {
    static char buf[2] = {0, 0};
    if (!s || idx < 0 || (size_t)idx >= strlen(s)) return "";
    buf[0] = s[idx];
    return buf;
}

const char *aura_string_substring(const char *s, int64_t start, int64_t end) {
    static char buf[4096];
    if (!s) return "";
    size_t len = strlen(s);
    if (start < 0) start = 0;
    if (end > (int64_t)len) end = len;
    if (start > end) return "";
    size_t n = (size_t)(end - start);
    if (n >= sizeof(buf)) n = sizeof(buf) - 1;
    strncpy(buf, s + start, n);
    buf[n] = '\0';
    return buf;
}

const char *aura_string_toUpperCase(const char *s) {
    static char buf[4096];
    if (!s) return "";
    size_t len = strlen(s);
    if (len >= sizeof(buf)) len = sizeof(buf) - 1;
    for (size_t i = 0; i < len; i++) {
        buf[i] = toupper((unsigned char)s[i]);
    }
    buf[len] = '\0';
    return buf;
}

const char *aura_string_toLowerCase(const char *s) {
    static char buf[4096];
    if (!s) return "";
    size_t len = strlen(s);
    if (len >= sizeof(buf)) len = sizeof(buf) - 1;
    for (size_t i = 0; i < len; i++) {
        buf[i] = tolower((unsigned char)s[i]);
    }
    buf[len] = '\0';
    return buf;
}

const char *aura_string_trim(const char *s) {
    static char buf[4096];
    if (!s) return "";
    size_t len = strlen(s);
    if (len == 0) return "";
    // 找起始非空白
    size_t start = 0;
    while (start < len && isspace((unsigned char)s[start])) start++;
    // 找结束非空白
    size_t end = len;
    while (end > start && isspace((unsigned char)s[end - 1])) end--;
    size_t n = end - start;
    if (n >= sizeof(buf)) n = sizeof(buf) - 1;
    strncpy(buf, s + start, n);
    buf[n] = '\0';
    return buf;
}

int aura_string_startsWith(const char *s, const char *prefix) {
    if (!s || !prefix) return 0;
    return strncmp(s, prefix, strlen(prefix)) == 0 ? 1 : 0;
}

int aura_string_endsWith(const char *s, const char *suffix) {
    if (!s || !suffix) return 0;
    size_t sl = strlen(s);
    size_t su = strlen(suffix);
    if (su > sl) return 0;
    return strcmp(s + sl - su, suffix) == 0 ? 1 : 0;
}

const char *aura_string_replace(const char *s, const char *from, const char *to) {
    static char buf[4096];
    if (!s || !from) return s ? s : "";
    size_t from_len = strlen(from);
    if (from_len == 0) return s ? s : "";
    size_t to_len = strlen(to);

    size_t i = 0;
    size_t j = 0;
    while (i < strlen(s) && j < sizeof(buf) - 1) {
        if (strncmp(s + i, from, from_len) == 0) {
            for (size_t k = 0; k < to_len && j < sizeof(buf) - 1; k++) {
                buf[j++] = to[k];
            }
            i += from_len;
        } else {
            buf[j++] = s[i++];
        }
    }
    buf[j] = '\0';
    return buf;
}

// ─────────────────────────────────────────────────────────────────────────────
// aura.time — 时间
// ─────────────────────────────────────────────────────────────────────────────

int64_t aura_time_epoch(void) {
#ifdef _WIN32
    // Windows: 从 UTC 时间戳计算 Unix 时间戳
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    ULARGE_INTEGER uli;
    uli.LowPart = ft.dwLowDateTime;
    uli.HighPart = ft.dwHighDateTime;
    // Windows FILETIME 是从 1601-01-01 开始的 100ns 间隔
    // Unix 时间戳是从 1970-01-01 开始的秒
    // 两者相差 11644473600 秒
    return (int64_t)(uli.QuadPart / 10000000ULL) - 11644473600LL;
#else
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (int64_t)tv.tv_sec;
#endif
}

int64_t aura_time_epochMillis(void) {
#ifdef _WIN32
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    ULARGE_INTEGER uli;
    uli.LowPart = ft.dwLowDateTime;
    uli.HighPart = ft.dwHighDateTime;
    return (int64_t)(uli.QuadPart / 10000ULL) - 11644473600000LL;
#else
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (int64_t)tv.tv_sec * 1000 + (int64_t)tv.tv_usec / 1000;
#endif
}

const char *aura_time_format(int64_t timestamp) {
    static char buf[64];
    time_t t = (time_t)timestamp;
    struct tm tm;
#ifdef _WIN32
    localtime_s(&tm, &t);
#else
    localtime_r(&t, &tm);
#endif
    strftime(buf, sizeof(buf), "%Y-%m-%d %H:%M:%S", &tm);
    return buf;
}

// ─────────────────────────────────────────────────────────────────────────────
// aura.random — 随机数
// ─────────────────────────────────────────────────────────────────────────────

static int64_t aura_random_seed = 0;

static void aura_random_init(void) {
    if (aura_random_seed == 0) {
#ifdef _WIN32
        aura_random_seed = (int64_t)GetTickCount64();
#else
        struct timeval tv;
        gettimeofday(&tv, NULL);
        aura_random_seed = (int64_t)tv.tv_sec * 1000000 + (int64_t)tv.tv_usec;
#endif
        srand((unsigned)aura_random_seed);
    }
}

int64_t aura_random_nextInt(void) {
    aura_random_init();
    return (int64_t)(rand() * (RAND_MAX / 32768.0));
}

double aura_random_nextFloat(void) {
    aura_random_init();
    return (double)rand() / (double)RAND_MAX;
}
