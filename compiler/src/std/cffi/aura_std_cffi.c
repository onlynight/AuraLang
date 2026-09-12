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

/* 分配一份新的、以 NUL 结尾的字符串副本。
 *
 * 为什么不能用共享 static 缓冲：AOT 字符串是「值语义」，返回值可能被调用方
 * 长期持有（如词法器字段 `src`、解析器缓存的 token 文本）。若返回 static
 * 缓冲，后续任意一次字符串运算都会就地覆盖此前所有「字符串」的内容，表现为
 * 字段读出来变成别的文本甚至空串。代价是这些副本不会被释放（AOT 运行时暂无
 * 字符串 GC）。 */
static char *aura_dup_n(const char *s, size_t n) {
    char *out = (char *)malloc(n + 1);
    if (!out) return (char *)"";
    if (s && n > 0) memcpy(out, s, n);
    out[n] = '\0';
    return out;
}

const char *aura_to_str(int64_t x) {
    char *buf = (char *)malloc(64);
    if (!buf) return "";
    snprintf(buf, 64, "%lld", (long long)x);
    return buf;
}

const char *aura_to_str_float(double x) {
    char *buf = (char *)malloc(64);
    if (!buf) return "";
    snprintf(buf, 64, "%g", x);
    return buf;
}

/* `toStr(Boolean)` / `toString(Boolean)` 的专用实现。
 *
 * VM 语义：true → "true"，false → "false"。
 * AOT 下布尔与整数共用 Plan A 的 `i8*` 装箱通道（true 会编码成 -1），
 * 若直接走 aura_to_str_any 会打印成 "-1"/"0" —— 与 VM 行为不一致。
 * emit_call 对 `toStr`/`toString` 的 i1 实参改派到这里。 */
const char *aura_to_str_bool(int64_t v) {
    return v ? "true" : "false";
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
double aura_math_sqrt(double x) { return sqrt(x); }
double aura_math_cbrt(double x) { return cbrt(x); }
double aura_math_pow(double base, double exp) { return pow(base, exp); }
double aura_math_round(double x) { return round(x); }
double aura_math_trunc(double x) { return trunc(x); }
double aura_math_log2(double x) { return log2(x); }
double aura_math_log10(double x) { return log10(x); }
double aura_math_sign(double x) { return (x > 0) - (x < 0); }
double aura_math_clamp(double x, double lo, double hi) { return x < lo ? lo : (x > hi ? hi : x); }

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
    if (!s || idx < 0 || (size_t)idx >= strlen(s)) return "";
    return aura_dup_n(s + idx, 1);
}

const char *aura_string_substring(const char *s, int64_t start, int64_t end) {
    if (!s) return "";
    size_t len = strlen(s);
    if (start < 0) start = 0;
    if (end > (int64_t)len) end = len;
    if (start > end) return "";
    return aura_dup_n(s + start, (size_t)(end - start));
}

const char *aura_string_toUpperCase(const char *s) {
    if (!s) return "";
    size_t len = strlen(s);
    char *buf = (char *)malloc(len + 1);
    if (!buf) return "";
    for (size_t i = 0; i < len; i++) {
        buf[i] = (char)toupper((unsigned char)s[i]);
    }
    buf[len] = '\0';
    return buf;
}

const char *aura_string_toLowerCase(const char *s) {
    if (!s) return "";
    size_t len = strlen(s);
    char *buf = (char *)malloc(len + 1);
    if (!buf) return "";
    for (size_t i = 0; i < len; i++) {
        buf[i] = (char)tolower((unsigned char)s[i]);
    }
    buf[len] = '\0';
    return buf;
}

const char *aura_string_trim(const char *s) {
    if (!s) return "";
    size_t len = strlen(s);
    if (len == 0) return "";
    // 找起始非空白
    size_t start = 0;
    while (start < len && isspace((unsigned char)s[start])) start++;
    // 找结束非空白
    size_t end = len;
    while (end > start && isspace((unsigned char)s[end - 1])) end--;
    return aura_dup_n(s + start, end - start);
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
    static char buf[65536];
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
    // 拷贝到新分配内存返回（static 仅作本次调用的临时缓冲）
    return aura_dup_n(buf, j);
}

// ─────────────────────────────────────────────────────────────────────────────
// AOT 字符串操作（{ i8*, i64 } 结构体表示）
// ─────────────────────────────────────────────────────────────────────────────

const char *aura_string_concat(const char *a, int64_t alen, const char *b, int64_t blen) {
    size_t a_len = (size_t)(alen > 0 ? alen : 0);
    size_t b_len = (size_t)(blen > 0 ? blen : 0);

    // 必须返回新分配内存：同一表达式内的链式拼接（`a + b + c`）会把前一步的
    // 结果再当作输入，若返回共享 static 缓冲则前后互相覆盖。
    char *out = (char *)malloc(a_len + b_len + 1);
    if (!out) return "";
    if (a && a_len > 0) {
        memcpy(out, a, a_len);
    }
    if (b && b_len > 0) {
        memcpy(out + a_len, b, b_len);
    }
    out[a_len + b_len] = '\0';
    return out;
}

const char *aura_string_data(AuraString s) {
    return s.data ? s.data : "";
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

// ─────────────────────────────────────────────────────────────────────────────
// 短名称包装函数（供 AOT IR 直接调用）
// 注意：避免与标准库函数冲突，使用 aura_ 前缀的内部调用
// ─────────────────────────────────────────────────────────────────────────────

void println(const char *s) { aura_println(s); }
void print(const char *s) { aura_print(s); }
int64_t aura_abs_wrapper(int64_t x) { return x < 0 ? -x : x; }
double aura_sqrt_wrapper(double x) { return sqrt(x); }
double aura_pow_wrapper(double b, double e) { return pow(b, e); }
int64_t toInt(double x) { return (int64_t)x; }
double toFloat(int64_t x) { return (double)x; }
/* Plan A 助手的前向声明：必须先于 toString 声明，否则 C 会按「隐式声明返回 int」
   处理，把 64 位指针截断成 32 位，导致返回的字符串指针被破坏。 */
int64_t aura_to_int_any(uint64_t v);
const char *aura_to_str_any(uint64_t v);

/* toString 同样按 Plan A 解析：调用点传入的是低位标记的 i8*
   （带标记的整数 → 十进制串；偶数真实指针 → 原样）。 */
const char *toString(int64_t x) { return aura_to_str_any((uint64_t)x); }

/* ── Plan A：低位标记（low-bit tagged）的统一值表示 ──
 * AOT 下列表/Any 只有 64 位槽，整数经 inttoptr 装入后会与真实指针无法区分，
 * 读回时若误按字符串 strlen 会对非法指针解引用 → 访问违规崩溃。
 * 约定：
 *   - 奇数 ((v<<1)|1)  : 装箱的整数 v
 *   - 偶数             : 真实指针（字符串数据指针等），按指针原义使用
 * 分配器返回的指针至少 2 字节对齐（实际通常 8/16），故真实指针低位恒为 0。
 * 这样两个 helper 对「真实指针」的行为与旧代码完全一致（无回归）。 */
int64_t aura_to_int_any(uint64_t v) {
    if (v & (uint64_t)1) {
        /* 带标记的整数：用算术右移恢复符号 */
        return (int64_t)((int64_t)v >> 1);
    }
    /* 偶数：旧语义（raw inttoptr 的值或真实指针），原样返回 */
    return (int64_t)v;
}

const char *aura_to_str_any(uint64_t v) {
    if (v & (uint64_t)1) {
        return aura_to_str((int64_t)((int64_t)v >> 1));
    }
    /* 偶数：真实字符串数据指针 */
    return (const char *)(uintptr_t)v;
}
/* `toStr` 与 `toString` 语义一致：AOT 下调用点会把整数经 inttoptr 打成 i8* 句柄，
 * 指针与 int64 在 x86-64 上均用整数寄存器传递，故 ABI 兼容，此处直接按整数解释。 */
/* toStr 接收的是「已按 Plan A 装箱」的 i8*（低位标记），故必须经 aura_to_str_any 解析：
   带标记的整数 → 十进制串；偶数（真实字符串指针）→ 原样返回。
   （注：toString 仍保持原始语义，用于调用点现场 inttoptr 的裸整数。） */
const char *toStr(int64_t x) { return aura_to_str_any((uint64_t)x); }
double aura_clock_wrapper(void) { return (double)clock(); }
int64_t aura_strlen_wrapper(const char *s) { return (int64_t)strlen(s); }
const char *toStringFloat(double x) { return aura_to_str_float(x); }

// aura.isOfType(value, typeName) → i1：检查值的运行时类型是否匹配目标类型名
// value: i8* (值指针), typeName: { i8*, i64 } (类型名字符串结构体)
// 简化实现：将值指针的第一个 4 字节视为类型标签，与目标类型名比较
_Bool aura_isOfType(const void *value, const AuraString *typeName) {
    if (!value || !typeName || !typeName->data) return 0;
    // 将值视为带类型标签的对象：第一个 8 字节存储类型标签指针
    const char *value_type_name = *(const char *const *)value;
    if (!value_type_name) return 0;
    size_t len = typeName->len > 0 ? (size_t)typeName->len : 0;
    return strncmp(value_type_name, typeName->data, len) == 0 && value_type_name[len] == '\0';
}

// __throw(value) → void：打印异常值到 stderr（AOT throw 表达式支持）
// value: i8* (异常值指针)
void __throw(const void *value) {
    if (value) {
        // 尝试将值视为字符串指针并输出
        const char *msg = (const char *)value;
        fprintf(stderr, "[throw] %s\n", msg);
        fflush(stderr);
    } else {
        fprintf(stderr, "[throw] null\n");
        fflush(stderr);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// P8: 并发运行时（aura.concurrent.*）— AOT 原生支持
// 语义：单线程等价实现。spawn/spawnActor 返回递增 ID；Channel 为无界 FIFO 队列；
//       select 返回第一个非空通道的队首值。真协程/多线程调度由 VM 运行时承担。
// ─────────────────────────────────────────────────────────────────────────────

static int64_t aura_concurrent_next_id = 0;

#define AURA_CH_CAP 256

typedef struct {
    int64_t buf[AURA_CH_CAP];
    int head;
    int tail;
    int count;
} AuraChannel;

static AuraChannel aura_channels[64];
static int aura_channel_count = 0;

static void aura_ch_push(int32_t ch, int64_t v) {
    if (ch < 1 || ch > aura_channel_count) return;
    AuraChannel *c = &aura_channels[ch - 1];
    if (c->count >= AURA_CH_CAP) return;
    c->buf[c->tail] = v;
    c->tail = (c->tail + 1) % AURA_CH_CAP;
    c->count++;
}

static int64_t aura_ch_pop(int32_t ch) {
    if (ch < 1 || ch > aura_channel_count) return 0;
    AuraChannel *c = &aura_channels[ch - 1];
    if (c->count == 0) return 0;
    int64_t v = c->buf[c->head];
    c->head = (c->head + 1) % AURA_CH_CAP;
    c->count--;
    return v;
}

// spawn(value: Any) → Int：创建协程，返回协程 ID
// AOT 无协程运行时：数值负载直接回传（与 VM 语义对齐），其余返回递增 ID
int32_t aura_concurrent_spawn(const void *value) {
    if (value) {
        intptr_t v = (intptr_t)value;
        if (v > 0 && v < 0x10000) return (int32_t)v;
    }
    return (int32_t)(++aura_concurrent_next_id);
}

// send(actor: Int, msg: Any) → Unit：向 Actor 发送消息（AOT 下记录 ID，不排队）
void aura_concurrent_send(int32_t actor, const void *msg) {
    (void)actor;
    (void)msg;
}

// ask(actor: Int, msg: Any) → Any：请求响应（无运行时，返回 null）
const void *aura_concurrent_ask(int32_t actor, const void *msg) {
    (void)actor;
    (void)msg;
    return 0;
}

// newChannel(bound: Int) → Int：创建 Channel，返回句柄
int32_t aura_concurrent_newChannel(int32_t bound) {
    (void)bound;
    if (aura_channel_count >= 64) return 0;
    aura_channel_count++;
    return aura_channel_count;
}

// channelSend(ch: Int, val: Any) → Unit
void aura_concurrent_channelSend(int32_t ch, const void *val) {
    aura_ch_push(ch, (int64_t)(intptr_t)val);
}

// channelRecv(ch: Int) → Any（空通道返回 null）
const void *aura_concurrent_channelRecv(int32_t ch) {
    return (const void *)(intptr_t)aura_ch_pop(ch);
}

// channelTryRecv(ch: Int) → Any
const void *aura_concurrent_channelTryRecv(int32_t ch) {
    return (const void *)(intptr_t)aura_ch_pop(ch);
}

// select(ch1: Int, ch2: Int) → Any：第一个非空通道的队首值
const void *aura_concurrent_select(int32_t ch1, int32_t ch2) {
    const void *v = (const void *)(intptr_t)aura_ch_pop(ch1);
    if (v) return v;
    return (const void *)(intptr_t)aura_ch_pop(ch2);
}

// spawnActor(name: String) → Int：创建 Actor，返回 ID
int32_t aura_concurrent_spawnActor(AuraString name) {
    (void)name;
    return (int32_t)(++aura_concurrent_next_id);
}

// supervise(parent: Int, child: Int) → Unit
void aura_concurrent_supervise(int32_t parent, int32_t child) {
    (void)parent;
    (void)child;
}

// actorAlive(id: Int) → Boolean
_Bool aura_concurrent_actorAlive(int32_t id) {
    return id > 0;
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 2: Runtime 运行时函数（ARC / 内存 / 协程 / 字符串）
// 通过 AOT FFI 直连 libc，获得接近原生的性能
// ─────────────────────────────────────────────────────────────────────────────

// ARC 引用计数结构体（对象头部）
typedef struct {
    volatile int32_t refcount;
    // 对象数据紧跟其后
} AuraArcHeader;

// ARC 引用计数 +1（原子操作）
// ptr: 对象指针（指向 AuraArcHeader）
void aura_arc_increment(const void *ptr) {
    if (!ptr) return;
    const AuraArcHeader *header = (const AuraArcHeader *)ptr;
#ifdef _WIN32
    // Windows: InterlockedIncrement
    InterlockedIncrement((volatile LONG*)(&header->refcount));
#else
    // POSIX: __sync_fetch_and_add
    __sync_fetch_and_add(&header->refcount, 1);
#endif
}

// ARC 引用计数 -1，归零时释放（原子操作）
void aura_arc_decrement(const void *ptr) {
    if (!ptr) return;
    AuraArcHeader *header = (AuraArcHeader *)ptr;
    int32_t old_count;
#ifdef _WIN32
    old_count = InterlockedDecrement((volatile LONG*)(&header->refcount));
#else
    old_count = __sync_sub_and_fetch(&header->refcount, 1);
#endif
    if (old_count <= 0) {
        // 引用计数归零，释放对象内存
        // 注意：这里假设对象是通过 aura_malloc 分配的
        aura_free(ptr);
    }
}

// 协程挂起（AOT 下为 no-op，VM 运行时处理）
// ctx: 协程上下文指针
void aura_coroutine_yield(const void *ctx) {
    (void)ctx;
    // AOT 下协程由 VM 运行时管理，此处为空操作
}

// 堆分配（返回指针）
void *aura_malloc(int64_t size) {
    if (size <= 0) return NULL;
    return malloc((size_t)size);
}

// 堆释放
void aura_free(const void *ptr) {
    if (ptr) free((void *)ptr);
}

// 创建字符串对象（返回字符串指针）
// data: 字符串数据指针，len: 字符串长度
const char *aura_string_new(const char *data, int64_t len) {
    if (!data || len <= 0) return "";
    return aura_dup_n(data, (size_t)len);
}

// 注：`aura_string_length` / `aura_string_data` 已在文件前部以 `const char*`
// 形参形式定义（AOT 发射器按 i8* 指针调用）；此处不再定义 `AuraString*` 版本，
// 以避免同名函数签名冲突（此前重复定义导致 clang 报 conflicting types）。

// ─────────────────────────────────────────────────────────────────────────────
// P13: 标准库快照（AOT C 实现）— math/ascii/collections/time/random/encoding/
//      path/env/fs 中语言测试用到的子集。字符串参数均为 const char*（数据指针）。
// ─────────────────────────────────────────────────────────────────────────────

// aura.math
int64_t aura_math_abs(double x) { return (int64_t)fabs(x); }

// aura.ascii（Char 以 i16 传入）
_Bool aura_ascii_isAlpha(int16_t c) { return isalpha((unsigned char)c) != 0; }
_Bool aura_ascii_isDigit(int16_t c) { return isdigit((unsigned char)c) != 0; }
_Bool aura_ascii_isAlphaNumeric(int16_t c) { return isalnum((unsigned char)c) != 0; }
_Bool aura_ascii_isWhitespace(int16_t c) { return isspace((unsigned char)c) != 0; }
_Bool aura_ascii_isUpper(int16_t c) { return isupper((unsigned char)c) != 0; }
_Bool aura_ascii_isLower(int16_t c) { return islower((unsigned char)c) != 0; }
int64_t aura_ascii_toUpper(int16_t c) { return (int64_t)toupper((unsigned char)c); }
int64_t aura_ascii_toLower(int16_t c) { return (int64_t)tolower((unsigned char)c); }
int64_t aura_ascii_codeAt(int16_t c) { return (int64_t)c; }

// aura.collections — 静态注册表：listOf 注册元素（值均为 Any→i8* 指针/整数），
// listContains/listIndexOf 按指针值（整数语义）比较
#define AURA_LIST_MAX 32
#define AURA_LIST_ELEMS 64
typedef struct {
    int64_t items[AURA_LIST_ELEMS];
    int count;
} AuraList;
static AuraList aura_lists[AURA_LIST_MAX];
static int aura_list_count = 0;

const void *aura_collections_listOf(const void *a, const void *b, const void *c) {
    if (aura_list_count >= AURA_LIST_MAX) return 0;
    AuraList *l = &aura_lists[aura_list_count++];
    l->count = 0;
    if (a) l->items[l->count++] = (int64_t)(intptr_t)a;
    if (b) l->items[l->count++] = (int64_t)(intptr_t)b;
    if (c) l->items[l->count++] = (int64_t)(intptr_t)c;
    return (const void *)l;
}

_Bool aura_collections_listContains(const void *list, const void *val) {
    const AuraList *l = (const AuraList *)list;
    if (!l || !val) return 0;
    int64_t v = (int64_t)(intptr_t)val;
    for (int i = 0; i < l->count; i++)
        if (l->items[i] == v) return 1;
    return 0;
}

int64_t aura_collections_listIndexOf(const void *list, const void *val) {
    const AuraList *l = (const AuraList *)list;
    if (!l || !val) return -1;
    int64_t v = (int64_t)(intptr_t)val;
    for (int i = 0; i < l->count; i++)
        if (l->items[i] == v) return i;
    return -1;
}

// ── 特化集合（ArrayList / LinkedList / HashSet / HashMap / LinkedHashMap）──
// 复用 AuraList 结构体；Map 用 AuraMap（键值对数组 + 顺序数组）

#define AURA_MAP_MAX 32
#define AURA_MAP_ELEMS 16
typedef struct {
    int64_t keys[AURA_MAP_ELEMS];
    int64_t values[AURA_MAP_ELEMS];
    int64_t order[AURA_MAP_ELEMS]; // 插入顺序的键
    int count;
} AuraMap;
static AuraMap aura_maps[AURA_MAP_MAX];
static int aura_map_count = 0;

// 辅助：从 AuraList 创建新列表
static AuraList *new_list(int64_t *items, int count) {
    if (aura_list_count >= AURA_LIST_MAX) return 0;
    AuraList *l = &aura_lists[aura_list_count++];
    l->count = 0;
    for (int i = 0; i < count && i < AURA_LIST_ELEMS; i++) {
        l->items[l->count++] = items[i];
    }
    return l;
}

// ArrayList
const void *aura_collections_arrayListOf(const void *a, const void *b, const void *c,
    const void *d, const void *e, const void *f, const void *g,
    const void *h, const void *i, const void *j) {
    const void *args[] = {a, b, c, d, e, f, g, h, i, j};
    int64_t items[AURA_LIST_ELEMS];
    int count = 0;
    for (int k = 0; k < 10 && args[k]; k++)
        items[count++] = (int64_t)(intptr_t)args[k];
    AuraList *l = new_list(items, count);
    return (const void *)l;
}

int64_t aura_collections_arrayListSize(const void *list) {
    const AuraList *l = (const AuraList *)list;
    return l ? l->count : 0;
}

// LinkedList
const void *aura_collections_linkedListOf(const void *a, const void *b, const void *c,
    const void *d, const void *e, const void *f, const void *g,
    const void *h, const void *i, const void *j) {
    return aura_collections_arrayListOf(a, b, c, d, e, f, g, h, i, j);
}

const void *aura_collections_linkedAddFirst(const void *list, const void *value) {
    if (!list) {
        int64_t items[1] = {value ? (int64_t)(intptr_t)value : 0};
        return (const void *)new_list(items, 1);
    }
    const AuraList *l = (const AuraList *)list;
    int64_t items[AURA_LIST_ELEMS];
    int count = 0;
    if (value) items[count++] = (int64_t)(intptr_t)value;
    for (int k = 0; k < l->count && count < AURA_LIST_ELEMS; k++)
        items[count++] = l->items[k];
    return (const void *)new_list(items, count);
}

const void *aura_collections_linkedAddLast(const void *list, const void *value) {
    if (!list) {
        int64_t items[1] = {value ? (int64_t)(intptr_t)value : 0};
        return (const void *)new_list(items, 1);
    }
    const AuraList *l = (const AuraList *)list;
    int64_t items[AURA_LIST_ELEMS];
    int count = 0;
    for (int k = 0; k < l->count && count < AURA_LIST_ELEMS - 1; k++)
        items[count++] = l->items[k];
    if (value) items[count++] = (int64_t)(intptr_t)value;
    return (const void *)new_list(items, count);
}

const void *aura_collections_linkedRemoveFirst(const void *list) {
    if (!list) return 0;
    const AuraList *l = (const AuraList *)list;
    if (l->count == 0) return (const void *)new_list(NULL, 0);
    int64_t items[AURA_LIST_ELEMS];
    int count = 0;
    for (int k = 1; k < l->count && count < AURA_LIST_ELEMS; k++)
        items[count++] = l->items[k];
    return (const void *)new_list(items, count);
}

const void *aura_collections_linkedRemoveLast(const void *list) {
    if (!list) return 0;
    const AuraList *l = (const AuraList *)list;
    if (l->count == 0) return (const void *)new_list(NULL, 0);
    int64_t items[AURA_LIST_ELEMS];
    int count = 0;
    for (int k = 0; k < l->count - 1 && count < AURA_LIST_ELEMS; k++)
        items[count++] = l->items[k];
    return (const void *)new_list(items, count);
}

// HashSet
const void *aura_collections_hashSetOf(const void *a, const void *b, const void *c,
    const void *d, const void *e, const void *f, const void *g,
    const void *h, const void *i, const void *j) {
    const void *args[] = {a, b, c, d, e, f, g, h, i, j};
    int64_t items[AURA_LIST_ELEMS];
    int count = 0;
    for (int k = 0; k < 10 && args[k]; k++) {
        int64_t v = (int64_t)(intptr_t)args[k];
        int dup = 0;
        for (int m = 0; m < count; m++) {
            if (items[m] == v) { dup = 1; break; }
        }
        if (!dup && count < AURA_LIST_ELEMS) items[count++] = v;
    }
    return (const void *)new_list(items, count);
}

_Bool aura_collections_hashSetContains(const void *set, const void *item) {
    if (!set || !item) return 0;
    const AuraList *l = (const AuraList *)set;
    int64_t v = (int64_t)(intptr_t)item;
    for (int k = 0; k < l->count; k++)
        if (l->items[k] == v) return 1;
    return 0;
}

const void *aura_collections_hashSetAdd(const void *set, const void *item) {
    if (!item) return set;
    int64_t v = (int64_t)(intptr_t)item;
    if (set) {
        const AuraList *l = (const AuraList *)set;
        int exists = 0;
        for (int k = 0; k < l->count; k++)
            if (l->items[k] == v) { exists = 1; break; }
        if (exists) return set;
        int64_t items[AURA_LIST_ELEMS];
        int count = 0;
        for (int k = 0; k < l->count && count < AURA_LIST_ELEMS - 1; k++)
            items[count++] = l->items[k];
        items[count++] = v;
        return (const void *)new_list(items, count);
    }
    int64_t items[1] = {v};
    return (const void *)new_list(items, 1);
}

_Bool aura_collections_hashSetRemove(const void *set, const void *item) {
    if (!set || !item) return 0;
    const AuraList *l = (const AuraList *)set;
    int64_t v = (int64_t)(intptr_t)item;
    int found = 0;
    int64_t items[AURA_LIST_ELEMS];
    int count = 0;
    for (int k = 0; k < l->count; k++) {
        if (l->items[k] == v) found = 1;
        else if (count < AURA_LIST_ELEMS) items[count++] = l->items[k];
    }
    if (found) (void)new_list(items, count); // 新列表已分配，但调用者不使用返回值
    return (_Bool)found;
}

// HashMap
const void *aura_collections_hashMapOf(const void *k0, const void *v0,
    const void *k1, const void *v1, const void *k2, const void *v2,
    const void *k3, const void *v3, const void *k4, const void *v4) {
    if (aura_map_count >= AURA_MAP_MAX) return 0;
    AuraMap *m = &aura_maps[aura_map_count++];
    m->count = 0;
    // 参数表（键值对）
    const void *keys[] = {k0, k1, k2, k3, k4};
    const void *vals[] = {v0, v1, v2, v3, v4};
    for (int k = 0; k < 5 && keys[k]; k++) {
        int64_t key = (int64_t)(intptr_t)keys[k];
        int64_t val = vals[k] ? (int64_t)(intptr_t)vals[k] : 0;
        // 检查是否已存在（覆盖）
        int found = 0;
        for (int j = 0; j < m->count; j++) {
            if (m->keys[j] == key) {
                m->values[j] = val;
                found = 1;
                break;
            }
        }
        if (!found && m->count < AURA_MAP_ELEMS) {
            m->keys[m->count] = key;
            m->values[m->count] = val;
            m->order[m->count] = key;
            m->count++;
        }
    }
    return (const void *)m;
}

const void *aura_collections_hashMapGet(const void *map, const void *key) {
    if (!map || !key) return 0;
    const AuraMap *m = (const AuraMap *)map;
    int64_t k = (int64_t)(intptr_t)key;
    for (int j = 0; j < m->count; j++)
        if (m->keys[j] == k) return (const void *)(intptr_t)m->values[j];
    return 0;
}

const void *aura_collections_hashMapPut(const void *map, const void *key, const void *value) {
    if (!key) return map;
    AuraMap *m = (AuraMap *)map;
    if (!m) {
        if (aura_map_count >= AURA_MAP_MAX) return 0;
        m = &aura_maps[aura_map_count++];
        m->count = 0;
    }
    int64_t k = (int64_t)(intptr_t)key;
    int64_t v = value ? (int64_t)(intptr_t)value : 0;
    for (int j = 0; j < m->count; j++) {
        if (m->keys[j] == k) {
            m->values[j] = v;
            return (const void *)m;
        }
    }
    if (m->count < AURA_MAP_ELEMS) {
        m->keys[m->count] = k;
        m->values[m->count] = v;
        m->order[m->count] = k;
        m->count++;
    }
    return (const void *)m;
}

const void *aura_collections_hashMapRemove(const void *map, const void *key) {
    if (!map || !key) return 0;
    const AuraMap *m = (const AuraMap *)map;
    int64_t k = (int64_t)(intptr_t)key;
    for (int j = 0; j < m->count; j++) {
        if (m->keys[j] == k) {
            // 创建新 map 排除该键
            if (aura_map_count >= AURA_MAP_MAX) return 0;
            AuraMap *nm = &aura_maps[aura_map_count++];
            nm->count = 0;
            for (int n = 0; n < m->count; n++) {
                if (m->keys[n] != k) {
                    nm->keys[nm->count] = m->keys[n];
                    nm->values[nm->count] = m->values[n];
                    nm->order[nm->count] = m->keys[n];
                    nm->count++;
                }
            }
            return (const void *)nm;
        }
    }
    return 0;
}

// LinkedHashMap（复用 AuraMap，保持插入顺序）
const void *aura_collections_linkedHashMapOf(const void *k0, const void *v0,
    const void *k1, const void *v1, const void *k2, const void *v2,
    const void *k3, const void *v3, const void *k4, const void *v4) {
    return aura_collections_hashMapOf(k0, v0, k1, v1, k2, v2, k3, v3, k4, v4);
}

const void *aura_collections_linkedHashMapKeys(const void *map) {
    if (!map) return (const void *)new_list(NULL, 0);
    const AuraMap *m = (const AuraMap *)map;
    int64_t items[AURA_LIST_ELEMS];
    int count = 0;
    for (int j = 0; j < m->count && count < AURA_LIST_ELEMS; j++)
        items[count++] = m->order[j];
    return (const void *)new_list(items, count);
}

const void *aura_collections_linkedHashMapFirstKey(const void *map) {
    if (!map) return 0;
    const AuraMap *m = (const AuraMap *)map;
    return m->count > 0 ? (const void *)(intptr_t)m->order[0] : 0;
}

const void *aura_collections_linkedHashMapLastKey(const void *map) {
    if (!map) return 0;
    const AuraMap *m = (const AuraMap *)map;
    return m->count > 0 ? (const void *)(intptr_t)m->order[m->count - 1] : 0;
}

// aura.time / aura.random — 已有实现（aura_time_epoch / aura_time_epochMillis / aura_random_nextInt）

// aura.encoding — base64
static const char *B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
static char aura_b64_buf[512];

const char *aura_encoding_base64Encode(const char *s) {
    if (!s) return "";
    size_t len = strlen(s), o = 0;
    for (size_t i = 0; i < len && o + 4 < sizeof(aura_b64_buf); i += 3) {
        uint32_t n = (uint8_t)s[i] << 16;
        if (i + 1 < len) n |= (uint8_t)s[i + 1] << 8;
        if (i + 2 < len) n |= (uint8_t)s[i + 2];
        aura_b64_buf[o++] = B64[(n >> 18) & 63];
        aura_b64_buf[o++] = B64[(n >> 12) & 63];
        aura_b64_buf[o++] = (i + 1 < len) ? B64[(n >> 6) & 63] : '=';
        aura_b64_buf[o++] = (i + 2 < len) ? B64[n & 63] : '=';
    }
    aura_b64_buf[o] = '\0';
    return aura_b64_buf;
}

static int b64_val(char c) {
    if (c >= 'A' && c <= 'Z') return c - 'A';
    if (c >= 'a' && c <= 'z') return c - 'a' + 26;
    if (c >= '0' && c <= '9') return c - '0' + 52;
    if (c == '+') return 62;
    if (c == '/') return 63;
    return -1;
}

const char *aura_encoding_base64Decode(const char *s) {
    if (!s) return "";
    size_t o = 0;
    for (size_t i = 0; s[i] && o + 3 < sizeof(aura_b64_buf);) {
        int a = b64_val(s[i++]);
        if (a < 0) break;
        int b = (s[i] && s[i] != '=') ? b64_val(s[i++]) : (i++, -1);
        int c = (s[i] && s[i] != '=') ? b64_val(s[i++]) : (i++, -1);
        int d = (s[i] && s[i] != '=') ? b64_val(s[i++]) : (i++, -1);
        if (b < 0) break;
        aura_b64_buf[o++] = (char)((a << 2) | (b >> 4));
        if (c >= 0) aura_b64_buf[o++] = (char)(((b & 15) << 4) | (c >> 2));
        if (d >= 0) aura_b64_buf[o++] = (char)(((c & 3) << 6) | d);
    }
    aura_b64_buf[o] = '\0';
    return aura_b64_buf;
}

// aura.path（Windows 分隔符）
static char aura_path_buf[512];

const char *aura_path_join(const char *a, const char *b) {
    snprintf(aura_path_buf, sizeof(aura_path_buf), "%s\\%s", a ? a : "", b ? b : "");
    return aura_path_buf;
}

const char *aura_path_basename(const char *p) {
    if (!p) return "";
    const char *s1 = strrchr(p, '/');
    const char *s2 = strrchr(p, '\\');
    const char *s = (s1 && s2) ? (s1 > s2 ? s1 : s2) : (s1 ? s1 : s2);
    return s ? s + 1 : p;
}

const char *aura_path_dirname(const char *p) {
    if (!p) return "";
    snprintf(aura_path_buf, sizeof(aura_path_buf), "%s", p);
    char *s1 = strrchr(aura_path_buf, '/');
    char *s2 = strrchr(aura_path_buf, '\\');
    char *s = (s1 && s2) ? (s1 > s2 ? s1 : s2) : (s1 ? s1 : s2);
    if (s) *s = '\0';
    return aura_path_buf;
}

// aura.env
const char *aura_env_platform(void) {
#ifdef _WIN32
    return "windows";
#elif defined(__APPLE__)
    return "macos";
#elif defined(__linux__)
    return "linux";
#else
    return "unknown";
#endif
}

const char *aura_env_os(void) { return aura_env_platform(); }
const char *aura_env_arch(void) { return "x86_64"; }

const char *aura_env_get(const char *name) {
    static char env_buf[512];
    const char *v = getenv(name ? name : "");
    if (!v) return "";
    snprintf(env_buf, sizeof(env_buf), "%s", v);
    return env_buf;
}

/* ═══════════════════════════════════════════════════════════════════════════
 * AOT 调用点符号层（sanitize(aura.lang.std.X.y)）
 *
 * AOT 发射器把 `aura.lang.std.String.split` 这类调用落成 LLVM 符号
 * `aura_lang_std_String_split`（见 codegen/aot/emit.rs 的 sanitizellvm）。
 * 本文件历史上只导出 `aura_string_*` 旧名，导致除 prelude 外的 std 调用
 * 在链接期全部报 undefined symbol。本节按「调用点符号」导出，
 * 转发到既有实现或给出实现，使 AOT 产物可以真正链接。
 * ═══════════════════════════════════════════════════════════════════════════ */

/* ── 极简动态列表：元素为字符串指针（AOT 下 List<T> 映射为 i8*） ── */

typedef struct {
    int64_t len;
    int64_t cap;
    const char **items;
} AuraDynList;

static AuraDynList *aura_dynlist_new(int64_t cap) {
    AuraDynList *l = (AuraDynList *)malloc(sizeof(AuraDynList));
    if (!l) return NULL;
    if (cap < 4) cap = 4;
    l->len = 0;
    l->cap = cap;
    l->items = (const char **)malloc(sizeof(const char *) * (size_t)cap);
    return l;
}

static void aura_dynlist_push(AuraDynList *l, const char *s) {
    if (!l) return;
    if (l->len >= l->cap) {
        int64_t ncap = l->cap * 2;
        const char **ni =
            (const char **)realloc((void *)l->items, sizeof(const char *) * (size_t)ncap);
        if (!ni) return;
        l->items = ni;
        l->cap = ncap;
    }
    l->items[l->len++] = s ? s : "";
}

/** 左闭右开的 substr 到新分配的 C 字符串 */
static const char *aura_substr_dup(const char *s, size_t n) {
    char *out = (char *)malloc(n + 1);
    if (!out) return "";
    if (s && n > 0) memcpy(out, s, n);
    out[n] = '\0';
    return out;
}

/* ── aura.lang.std.String.* ── */

int64_t aura_lang_std_String_length(const char *s) {
    return aura_string_length(s);
}

int aura_lang_std_String_contains(const char *s, const char *sub) {
    return aura_string_contains(s, sub);
}

int aura_lang_std_String_startsWith(const char *s, const char *prefix) {
    return aura_string_startsWith(s, prefix);
}

int aura_lang_std_String_endsWith(const char *s, const char *suffix) {
    return aura_string_endsWith(s, suffix);
}

const char *aura_lang_std_String_substring(const char *s, int64_t start, int64_t end) {
    return aura_string_substring(s, start, end);
}

const char *aura_lang_std_String_charAt(const char *s, int64_t idx) {
    return aura_string_charAt(s, idx);
}

const char *aura_lang_std_String_trim(const char *s) {
    return aura_string_trim(s);
}

const char *aura_lang_std_String_toUpperCase(const char *s) {
    return aura_string_toUpperCase(s);
}

const char *aura_lang_std_String_toLowerCase(const char *s) {
    return aura_string_toLowerCase(s);
}

const char *aura_lang_std_String_replace(const char *s, const char *from, const char *to) {
    return aura_string_replace(s, from, to);
}

/** replaceAll：与 replace 同语义（C 实现已做全量替换） */
const char *aura_lang_std_String_replaceAll(const char *s, const char *from, const char *to) {
    return aura_string_replace(s, from, to);
}

int64_t aura_lang_std_String_indexOf(const char *s, const char *sub) {
    if (!s || !sub) return -1;
    const char *p = strstr(s, sub);
    return p ? (int64_t)(p - s) : -1;
}

int64_t aura_lang_std_String_lastIndexOf(const char *s, const char *sub) {
    if (!s || !sub) return -1;
    size_t sl = strlen(s);
    size_t bl = strlen(sub);
    if (bl > sl) return -1;
    for (size_t i = sl - bl + 1; i > 0; i--) {
        if (strncmp(s + i - 1, sub, bl) == 0) return (int64_t)(i - 1);
    }
    return -1;
}

int64_t aura_lang_std_String_countChar(const char *s, const char *ch) {
    if (!s || !ch || !ch[0]) return 0;
    char c = ch[0];
    int64_t n = 0;
    for (const char *p = s; *p; p++) {
        if (*p == c) n++;
    }
    return n;
}

const char *aura_lang_std_String_substringBefore(const char *s, const char *sep) {
    if (!s || !sep || !sep[0]) return s ? s : "";
    const char *p = strstr(s, sep);
    if (!p) return s;
    return aura_substr_dup(s, (size_t)(p - s));
}

const char *aura_lang_std_String_substringAfter(const char *s, const char *sep) {
    if (!s || !sep || !sep[0]) return "";
    const char *p = strstr(s, sep);
    if (!p) return "";
    p += strlen(sep);
    /* 必须返回新分配副本，不能返回 `p`（原串的内部指针）：
     * Plan A 低位标记方案依赖「真实字符串指针恒为偶数」来区分装箱整数
     * ((v<<1)|1)。内部指针的地址奇偶性不可控，可能被判成装箱整数，
     * 导致后续 aura_to_str_any 解出垃圾文本。 */
    return aura_substr_dup(p, strlen(p));
}

const char *aura_lang_std_String_padStart(const char *s, int64_t width, const char *pad) {
    if (!s) return "";
    size_t sl = strlen(s);
    if ((int64_t)sl >= width) return s;
    size_t pl = pad ? strlen(pad) : 0;
    if (pl == 0) return s;
    static char buf[4096];
    size_t need = (size_t)width;
    if (need >= sizeof(buf)) need = sizeof(buf) - 1;
    size_t fill = need - sl;
    size_t j = 0;
    while (j < fill) {
        buf[j] = pad[j % pl];
        j++;
    }
    memcpy(buf + fill, s, sl);
    buf[need] = '\0';
    return buf;
}

/** 按分隔符切分 → AuraDynList（元素为堆分配的 C 字符串） */
const void *aura_lang_std_String_split(const char *s, const char *sep) {
    AuraDynList *l = aura_dynlist_new(8);
    if (!s) {
        aura_dynlist_push(l, "");
        return (const void *)l;
    }
    if (!sep || !sep[0]) {
        aura_dynlist_push(l, s);
        return (const void *)l;
    }
    size_t seplen = strlen(sep);
    const char *start = s;
    const char *p;
    while ((p = strstr(start, sep)) != NULL) {
        aura_dynlist_push(l, aura_substr_dup(start, (size_t)(p - start)));
        start = p + seplen;
    }
    aura_dynlist_push(l, aura_substr_dup(start, strlen(start)));
    return (const void *)l;
}

/** 字符串内容相等（AOT 字符串比较统一走这里，避免结构体按位比较） */
int aura_lang_std_String_equals(const char *a, const char *b) {
    if (!a || !b) return a == b ? 1 : 0;
    return strcmp(a, b) == 0 ? 1 : 0;
}

/* ── 极简字符串键值表（`Map<String, Any>`；AOT 下 Map 表示为 i8*） ── */

static const char *aura_strdup(const char *s) {
    size_t n = strlen(s);
    char *out = (char *)malloc(n + 1);
    if (!out) return "";
    memcpy(out, s, n + 1);
    return out;
}

typedef struct {
    int64_t len;
    int64_t cap;
    const char **keys;
    const char **vals;
} AuraDynMap;

static AuraDynMap *aura_map_new(int64_t cap) {
    AuraDynMap *m = (AuraDynMap *)malloc(sizeof(AuraDynMap));
    if (!m) return NULL;
    if (cap < 4) cap = 4;
    m->len = 0;
    m->cap = cap;
    m->keys = (const char **)malloc(sizeof(const char *) * (size_t)cap);
    m->vals = (const char **)malloc(sizeof(const char *) * (size_t)cap);
    return m;
}

const void *aura_lang_std_Collections_mutableMapOf(void) {
    return (const void *)aura_map_new(8);
}

const void *aura_lang_std_Collections_emptyMap(void) {
    return (const void *)aura_map_new(4);
}

/** 就地写入（Map 为可变堆对象，调用方拿到的是同一指针） */
void aura_lang_std_Collections_mapSet(const void *map, const char *key, const void *value) {
    AuraDynMap *m = (AuraDynMap *)map;
    if (!m || !key) return;
    const char *v = value ? (const char *)value : "";
    for (int64_t i = 0; i < m->len; i++) {
        if (m->keys[i] && strcmp(m->keys[i], key) == 0) {
            m->vals[i] = v;
            return;
        }
    }
    if (m->len >= m->cap) {
        int64_t ncap = m->cap * 2;
        const char **nk =
            (const char **)realloc((void *)m->keys, sizeof(const char *) * (size_t)ncap);
        const char **nv =
            (const char **)realloc((void *)m->vals, sizeof(const char *) * (size_t)ncap);
        if (!nk || !nv) return;
        m->keys = nk;
        m->vals = nv;
        m->cap = ncap;
    }
    m->keys[m->len] = aura_strdup(key);
    m->vals[m->len] = v;
    m->len++;
}

const void *aura_lang_std_Collections_mapGet(const void *map, const char *key) {
    const AuraDynMap *m = (const AuraDynMap *)map;
    if (!m || !key) return "";
    for (int64_t i = 0; i < m->len; i++) {
        if (m->keys[i] && strcmp(m->keys[i], key) == 0) {
            return (const void *)m->vals[i];
        }
    }
    return "";
}

int64_t aura_lang_std_Collections_mapSize(const void *map) {
    const AuraDynMap *m = (const AuraDynMap *)map;
    return m ? m->len : 0;
}

int aura_lang_std_Collections_mapContains(const void *map, const char *key) {
    const AuraDynMap *m = (const AuraDynMap *)map;
    if (!m || !key) return 0;
    for (int64_t i = 0; i < m->len; i++) {
        if (m->keys[i] && strcmp(m->keys[i], key) == 0) return 1;
    }
    return 0;
}

/** 列表按下标写入（越界则追加） */
void aura_lang_std_Collections_listSet(const void *list, int64_t idx, const void *value) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l) return;
    const char *v = value ? (const char *)value : "";
    if (idx >= 0 && idx < l->len) {
        l->items[idx] = v;
    } else {
        aura_dynlist_push(l, v);
    }
}

/* ── aura.lang.std.Collections.* ── */

const void *aura_lang_std_Collections_emptyList(void) {
    return (const void *)aura_dynlist_new(4);
}

const void *aura_lang_std_Collections_listOf(const void *a, const void *b, const void *c) {
    AuraDynList *l = aura_dynlist_new(4);
    aura_dynlist_push(l, (const char *)a);
    aura_dynlist_push(l, (const char *)b);
    aura_dynlist_push(l, (const char *)c);
    return (const void *)l;
}

/** pairOf(a, b)：`to` 运算符 / Pair 构造。AOT 下 Pair 以 2 元素 AuraDynList 表示
 *  （与 VM 侧 `nat_pair_of` 的 2 元素 `Value::List` 语义一致），供 `val (x, y) = p`
 *  解构经 getAt(0)/getAt(1) 取回，保证 AOT 产物链接期不再缺符号。 */
const void *aura_lang_std_Collections_pairOf(const void *a, const void *b) {
    AuraDynList *l = aura_dynlist_new(4);
    aura_dynlist_push(l, (const char *)a);
    aura_dynlist_push(l, (const char *)b);
    return (const void *)l;
}

int64_t aura_lang_std_Collections_count(const void *list) {
    const AuraDynList *l = (const AuraDynList *)list;
    return l ? l->len : 0;
}

int64_t aura_lang_std_Collections_listSize(const void *list) {
    return aura_lang_std_Collections_count(list);
}

int64_t aura_lang_std_Collections_isEmpty(const void *list) {
    return aura_lang_std_Collections_count(list) == 0 ? 1 : 0;
}

const void *aura_lang_std_Collections_getAt(const void *list, int64_t idx) {
    const AuraDynList *l = (const AuraDynList *)list;
    if (!l || idx < 0 || idx >= l->len) return "";
    return (const void *)l->items[idx];
}

const void *aura_lang_std_Collections_listGet(const void *list, int64_t idx) {
    return aura_lang_std_Collections_getAt(list, idx);
}

const void *aura_lang_std_Collections_listAppend(const void *list, const void *value) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l) {
        l = aura_dynlist_new(4);
    }
    aura_dynlist_push(l, (const char *)value);
    return (const void *)l;
}

/** list.pop()：弹出并返回末尾元素（AOT 下列表元素为 i8* 句柄）。
 *  空列表返回 NULL；返回值经 getAt 的逆转换还原为原类型。 */
const void *aura_lang_std_Collections_listPop(const void *list) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l || l->len <= 0) return (const void *)0;
    l->len--;
    return (const void *)l->items[l->len];
}

/** 构造整数区间列表（对应 Aura `start..end` / `start..<end` / `start..=end`）。
 *  start/end/inclusive 均为 i32；元素以 i64 句柄（值本身）存入 AuraDynList，
 *  供 AOT 的 `for i in 1..10` 等循环消费。 */
const void *aura_lang_std_Collections_range(int32_t start, int32_t end, int32_t inclusive) {
    AuraDynList *l = aura_dynlist_new(16);
    if (!l) return NULL;
    if (start <= end) {
        int64_t i = start;
        for (;;) {
            if (i > (int64_t)end) break;
            if (i == (int64_t)end && !inclusive) break;
            aura_dynlist_push(l, (const char *)(intptr_t)i);
            if (i == (int64_t)end) break;
            i++;
        }
    } else {
        int64_t i = start;
        for (;;) {
            if (i < (int64_t)end) break;
            if (i == (int64_t)end && !inclusive) break;
            aura_dynlist_push(l, (const char *)(intptr_t)i);
            if (i == (int64_t)end) break;
            i--;
        }
    }
    return (const void *)l;
}

int64_t aura_lang_std_Collections_indexOf(const void *list, const void *value) {
    const AuraDynList *l = (const AuraDynList *)list;
    const char *v = (const char *)value;
    if (!l || !v) return -1;
    for (int64_t i = 0; i < l->len; i++) {
        const char *it = l->items[i];
        if (it && strcmp(it, v) == 0) return i;
    }
    return -1;
}

int aura_lang_std_Collections_contains(const void *list, const void *value) {
    return aura_lang_std_Collections_indexOf(list, value) >= 0 ? 1 : 0;
}

/** set(list, idx, value)：简化实现——越界为追加 */
const void *aura_lang_std_Collections_set(const void *list, int64_t idx, const void *value) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l) {
        l = aura_dynlist_new(4);
    }
    const char *v = (const char *)value ? (const char *)value : "";
    if (idx >= 0 && idx < l->len) {
        l->items[idx] = v;
    } else {
        aura_dynlist_push(l, v);
    }
    return (const void *)l;
}

/* ── aura.lang.std.FileSystem.* ── */

int aura_lang_std_FileSystem_exists(const char *path) {
    return aura_fs_exists(path) ? 1 : 0;
}

const char *aura_lang_std_FileSystem_readText(const char *path) {
    return aura_fs_readText(path);
}

void aura_lang_std_FileSystem_writeText(const char *path, const char *content) {
    (void)aura_fs_writeText(path, content);
}

#if defined(_WIN32)
#include <direct.h>
#define AURA_MKDIR_ONE(p) _mkdir(p)
#else
#define AURA_MKDIR_ONE(p) mkdir((p), 0755)
#endif

/** 递归创建目录（等价 FileSystem.mkdirP） */
int aura_lang_std_FileSystem_mkdirP(const char *path) {
    if (!path || !path[0]) return -1;
    char buf[1024];
    size_t n = strlen(path);
    if (n >= sizeof(buf)) return -1;
    memcpy(buf, path, n + 1);
    for (size_t i = 1; i <= n; i++) {
        if (buf[i] == '/' || buf[i] == '\\' || buf[i] == '\0') {
            char saved = buf[i];
            buf[i] = '\0';
            if (buf[0]) (void)AURA_MKDIR_ONE(buf);
            buf[i] = saved;
        }
    }
    return 0;
}

/* ── aura.lang.std.Process.* ── */

/** 同步执行命令，返回退出码（AOT 下用系统 shell） */
int64_t aura_lang_std_Process_run(const char *cmd) {
    if (!cmd) return -1;
    int rc = system(cmd);
    return (int64_t)rc;
}

/* ── 命令行参数（AOT） ──
 * AOT 发射的 C 入口 `main` 在函数体最开始调用 `aura_args_set(argc, argv)`，
 * 把宿主进程的 argv 存入本模块；`Process.arg(i)` / `Process.argCount()` 据此读取。
 * （VM 侧对应 std_process.rs 的 `std::env::args()`。） */
static int aura_saved_argc = 0;
static char **aura_saved_argv = NULL;

void aura_args_set(int argc, char **argv) {
    aura_saved_argc = argc;
    aura_saved_argv = argv;
}

int64_t aura_lang_std_Process_argCount(void) {
    return (int64_t)aura_saved_argc;
}

const char *aura_lang_std_Process_arg(int64_t index) {
    if (index < 0 || index >= (int64_t)aura_saved_argc) return "";
    if (!aura_saved_argv || !aura_saved_argv[index]) return "";
    /* 必须返回 malloc 副本：argv[i] 是 OS 命令行缓冲的内部指针，低 bit 奇偶不可控，
     * 而 Plan A 依赖「真实字符串指针恒为偶数」来区分装箱整数 ((v<<1)|1)。
     * 返回内部指针会被 aura_to_str_any 误判成装箱整数，打印出地址数字。 */
    return aura_strdup(aura_saved_argv[index]);
}

/** 所有 argv 以 '\n' 连接（AOT 下的简化表示；VM 侧返回 List）。 */
const char *aura_lang_std_Process_args(void) {
    static char *joined = NULL;
    size_t total = 1;
    int i;
    if (joined) {
        free(joined);
        joined = NULL;
    }
    if (!aura_saved_argv) return "";
    for (i = 0; i < aura_saved_argc; i++) {
        if (aura_saved_argv[i]) total += strlen(aura_saved_argv[i]) + 1;
    }
    joined = (char *)malloc(total);
    if (!joined) return "";
    joined[0] = '\0';
    for (i = 0; i < aura_saved_argc; i++) {
        if (i > 0) strcat(joined, "\n");
        if (aura_saved_argv[i]) strcat(joined, aura_saved_argv[i]);
    }
    return joined;
}

/* ── aura.lang.std.Math.*（转发到 aura_math_* 实现） ── */

double aura_lang_std_Math_sin(double x) { return aura_math_sin(x); }
double aura_lang_std_Math_cos(double x) { return aura_math_cos(x); }
double aura_lang_std_Math_tan(double x) { return aura_math_tan(x); }
double aura_lang_std_Math_asin(double x) { return aura_math_asin(x); }
double aura_lang_std_Math_acos(double x) { return aura_math_acos(x); }
double aura_lang_std_Math_atan(double x) { return aura_math_atan(x); }
double aura_lang_std_Math_log(double x) { return aura_math_log(x); }
double aura_lang_std_Math_exp(double x) { return aura_math_exp(x); }
double aura_lang_std_Math_pow(double a, double b) { return aura_math_pow(a, b); }
double aura_lang_std_Math_min(double a, double b) { return aura_math_min(a, b); }
double aura_lang_std_Math_max(double a, double b) { return aura_math_max(a, b); }
int64_t aura_lang_std_Math_ceil(double x) { return aura_math_ceil(x); }
int64_t aura_lang_std_Math_floor(double x) { return aura_math_floor(x); }

_Bool aura_env_has(const char *name) {
    return getenv(name ? name : "") != NULL;
}

// aura.fs
_Bool aura_fs_exists(const char *path) {
    struct _stat st;
    return _stat(path ? path : "", &st) == 0;
}

_Bool aura_fs_isFile(const char *path) {
    struct _stat st;
    return _stat(path ? path : "", &st) == 0 && (st.st_mode & _S_IFREG);
}

_Bool aura_fs_isDirectory(const char *path) {
    struct _stat st;
    return _stat(path ? path : "", &st) == 0 && (st.st_mode & _S_IFDIR);
}

const char *aura_fs_readText(const char *path) {
    /* 动态扩容读取（旧实现用 static 4KB 缓冲，>4KB 的源码会被静默截断，
     * 导致自举编译器中大文件解析出错）。返回堆分配副本，避免多次读取互相覆盖。 */
    FILE *f = fopen(path ? path : "", "rb");
    size_t cap = 4096, len = 0;
    char *buf;
    if (!f) return "";
    buf = (char *)malloc(cap);
    if (!buf) {
        fclose(f);
        return "";
    }
    for (;;) {
        size_t n;
        if (len + 1 >= cap) {
            char *nb = (char *)realloc(buf, cap * 2);
            if (!nb) break;
            buf = nb;
            cap *= 2;
        }
        n = fread(buf + len, 1, cap - len - 1, f);
        len += n;
        if (n == 0) break;
    }
    fclose(f);
    buf[len] = '\0';
    return buf;
}

const void *aura_fs_writeText(const char *path, const char *content) {
    FILE *f = fopen(path ? path : "", "wb");
    if (!f) return 0;
    if (content) fputs(content, f);
    fclose(f);
    return content;
}

