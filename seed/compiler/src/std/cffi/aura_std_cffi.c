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
/* 区域区块的页级分配(避免 CRT 堆空闲链滞留;驱动链接 kernel32 ✓) */
extern void *VirtualAlloc(void *, unsigned long long, unsigned long, unsigned long);
extern int VirtualFree(void *, unsigned long long, unsigned long);

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

/* ──────────────────────────────────────────────────────────────
 * Memory limit guard (backstop for runaway compile/run-time growth)
 *
 * 说明（中文）：AOT 运行时的字符串 / 列表 / 映射 / 对象目前**只分配不释放**，
 * 一旦某处出现「循环里不断分配」，进程就会持续吃内存直至拖垮整个系统。
 * 这里给所有分配加一个总量闸门：默认上限 8 GiB；外部可用环境变量
 * `AURA_MEM_LIMIT_MB` 覆盖（单位 MiB；`0` / `off` 表示不限制）。
 * 超限时打印清晰诊断并 exit(70)，把「系统级假死」降级为「一次可读的失败」。
 *
 * AOT 运行时的字符串 / 列表 / 映射 / 对象目前**只分配不释放**，
 * 一旦某处出现「循环里不断分配」，进程就会持续吃内存直至拖垮整个
 * 系统（物理内存被占满 → 系统无响应）。这里给所有分配加一个总量闸门：
 *
 *   默认上限 8 GiB；外部可用环境变量 `AURA_MEM_LIMIT_MB` 覆盖
 *   （单位 MiB；`0` / `off` 表示不限制）。
 *
 * 超限时打印清晰诊断并 `exit(70)`，把“系统级假死”降级为“一次可读的
 * 编译失败”。所有 Aura 分配都必须走下面的 `aura_mem_*`（含 `aura_malloc`
 * 这一公开入口），否则统计会漏。
 * ────────────────────────────────────────────────────────────── */

/// 默认内存上限（MiB）。
#define AURA_MEM_DEFAULT_LIMIT_MB 8192

/// 每个分配块的前置头：记录负载大小（16 字节，保证负载 16 字节对齐，
/// `Plan A` 依赖「真实指针低位为 0」来区分装箱整数）。
///
/// [P0/D2] `pad` 升级为 ARC 字段（位布局不变，仍 16B ⇒ 负载对齐 / Plan A
/// 不受影响，零额外空间）：
///   · `rc`     [0..31]  引用计数（分配即持有 → 初始 1）
///   · `magic`  [32..47] 0x4155（"AU"）头校验魔数
///   · `flags`  [48..63] 保留（多线程原子模式等预留）
/// magic 参与 `hdr_sane` 强校验：double-free / use-after-free / 外来指针
/// （static 串缓存 / 字符串字面量 / 已释放块）从此可检测。
typedef union AuraMemHdr {
    struct {
        int64_t size;      /* 负载字节数（现状保留） */
        int32_t rc;        /* [0..31]  引用计数 */
        uint16_t magic;    /* [32..47] 头校验魔数 */
        uint16_t flags;    /* [48..63] 保留；[P2] 低 2 位 = 块类型标签 */
    } h;
    long double align_ld;
    void *align_p;
} AuraMemHdr;

/* [P2] 容器/闭包结构前向声明（定义在下方「调用点符号层」节）——
 * ARC 析构级联（rc 归零时释放元素/捕获项）需要提前见到类型。 */
typedef struct AuraDynList AuraDynList;
typedef struct AuraDynMap AuraDynMap;
static void aura_arc_destroy_tagged(void *p, uint16_t tag);

/* [P0/D2] 头魔数与访问助手（单线程非原子 rc，D6；原子版见
 * AURA_ARC_ATOMIC 分支）。rc 独立成 32 位字段 ⇒ 原子模式可直接对
 * `&h->h.rc` 做 InterlockedIncrement/Decrement，不碰 magic。
 *
 * [P2] flags 低 2 位复用为**块类型标签**（0=普通 / 1=dynlist / 2=dynmap /
 * 3=closure env），供 rc 归零时的析构级联释放元素/捕获项。标签校验把
 * 头识别的假阳性率从 2^-16（16 位 magic）压到 ~2^-30：发射器插入的
 * retain/release 会对**任意** i8* 值调用（栈地址 / 枚举变体 alloca /
 * 函数指针 / 静态缓存），16 位魔数在千万级调用下会产生真实踩踏
 * （rc 写到 p-8、甚至 free 栈地址）。⚠️ flags 高位（bit2..15）保留：
 * 未来启用时必须保持「magic 匹配 + 高位为 0」的双重断言强度。 */
#define AURA_MEM_MAGIC 0x4155u   /* "AU" */
#define AURA_MEM_TAG_PLAIN  0u   /* 普通块（字符串/数组/类实例） */
#define AURA_MEM_TAG_LIST   1u   /* AuraDynList：析构时释放 items 数组与元素 */
#define AURA_MEM_TAG_MAP    2u   /* AuraDynMap：析构时释放 keys/vals 数组与元素 */
#define AURA_MEM_TAG_ENV    3u   /* 闭包 env：析构时释放捕获项（count 在 +8） */
#define AURA_MEM_TAG_MASK   0x3u /* 标签占 flags 低 2 位；高位须为 0 */

static void aura_hdr_init(AuraMemHdr *h, int64_t size) {
    h->h.size = size;
    h->h.rc = 1;               /* 分配即持有（D2） */
    h->h.magic = AURA_MEM_MAGIC;
    h->h.flags = 0;
}

/* 头识别：magic 匹配且 flags 高位为 0（标签限定在低 2 位）。
 * 双重断言 ⇒ 对栈/静态/字面量地址的假阳性 ~2^-30，可安全承受任意
 * i8* 值上的 retain/release 调用（P2 发射器依赖此性质）。 */
static int aura_hdr_magic_ok(const AuraMemHdr *h) {
    return h->h.magic == AURA_MEM_MAGIC && (h->h.flags & (uint16_t)~AURA_MEM_TAG_MASK) == 0;
}

static uint16_t aura_hdr_tag(const AuraMemHdr *h) {
    return (uint16_t)(h->h.flags & AURA_MEM_TAG_MASK);
}

/* rc++（保留返回值便于诊断；单线程普通加，D6）。 */
static int32_t aura_hdr_rc_inc(AuraMemHdr *h) {
#ifdef AURA_ARC_ATOMIC
    return InterlockedIncrement((volatile LONG *)&h->h.rc);
#else
    h->h.rc += 1;
    return h->h.rc;
#endif
}

/* rc--，返回递减后的值（调用方据其判归零）。 */
static int32_t aura_hdr_rc_dec(AuraMemHdr *h) {
#ifdef AURA_ARC_ATOMIC
    return InterlockedDecrement((volatile LONG *)&h->h.rc);
#else
    h->h.rc -= 1;
    return h->h.rc;
#endif
}

static int64_t g_aura_mem_limit = -1; /* -1 未初始化；0 = 不限制 */
static int64_t g_aura_mem_used = 0;   /* 当前存活负载字节数 */

/* 记账诊断：定位「计数虚增 / 单块异常大」的分配来源（见 aura_mem_oom 的
 * 第二行统计输出）。默认零成本（只加计数与一次比较）。 */
static int64_t g_aura_mem_alloc_calls = 0;
static int64_t g_aura_mem_realloc_calls = 0;
static int64_t g_aura_mem_free_calls = 0;
static int64_t g_aura_mem_alloc_bytes = 0;   /* alloc 累计请求字节 */
static int64_t g_aura_mem_realloc_net = 0;   /* realloc 累计净增字节 */
static int64_t g_aura_mem_max_block = 0;     /* 单次最大请求（含 realloc 的 n） */
static int64_t g_aura_mem_bad_hdr = 0;       /* 非法头的 free/realloc 次数 */
/* 分配尺寸直方图（定位「哪个量级在吃内存」）。
 * 桶：<=64 / <=256 / <=1K / <=8K / <=64K / <=1M / >1M */
static int64_t g_aura_mem_bucket_calls[7];
static int64_t g_aura_mem_bucket_bytes[7];
static int64_t g_aura_mem_big_trace = 0;    /* 大块追踪已打印条数 */

/* ── 分配归因（定位「哪个 C 函数在分配」）────────────────────────
 *
 * 尺寸直方图只能告诉我们「哪个量级」在吃内存，说不出**位置**。这里的桶按
 * `ra0` = 直接调用 `aura_mem_alloc` / `aura_mem_realloc` 的 C 函数的返回地址
 * 归因（字符串拼接 / 子串 / 装箱 / 列表扩容 …）。
 *
 * 只按 ra0 分桶，**不**带上层 ra1：Aura 编译产物的调用帧不是标准
 * Windows x64 帧，`__builtin_return_address(1)` 读到的是脏栈槽（实测会出现
 * 0x181CBC8E80、0x37007FF7202554AD 这类不可能的值），拿它做桶键会让几乎每次
 * 分配都落到一个新桶，24 个桶瞬间填满、99% 的分配漏记。
 *
 * 内联体写在 `aura_mem_alloc` 里而不是抽成 static helper 传参 —— 后者会让
 * `__builtin_return_address` 被常量折叠成 0x0（实测：整张表塌成一个 ra0=NULL
 * 的桶）。默认零 I/O；报告在 `AURA_MEM_STATS=1` 时随 OOM 统计打印。
 * ─────────────────────────────────────────────────────────── */
#define AURA_FN_COUNT 265
typedef struct { void *a; const char *n; } AuraFnEnt;

/* ra0 → 函数名。AOT 产物被 strip（`llvm-nm` 报 no symbols），返回地址事后无从
 * 解析；但归因代码与全部热点 C 函数在**同一翻译单元**，运行时取函数地址、按
 * 地址排序、二分即可定位「ra0 落在哪个函数体内」。表定义在文件末尾（那里所有
 * 函数才都已声明），见 `AURA_FN` 宏。`off` 异常大说明调用者不在表内。 */
const char *aura_attr_who(void *ra);
size_t aura_attr_off(void *ra);

/* [R151] 大块普查前向声明（实现见「退出泄漏报告」之前）。 */
static void aura_big_record(void *p, int64_t n);
static void aura_big_forget(void *p);
static void aura_big_dump(void);

#define AURA_ATTR_MAX 16384
/* [R131] 归因记录开关：仅 AURA_ATTR_DUMP=1 时记录。⚠️ 无开关时每次分配
 * 也要付哈希/扫描代价（后端 ~4 亿次堆分配 × 哪怕 100ns 都是几十秒），
 * 且 ra1 块曾是 O(AURA_ATTR_MAX) 线性扫——16384 桶时后端实测膨胀 6 倍。 */
static int g_attr_enabled = -1;   /* -1 = 未初始化 */
static void *g_attr_ra0[AURA_ATTR_MAX];
static void *g_attr_ra1[AURA_ATTR_MAX];
static void aura_attr_dump_atexit(void);
static int64_t g_attr_calls[AURA_ATTR_MAX];
static int64_t g_attr_bytes[AURA_ATTR_MAX];
static int64_t g_attr_null_ra0 = 0;      /* ra0 取不到（= NULL）的分配次数 */
static int64_t g_attr_nonnull_ra0 = 0;   /* ra0 取到的分配次数 */
static int64_t g_attr_overflow = 0;      /* 桶满被丢弃的分配次数 */

/* 归因登记（只按 ra0 分桶，见上方说明）。用宏而不是 static helper，避免
 * `__builtin_return_address` 经传参被常量折叠成 0x0。 */
/* [R131] 哈希版归因：ra 直接散列 + 线性探查（8 步内），满了落 overflow。
 * 旧版线性扫 64 桶：表被早期 C 内部地址占满后，其余全部丢弃 ⇒ 归因失真。 */
#define AURA_ATTR_RECORD(ra, bytes) \
    do { \
        static int _ae = -1; \
        if (_ae < 0) { _ae = getenv("AURA_ATTR_DUMP") != 0; } \
        if (_ae && ra) { \
            unsigned long long _ah = ((unsigned long long)(ra)) >> 4; \
            _ah = _ah * 0x9E3779B97F4A7C15ULL; \
            int _ak, _af = 0; \
            for (_ak = 0; _ak < 8; _ak++) { \
                int _as = (int)((_ah + (unsigned long long)_ak) & (AURA_ATTR_MAX - 1)); \
                if (g_attr_ra0[_as] == (ra)) { \
                    g_attr_calls[_as] += 1; \
                    g_attr_bytes[_as] += (bytes); \
                    _af = 1; \
                    break; \
                } \
                if (g_attr_ra0[_as] == NULL) { \
                    g_attr_ra0[_as] = (ra); \
                    g_attr_calls[_as] += 1; \
                    g_attr_bytes[_as] += (bytes); \
                    _af = 1; \
                    break; \
                } \
            } \
            if (!_af) { \
                g_attr_overflow += 1; \
            } \
        } else { \
            g_attr_null_ra0 += 1; \
        } \
    } while (0)

/* 打印分配归因表：先按字节降序、再按次数降序（各 10 行）。 */
static void aura_attr_dump(FILE *f, const char *tag) {
    int i, j, best;
    if (!f) return;
    fprintf(f, "   %s total_sites=%d overflow=%lld (桶满丢弃)\n",
            tag, AURA_ATTR_MAX, (long long)g_attr_overflow);
    {
        /* [R131-DIAG] exe 映像基址：Aura 级 ra0 − base = RVA，可离线对照
         * llvm-nm build/hat-native/hat-native.obj 的符号（RVA−0x1000）。 */
        void *base = GetModuleHandleA((const char *)0);
        fprintf(f, "   %s imagebase=%p\n", tag, base);
    }
    for (i = 0; i < 30 && i < AURA_ATTR_MAX; i++) {
        best = -1;
        for (j = 0; j < AURA_ATTR_MAX; j++) {
            if (g_attr_bytes[j] > 0 &&
                (best < 0 || g_attr_bytes[j] > g_attr_bytes[best])) {
                best = j;
            }
        }
        if (best < 0) break;
        fprintf(f, "   %s bytes ra0=%p who=%s+0x%zx calls=%lld bytes=%lldMB avg=%lldB\n",
                tag, g_attr_ra0[best], aura_attr_who(g_attr_ra0[best]),
                aura_attr_off(g_attr_ra0[best]),
                (long long)g_attr_calls[best],
                (long long)(g_attr_bytes[best] / (1024 * 1024)),
                (long long)(g_attr_calls[best] > 0
                            ? g_attr_bytes[best] / g_attr_calls[best] : 0));
        g_attr_bytes[best] = 0;   /* 打印一次即清，避免与下一组重复 */
    }
    for (i = 0; i < 10 && i < AURA_ATTR_MAX; i++) {
        best = -1;
        for (j = 0; j < AURA_ATTR_MAX; j++) {
            if (g_attr_calls[j] > 0 &&
                (best < 0 || g_attr_calls[j] > g_attr_calls[best])) {
                best = j;
            }
        }
        if (best < 0) break;
        fprintf(f, "   %s calls ra0=%p who=%s+0x%zx calls=%lld bytes=%lldMB avg=%lldB\n",
                tag, g_attr_ra0[best], aura_attr_who(g_attr_ra0[best]),
                aura_attr_off(g_attr_ra0[best]),
                (long long)g_attr_calls[best],
                (long long)(g_attr_bytes[best] / (1024 * 1024)),
                (long long)(g_attr_calls[best] > 0
                            ? g_attr_bytes[best] / g_attr_calls[best] : 0));
        g_attr_calls[best] = 0;
    }
}

/* ──────────────────────────────────────────────────────────────
 * 拼接链探测（定位「s = s + x」自指增长累加器）
 *
 * AOT 运行时**只分配不释放**：`s = s + x` 每次都 `aura_string_concat` 申请新块
 * 并把前缀整体拷一遍，旧前缀永不释放 —— 循环里这样做内存按 O(n²) 增长。
 * 实测 Phase C 所有 >=32KB 的大块分配 ra0 完全相同（= 本函数），尺寸从 32KB
 * 爬到 ~1MB，即某处存在这样的累加器。这里在唯一的分配点上检测
 * 「输入 == 上一次返回值」的自指链并留下可回溯的证据：
 *
 *   · 链长度 / 首个尺寸 / 峰值尺寸；
 *   · 每个「2 的幂」步数处的尺寸（log 采样，48 个点覆盖 2^47 步）；
 *   · 峰值时刻的**内容 head/tail**（可直接看出累加器装的是什么 IR）。
 *
 * 输出（默认零 I/O，只维护计数器）：
 *   · `AURA_MEM_STATS=1`        每跨 32MiB 存活字节向报告文件追加一行快照；
 *   · `AURA_CHAIN_TRIP_KB=<KB>` 链长超过阈值 → 写完整报告 + exit(71)，使
 *                               Aura 侧定位探针正好停在出事的上下文；
 *   · `AURA_CHAIN_REPORT=<path>` 报告文件（默认 ./chain_report.txt）。
 *
 * 报告走普通 fopen（不经过 aura_mem_*），避免递归触发闸门。
 * ────────────────────────────────────────────────────────────── */

#define AURA_CHAIN_LOGPTS 48      /* log2 采样点数 */
#define AURA_CHAIN_SNAP_MB 8      /* 存活字节快照步长（MiB） */

static const char *g_chain_last_out = NULL;
static int64_t g_chain_calls = 0;
static int64_t g_chain_first = -1;
static int64_t g_chain_max = 0;
static int64_t g_chain_max_calls = 0;
static int64_t g_chain_pts_calls[AURA_CHAIN_LOGPTS];
static int64_t g_chain_pts_size[AURA_CHAIN_LOGPTS];
static char g_chain_head[256];
static char g_chain_tail[256];
static int64_t g_chain_head_calls = -1;
static int g_chain_diag_on = -1;          /* -1 未初始化；1 = 开启文件输出 */
static int64_t g_chain_trip_kb = 0;
static int64_t g_chain_snap_mb = 0;
static FILE *g_chain_report_f = NULL;
static int g_chain_report_tried = 0;

/* ── 按「调用者返回地址」归因 ──
 *
 * 上面的链探测要求「输入 == 上一次的输出」——只要两次拼接之间夹了**任何**其它
 * 拼接调用，链就断掉并归零（实测：`s = s + a + b` 这种写法里，第二次拼接的
 * `a` 确实是上一次的输出，但同一基本块里若还有别处的拼接，链照样被冲掉）。
 * 于是链永远为 0，却仍能观察到尺寸单调爬升。这里改成按**调用点地址**分桶：
 * 每个不同的 `aura_string_concat` 调用点各自累计调用次数与最大输出长度，
 * 谁在吃内存一眼可辨。 */
#define AURA_CSITES 24
static void *g_cs_ra[AURA_CSITES];
static int64_t g_cs_calls[AURA_CSITES];
static int64_t g_cs_max[AURA_CSITES];
static int64_t g_cs_bytes[AURA_CSITES];

static int aura_cs_slot(void *ra) {
    int i;
    for (i = 0; i < AURA_CSITES; i++) {
        if (g_cs_ra[i] == ra) return i;
    }
    for (i = 0; i < AURA_CSITES; i++) {
        if (g_cs_ra[i] == NULL) return i;
    }
    return 0;   /* 桶满则复用 0 号 */
}

/* 打印按调用点归因的表（按累计字节降序，最多 12 行）。 */
static void aura_cs_dump(FILE *f, const char *tag) {
    int i, j, best;
    if (!f) return;
    for (i = 0; i < 12 && i < AURA_CSITES; i++) {
        best = -1;
        for (j = 0; j < AURA_CSITES; j++) {
            if (g_cs_ra[j] && g_cs_bytes[j] > 0 &&
                (best < 0 || g_cs_bytes[j] > g_cs_bytes[best])) {
                best = j;
            }
        }
        if (best < 0) break;
        fprintf(f, "   %s site ra=%p calls=%lld bytes=%lldMB max=%lldB\n",
                tag, g_cs_ra[best],
                (long long)g_cs_calls[best],
                (long long)(g_cs_bytes[best] / (1024 * 1024)),
                (long long)g_cs_max[best]);
        /* 打印一次后清掉，避免重复 */
        g_cs_bytes[best] = 0;
    }
}

static void aura_chain_capture_head(const char *s, int64_t n) {
    size_t sn = (size_t)(n > 0 ? n : 0);
    size_t hn = sn > 255 ? 255 : sn;
    if (hn > 0) memcpy(g_chain_head, s, hn);
    g_chain_head[hn] = '\0';
    memcpy(g_chain_tail, sn > 0 ? s + (sn - hn) : (const char *)"", hn);
    g_chain_tail[hn] = '\0';
}

static void aura_chain_dump(FILE *f, const char *tag) {
    int i;
    if (!f) return;
    fprintf(f, "%s: chain=%lld first=%lld max=%lldB@step%d used=%lldMB "
            "alloc_calls=%lld alloc_bytes=%lldMB free_calls=%lld "
            "max_block=%lldB ra0=%p\n",
            tag,
            (long long)g_chain_calls, (long long)g_chain_first,
            (long long)g_chain_max, (int)g_chain_max_calls,
            (long long)(g_aura_mem_used / (1024 * 1024)),
            (long long)g_aura_mem_alloc_calls,
            (long long)(g_aura_mem_alloc_bytes / (1024 * 1024)),
            (long long)g_aura_mem_free_calls,
            (long long)g_aura_mem_max_block,
            __builtin_return_address(0));
    for (i = 0; i < AURA_CHAIN_LOGPTS; i++) {
        if (g_chain_pts_size[i] > 0) {
            fprintf(f, "   %s pt step=%lld size=%lldB\n", tag,
                    (long long)g_chain_pts_calls[i],
                    (long long)g_chain_pts_size[i]);
        }
    }
    if (g_chain_head_calls >= 0) {
        fprintf(f, "   %s head@step%d size=%lldB: [%.255s]\n", tag,
                (int)g_chain_head_calls, (long long)g_chain_max,
                g_chain_head);
        fprintf(f, "   %s tail@step%d: [...%.255s]\n", tag,
                (int)g_chain_head_calls, g_chain_tail);
    }
}

/* 每次拼接后更新链统计（只维护计数器，零 I/O）。 */
static void aura_chain_note(const char *a, int64_t out_len, const char *out) {
    int idx;
    int64_t c;
    if (a == g_chain_last_out) {
        g_chain_calls += 1;
        if (g_chain_first < 0) g_chain_first = out_len;
        if (out_len > g_chain_max) {
            g_chain_max = out_len;
            g_chain_max_calls = g_chain_calls;
            g_chain_head_calls = g_chain_calls;
            if (out && out_len > 0) aura_chain_capture_head(out, out_len);
        }
        c = g_chain_calls;
        idx = 0;
        while (idx < AURA_CHAIN_LOGPTS - 1 && (c >> (idx + 1)) > 0) idx += 1;
        if (c == (int64_t)1 << idx) {
            g_chain_pts_calls[idx] = c;
            g_chain_pts_size[idx] = out_len;
        }
    } else {
        g_chain_calls = 0;
        g_chain_first = -1;
        g_chain_max = 0;
        g_chain_max_calls = 0;
    }
}

/* 诊断开关初始化（惰性，一次）。 */
static void aura_chain_init(void) {
    const char *t;
    if (g_chain_diag_on >= 0) return;
    g_chain_diag_on = (getenv("AURA_MEM_STATS") != NULL) ? 1 : 0;
    t = getenv("AURA_CHAIN_TRIP_KB");
    if (t && t[0]) {
        long long v = atoll(t);
        if (v > 0) {
            g_chain_trip_kb = (int64_t)v;
            g_chain_diag_on = 1;
        }
    }
}

static FILE *aura_chain_report_file(void) {
    const char *p;
    if (g_chain_report_f) return g_chain_report_f;
    if (g_chain_report_tried) return NULL;
    g_chain_report_tried = 1;
    p = getenv("AURA_CHAIN_REPORT");
    if (!p || !p[0]) p = "chain_report.txt";
    /* "w" 而不是 "a"：脚本按 `-TripKB` 反复跑，追加会把上一次运行遗留的
     * 快照混进来（实测两次运行的行数一模一样，分不清哪份是真数据）。 */
    g_chain_report_f = fopen(p, "w");
    return g_chain_report_f;
}

/* 链阈值 / 周期快照。由 aura_string_concat 在返回前调用。 */
static void aura_chain_post_check(int64_t out_len, void *cs_ra) {
    int64_t mb;
    FILE *f;
    int slot, i;
    if (g_chain_diag_on < 0) aura_chain_init();

    /* 按调用点归因（无论诊断开关都统计，成本只是一次比较 + 三个加法）。 */
    slot = aura_cs_slot(cs_ra);
    g_cs_calls[slot] += 1;
    g_cs_bytes[slot] += out_len;
    if (out_len > g_cs_max[slot]) g_cs_max[slot] = out_len;

    if (g_chain_diag_on <= 0) return;

    if (g_chain_trip_kb > 0 && g_chain_calls > 0 &&
        out_len >= g_chain_trip_kb * 1024) {
        f = aura_chain_report_file();
        fprintf(stderr,
            "[aura] concat chain trip: chain=%lld steps, size=%lldB "
            "(trip=%lldKB) -> %s\n",
            (long long)g_chain_calls, (long long)out_len,
            (long long)g_chain_trip_kb,
            getenv("AURA_CHAIN_REPORT") ? getenv("AURA_CHAIN_REPORT")
                                        : "chain_report.txt");
        fflush(stderr);
        aura_chain_dump(f, "TRIP");
        aura_cs_dump(f, "TRIP");
        fflush(f);
        exit(71);
    }

    /* 周期快照：存活字节每跨一个步长。
     * 注意条件是 `mb >= snap + step`（不是 `mb + step > snap`）——后者会在
     * mb 长期不增长时**每次调用都触发**，把报告文件刷成几百 MB 的垃圾。 */
    mb = g_aura_mem_used / (1024 * 1024);
    if (mb >= g_chain_snap_mb + AURA_CHAIN_SNAP_MB) {
        g_chain_snap_mb = mb;
        f = aura_chain_report_file();
        if (f) {
            fprintf(f, "snap: used=%lldMB alloc_calls=%lld alloc_bytes=%lldMB "
                    "free=%lld chain=%lld chain_max=%lldB max_block=%lldB\n",
                    (long long)mb,
                    (long long)g_aura_mem_alloc_calls,
                    (long long)(g_aura_mem_alloc_bytes / (1024 * 1024)),
                    (long long)g_aura_mem_free_calls,
                    (long long)g_chain_calls,
                    (long long)g_chain_max,
                    (long long)g_aura_mem_max_block);
            /* 调用点归因也随快照打印：跨阶段对比谁在增长。 */
            for (i = 0; i < AURA_CSITES; i++) {
                if (g_cs_ra[i]) {
                    fprintf(f, "   site ra=%p calls=%lld bytes=%lldMB "
                            "max=%lldB\n",
                            g_cs_ra[i],
                            (long long)g_cs_calls[i],
                            (long long)(g_cs_bytes[i] / (1024 * 1024)),
                            (long long)g_cs_max[i]);
                }
            }
            fflush(f);
        }
    }
}

/* ──────────────────────────────────────────────────────────────
 * 记账稳健性：单块负载上限
 *
 * `aura_mem_realloc` / `aura_mem_free` 只能处理 `aura_mem_alloc` 产出的负载
 * （其前 16 字节是 `AuraMemHdr`）。若传入**外来指针**（static 串缓存 /
 * 字符串字面量 / 已释放或被越界写入的块），读到的 `h->h.size` 就是任意字节：
 *   * `aura_mem_free` 里 `g_aura_mem_used -= size` 若 size 为负 → 计数虚增；
 *   * `aura_mem_realloc` 里 `g_aura_mem_used += (n - old)` 同样会虚增。
 *
 * 注意：这不是 `Main.aura` 自举 OOM 的原因（实测关掉闸门后真实内存确实涨到
 * 28 GB+，计数是**真实**增长），纯粹是防御性加固。
 *
 * 任何**真实**单块分配都远小于 1 GiB，因此把越界的 size 视为「非法头」：
 * 记账按 0 计（不虚增），并把空闲/重分配请求降级为 no-op / 新分配。
 * ────────────────────────────────────────────────────────────── */
#define AURA_MEM_MAX_BLOCK ((int64_t)1 << 30)

/* [P0/D2] 强校验：size 范围 + magic。magic 使「外来指针」（static 串缓存 /
 * 字符串字面量 / 已释放或被越界写入的块）可检测 —— 两个字段同时误配的
 * 概率约 2^-48，实践上零误报。记账按 0 计（不虚增），并把空闲/重分配
 * 请求降级为 no-op / 新分配（与旧 size-only 校验的行为一致）。 */
static int aura_mem_hdr_sane(const AuraMemHdr *h) {
    if (!h) {
        return 0;
    }
    if (!aura_hdr_magic_ok(h)) {
        return 0;
    }
    return h->h.size >= 0 && h->h.size <= AURA_MEM_MAX_BLOCK;
}

/* 记一次分配尺寸到直方图。 */
static void aura_mem_bucket(int64_t n) {
    int i = 6;
    if (n <= 64) {
        i = 0;
    } else if (n <= 256) {
        i = 1;
    } else if (n <= 1024) {
        i = 2;
    } else if (n <= 8192) {
        i = 3;
    } else if (n <= 65536) {
        i = 4;
    } else if (n <= (1 << 20)) {
        i = 5;
    }
    g_aura_mem_bucket_calls[i] += 1;
    g_aura_mem_bucket_bytes[i] += n;
}

/* ── [P0/D2b] .rdata 字面量永生区间 ──────────────────────────────
 * HIR 字符串常量发射为 `private constant [n x i8] c"..."`（llc → COFF
 * `.rdata`，只读段无法写 rc）⇒ retain/release 必须把落在该区间内的指针
 * 判为「永生」直接 no-op。
 *
 * 注册方式（方案 D2b 候选②）：运行时解析**主模块 PE 头**取 `.rdata`
 * 区段 —— 不依赖发射器符号，seed emit.rs 与自举 aot/Emit.aura 两条发射
 * 链的产物（COFF .rdata 合并）一致适用；也不受符号裁剪影响。
 * 解析失败（非 Windows / 非预期布局）时区间为空 ⇒ 闸门自然关闭，
 * 退回 magic 防御（字面量无头 ⇒ 校验失败 ⇒ 安全 no-op）。
 * ─────────────────────────────────────────────────────────────── */
static char *g_lit_begin = NULL;
static char *g_lit_end = NULL;

/* [R152/D5] .rdata 之外的「永生长生」区间。
 *
 * D5 审计（方案 §四 D5 的遗留项）扫过全部返回 `char*` 的 C 导出，真正的
 * 静态缓冲只剩两处 —— 都是**有意为之的小整数/单字符缓存**（AOT 无 GC，
 * int→String 与 s[i] 是最高频分配，缓存把千万级分配降为一次写入）：
 *   · `aura_to_str`   的 0..1023 表（1024 × 16B）
 *   · `aura_char_cache` 的 256 槽（256 × 16B）
 * 它们落在 **.bss/.data**（不是 .rdata），所以 D2b 的字面量闸门接不住，
 * retain/release 会一路掉到 magic 校验失败、被计入 `bad_magic` —— 功能上
 * 安全（no-op），但会污染 ARC 诊断计数（P3 验收以 `bad_magic==0` 为信号）。
 * 因此显式把这两段注册为长生区间：O(1) 闸门、计数干净。
 *
 * 语义前提：两处槽位都是「写一次、此后只读」，可在调用间安全共享
 * （见各自定义处的注释：按 16 字节对齐以维持 Plan A 的「指针低位为 0」）。 */
#define AURA_LIT_EXTRA_MAX 4
static char *g_lit_x_lo[AURA_LIT_EXTRA_MAX];
static char *g_lit_x_hi[AURA_LIT_EXTRA_MAX];
static int g_lit_x_n = 0;

static void aura_lit_add_range(const void *lo, const void *hi) {
    if (!lo || !hi || hi <= lo) { return; }
    if (g_lit_x_n >= AURA_LIT_EXTRA_MAX) { return; }
    g_lit_x_lo[g_lit_x_n] = (char *)lo;
    g_lit_x_hi[g_lit_x_n] = (char *)hi;
    g_lit_x_n += 1;
}

/* 静态缓存本体（原先定义在各自函数内；为注册区间提到文件作用域）。 */
static char g_int_cache[1024][16];
static unsigned char g_int_cache_ready[1024];
static char g_char_cache_slots[256][16];
static unsigned char g_char_cache_ready[256];

static void aura_lit_register(void) {
    /* 与 PE 布局无关的两段静态缓存：任何平台都注册。 */
    aura_lit_add_range(g_int_cache, (char *)g_int_cache + sizeof(g_int_cache));
    aura_lit_add_range(g_char_cache_slots,
                       (char *)g_char_cache_slots + sizeof(g_char_cache_slots));
#ifdef _WIN32
    HMODULE base = GetModuleHandleA((const char *)0);
    if (!base) {
        return;
    }
    const IMAGE_DOS_HEADER *dos = (const IMAGE_DOS_HEADER *)base;
    if (dos->e_magic != IMAGE_DOS_SIGNATURE) {
        return;
    }
    const IMAGE_NT_HEADERS *nt =
        (const IMAGE_NT_HEADERS *)((const char *)base + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) {
        return;
    }
    const IMAGE_SECTION_HEADER *sec = IMAGE_FIRST_SECTION(nt);
    int i;
    for (i = 0; i < (int)nt->FileHeader.NumberOfSections; i++) {
        /* 段名 8 字节定长、NUL 填充；`.rdata` 恰好 6 字符 */
        if (memcmp(sec[i].Name, ".rdata", 6) == 0 && sec[i].Name[6] == '\0') {
            g_lit_begin = (char *)base + sec[i].VirtualAddress;
            g_lit_end = g_lit_begin + sec[i].Misc.VirtualSize;
            return;
        }
    }
#endif
}

/// p 是否落在 .rdata 字面量区间 / 显式注册的静态缓存区间（永生对象，不参与 ARC）。
static int aura_ptr_is_lit(const void *p) {
    int i;
    if (g_lit_begin && (const char *)p >= g_lit_begin && (const char *)p < g_lit_end) {
        return 1;
    }
    for (i = 0; i < g_lit_x_n; i++) {
        if ((const char *)p >= g_lit_x_lo[i] && (const char *)p < g_lit_x_hi[i]) {
            return 1;
        }
    }
    return 0;
}

/* [P0] 退出泄漏报告（定义在下方 ARC 节；此处前置声明供 atexit 挂钩）。 */
static void aura_leak_report_atexit(void);

#ifdef _WIN32
static LONG CALLBACK aura_arc_veh(EXCEPTION_POINTERS *ep);   /* [P2-DEBUG] 前向声明 */
#endif

static void aura_mem_init(void) {
    /* [T3-ALLOC 2026-10-21] 一次性初始化闸门**必须放在最前面**。
     *
     * 旧实现在这里先 `getenv("AURA_ARC_DEBUG")`（Windows 分支的 VEH 钩子），
     * 之后才判 `g_aura_mem_limit >= 0` 走快返回。UCRT 的 getenv 是线性扫
     * environ（实测 **12.2µs/次**），而 `aura_mem_alloc` 每次分配要调本函数
     * **两次**（见该函数体内重复的 init 调用）⇒ 单次分配固定 24µs。
     *
     * 实测（build/_obw/ObwBench.aura，AOT -O2）：
     *   Memory.alloc(16)            24,492 ns/次   ← 应为 ~50 ns
     *   2 字符字符串拼接             24,376 ns/次
     *   substring(3,9)              24,762 ns/次
     *   arrayListOf + add×3         49,383 ns/次
     * ⇒ 整个后端的「分配风暴」全是这一个 getenv 造成的假象：
     *   writer 段 130 s 里的上百次小分配，实际算力不到 2 s。
     *
     * 修法：闸门前置（语义不变 —— 首次调用仍完成 VEH/atexit/limit 全套初始化，
     * 之后立刻返回），并把分配器里重复的 init 调用删掉。
     */
    if (g_aura_mem_limit >= 0) {
        return;
    }
#ifdef _WIN32
    if (getenv("AURA_ARC_DEBUG")) {
        static int veh_inited = 0;
        if (!veh_inited) {
            veh_inited = 1;
            AddVectoredExceptionHandler(1, aura_arc_veh);
        }
    }
#endif
    if (getenv("AURA_ATTR_DUMP")) {
        atexit(aura_attr_dump_atexit);
    }
    /* [P0] .rdata 字面量区间注册（D2b）+ 泄漏报告挂钩（见 ARC 节）。 */
    aura_lit_register();
    if (getenv("AURA_LEAK_REPORT")) {
        atexit(aura_leak_report_atexit);
    }
    g_aura_mem_limit = (int64_t)AURA_MEM_DEFAULT_LIMIT_MB * 1024 * 1024;
    const char *env = getenv("AURA_MEM_LIMIT_MB");
    if (env && env[0]) {
        if (strcmp(env, "0") == 0 || strcmp(env, "off") == 0 ||
            strcmp(env, "unlimited") == 0) {
            g_aura_mem_limit = 0;
        } else {
            long long mb = atoll(env);
            if (mb > 0) {
                g_aura_mem_limit = (int64_t)mb * 1024 * 1024;
            }
        }
    }
}

/// 设置内存上限（MiB，<=0 表示不限制）。供 CLI / 宿主显式覆盖环境变量。
void aura_mem_set_limit_mb(int64_t mb) {
    g_aura_mem_limit = (mb > 0) ? mb * 1024 * 1024 : 0;
}

/// 当前存活分配字节数（诊断用）。
int64_t aura_mem_used_bytes(void) {
    return g_aura_mem_used;
}

/// 累计分配次数（诊断用；单调递增，从不回退）。
///
/// 给编译器侧做**进度探针**：在长循环里周期性采样，OOM 时最后一行就知道
/// 卡在第几个节点、已经分配了多少次 —— 比猜快得多。
int64_t aura_mem_alloc_calls(void) {
    return g_aura_mem_alloc_calls;
}

/// 当前存活分配 MiB（诊断用；返回 i32 便于 AOT 侧以默认 i32 调用约定直接调用）。
int32_t aura_mem_used_mb(void) {
    return (int32_t)(g_aura_mem_used / (1024 * 1024));
}

/// 当前生效的内存上限字节数（0 = 不限制；诊断用）。
int64_t aura_mem_limit_bytes(void) {
    aura_mem_init();
    return g_aura_mem_limit;
}

/// 超限诊断 + 中止（退出码 70：与普通编译失败区分）。
///
/// 这里刻意用**纯 ASCII**：C 运行时的 stderr 直接写字节，Windows 控制台
/// 默认代码页会把它当本地编码，中文会显示成乱码。
static void aura_mem_dump_stats(void);

/* ── 大拼接调用点归因 ──────────────────────────────────────────────
 * `oom-attr` 只能给出「哪个 C 函数在分配」（`who=aura_string_concat+0x83`
 * 1892 万次 / 7317MB），说不出**哪段 Aura 代码**在拼。运行时的「链探测」
 * 在这里也帮不上忙：它要求「输入 == 上一次输出」，而 Aura 侧形如
 * `s = s + toStr(x) + "|"` 的累加器每次拼接之间都夹了 `toStr()` 调用
 * （自举 Main.aura 实测 `chain=0`），恰好绕过了它。
 *
 * 这里按 `aura_string_concat` 的**返回地址**（= AOT 生成代码里的调用点）
 * 聚合「结果 > 4KB」的拼接，OOM 时按累计字节降序打印。拿到地址后用
 * `llvm-objdump -d <exe>` 反查所属函数名即可精确定位。
 */
#define AURA_LC_SITES 1024
static void *g_lc_ra[AURA_LC_SITES];
static int64_t g_lc_calls[AURA_LC_SITES];
static int64_t g_lc_bytes[AURA_LC_SITES];
static int64_t g_lc_maxes[AURA_LC_SITES];
static int g_lc_n = 0;

static void aura_lc_note(int64_t out_len, void *ra) {
    int i;
    for (i = 0; i < g_lc_n; i++) {
        if (g_lc_ra[i] == ra) {
            g_lc_calls[i] += 1;
            g_lc_bytes[i] += out_len;
            if (out_len > g_lc_maxes[i]) g_lc_maxes[i] = out_len;
            return;
        }
    }
    if (g_lc_n < AURA_LC_SITES) {
        g_lc_ra[g_lc_n] = ra;
        g_lc_calls[g_lc_n] = 1;
        g_lc_bytes[g_lc_n] = out_len;
        g_lc_maxes[g_lc_n] = out_len;
        g_lc_n += 1;
    }
}

static void aura_lc_dump(FILE *f) {
    int i, j, best;
    int order[AURA_LC_SITES];
    if (!f || g_lc_n <= 0) return;
    for (i = 0; i < g_lc_n; i++) order[i] = i;
    /* 选择排序（n <= 24）：按累计字节降序 */
    for (i = 0; i < g_lc_n - 1; i++) {
        best = i;
        for (j = i + 1; j < g_lc_n; j++) {
            if (g_lc_bytes[order[j]] > g_lc_bytes[order[best]]) best = j;
        }
        if (best != i) { int t = order[i]; order[i] = order[best]; order[best] = t; }
    }
    fprintf(f, "[aura] big-concat sites (result > 4KB; >=1MB shown):\n");
    for (i = 0; i < g_lc_n; i++) {
        int k = order[i];
        if (g_lc_bytes[k] < (int64_t)(4 << 20)) continue;
        fprintf(f, "   big-concat ra=%p calls=%lld bytes=%lldMB max=%lldB\n",
                g_lc_ra[k], (long long)g_lc_calls[k],
                (long long)(g_lc_bytes[k] / (1024 * 1024)),
                (long long)g_lc_maxes[k]);
    }
}

/* ── 全量拼接/dup 站点表（方案 3 测量，无尺寸门槛）─────────────────
 * lc 表只收结果 >256B 的拼接，而收口画像（09 报告 A·补充十一）显示剩余
 * 风暴是 3,076 万次 avg-112B concat + 5,106 万次 avg-20B dup 的**分布式
 * 小分配**——恰好低于 lc 门槛，站点级不可见。这张表按调用点 RA 哈希
 * 聚合**全部** aura_string_concat / aura_dup_n 调用（每调用点一次
 * O(1) 探测，AURA_LC_ALL=1 时才启用，默认零开销）：
 *   · RegionReset 时快照打印并清零 → 前端时代表（重置前）与后端
 *     时代表（OOM/exit 打的表）天然分离；
 *   · OOM 统计与 atexit 各打一份（后端时代累计）。
 * RA → Aura 函数：离线 `llvm-nm --numeric-sort <驱动obj>`，
 * rva-0x1000 取最近下方符号（同 oom-attr/exit-ra1 口径）；
 * 「同表达式 N 连 concat」表现为同函数内一串相邻 RA，RA 间距指纹
 * （如 [67,65,71,65,71]）可在 obj 重定位表中唯一命中源构造。 */
#define AURA_SC_SITES 16384              /* 2 的幂：掩码取模 */
#define AURA_SC_MASK   (AURA_SC_SITES - 1)
static void *g_sc_ra[AURA_SC_SITES];
static int64_t g_sc_calls[AURA_SC_SITES];
static int64_t g_sc_bytes[AURA_SC_SITES];
static int64_t g_sc_max[AURA_SC_SITES];
static unsigned char g_sc_kind[AURA_SC_SITES];   /* 0=concat 1=dup_n */
static int64_t g_sc_drop = 0;                    /* 探测超限丢弃次数 */
static int g_sc_on = -1;                         /* -1=未初始化 */

static int aura_sc_enabled(void) {
    if (g_sc_on < 0) {
        g_sc_on = (getenv("AURA_LC_ALL") != NULL);
    }
    return g_sc_on;
}

static void aura_sc_note(unsigned char kind, int64_t out_len, void *ra) {
    uintptr_t h = ((uintptr_t)ra >> 4) * (uintptr_t)2654435761u;
    int i = (int)(h & AURA_SC_MASK);
    int n;
    for (n = 0; n < 128; n++) {
        if (g_sc_ra[i] == ra) {
            g_sc_calls[i] += 1;
            g_sc_bytes[i] += out_len;
            if (out_len > g_sc_max[i]) g_sc_max[i] = out_len;
            return;
        }
        if (g_sc_ra[i] == NULL) {
            g_sc_ra[i] = ra;
            g_sc_calls[i] = 1;
            g_sc_bytes[i] = out_len;
            g_sc_max[i] = out_len;
            g_sc_kind[i] = kind;
            return;
        }
        i = (i + 1) & AURA_SC_MASK;
    }
    g_sc_drop += 1;
}

static unsigned long long aura_sc_rva(void *ra) {
#ifdef _WIN32
    return (unsigned long long)((uintptr_t)ra - (uintptr_t)GetModuleHandleW(NULL));
#else
    (void)ra;
    return 0;
#endif
}

/* top-40 按累计字节降序；打印过即清零，重复调用依次给出次高段。 */
static void aura_sc_dump(FILE *f, const char *tag) {
    int i, j, best;
    if (!f) return;
    fprintf(f, "[aura] %s all-sites top40 (concat/dup per RA, bytes desc):\n", tag);
    for (j = 0; j < 40; j++) {
        best = -1;
        for (i = 0; i < AURA_SC_SITES; i++) {
            if (g_sc_bytes[i] > 0 && (best < 0 || g_sc_bytes[i] > g_sc_bytes[best])) {
                best = i;
            }
        }
        if (best < 0) break;
        fprintf(f,
            "   %s kind=%s rva=0x%llx calls=%lld bytes=%lldMB max=%lldB avg=%lldB\n",
            tag, g_sc_kind[best] ? "dup_n" : "concat",
            aura_sc_rva(g_sc_ra[best]),
            (long long)g_sc_calls[best],
            (long long)(g_sc_bytes[best] >> 20),
            (long long)g_sc_max[best],
            (long long)(g_sc_bytes[best] / g_sc_calls[best]));
        g_sc_bytes[best] = 0;
    }
    fprintf(f, "[aura] %s end (distinct_sites_full=%d drop=%lld)\n",
            tag, AURA_SC_SITES, (long long)g_sc_drop);
}

static void aura_sc_clear(void) {
    memset(g_sc_ra, 0, sizeof(g_sc_ra));
    memset(g_sc_calls, 0, sizeof(g_sc_calls));
    memset(g_sc_bytes, 0, sizeof(g_sc_bytes));
    memset(g_sc_max, 0, sizeof(g_sc_max));
    memset(g_sc_kind, 0, sizeof(g_sc_kind));
    g_sc_drop = 0;
}

/* ── 内存区域取证（AURA_ATTR_DUMP=1 时随 OOM 统计打印）──────────────
 * 背景：big-concat 表的 RA 曾落在主镜像 base+1GB 附近——既不在 exe
 * .text 也不在当时已知的模块区间，事后拿 llvm-objdump 反查无门。归因
 * 必须在 OOM 现场做：用 VirtualQuery 把地址空间走一遍，回答三个问题：
 *   1) RA 落在哪种区域里：MEM_IMAGE（某模块内 → 反查模块名 + 模块内
 *      RVA，归属当场闭合）/ MEM_PRIVATE + PAGE_EXECUTE_*（动态生成的
 *      机器码，RWX/JIT 嫌疑）/ MEM_PRIVATE + PAGE_READWRITE（数据地址
 *      被当成 RA——脏栈槽或坏 RA，表本身不可信）；
 *   2) 全进程有哪些私有可执行区域（JIT/RWX 嫌疑人清单，正常应为空）；
 *   3) 提交总量与区域计数，与 mem stats 口径互相印证。
 * ─────────────────────────────────────────────────────────────── */
#ifdef _WIN32

static const char *aura_prot_str(DWORD p) {
    switch (p & 0xFF) {
    case PAGE_NOACCESS:           return "NA";
    case PAGE_READONLY:           return "R";
    case PAGE_READWRITE:          return "RW";
    case PAGE_WRITECOPY:          return "WC";
    case PAGE_EXECUTE:            return "X";
    case PAGE_EXECUTE_READ:       return "XR";
    case PAGE_EXECUTE_READWRITE:  return "XRW";
    case PAGE_EXECUTE_WRITECOPY:  return "XWC";
    default:                      return "?";
    }
}

/// 单个地址的区域归属（一行）。镜像区域用 GetModuleHandleExW 反查模块，
/// 打印模块完整路径 + 该地址的模块内 RVA——归属在现场直接闭合。
static void aura_region_line(FILE *f, void *addr) {
    MEMORY_BASIC_INFORMATION mbi;
    if (!f || !addr) return;
    if (!VirtualQuery(addr, &mbi, sizeof(mbi))) {
        fprintf(f, "   region %p: VirtualQuery failed (gle=%lu)\n",
                addr, (unsigned long)GetLastError());
        return;
    }
    fprintf(f,
        "   region %p -> base=%p span=%lldMB state=%s type=%s prot=%s%s",
        addr, mbi.BaseAddress, (long long)(mbi.RegionSize >> 20),
        (mbi.State == MEM_COMMIT) ? "commit"
            : (mbi.State == MEM_RESERVE) ? "reserve" : "free",
        (mbi.Type == MEM_IMAGE) ? "image"
            : (mbi.Type == MEM_MAPPED) ? "mapped" : "private",
        aura_prot_str(mbi.Protect),
        (mbi.Protect & PAGE_GUARD) ? "+guard" : "");
    if (mbi.Type == MEM_IMAGE) {
        HMODULE hm = NULL;
        char name[MAX_PATH];
        name[0] = '\0';
        if (GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                               GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                               (LPCSTR)addr, &hm) &&
            GetModuleFileNameA(hm, name, MAX_PATH)) {
            fprintf(f, " module=%s rva=0x%llx", name,
                    (unsigned long long)((uintptr_t)addr - (uintptr_t)hm));
        } else {
            fprintf(f, " module=(image-region, lookup failed)");
        }
    }
    fputc('\n', f);
}

/// 全地址空间走查 + big-concat RA 逐个归属。
/// 只展开三类区域：私有可执行（嫌疑人清单，正常为空）、镜像可执行
/// （数量有限，顺带核对「已知模块区间」）、以及汇总计数；数据区域只进
/// 汇总。RA 归属用与 aura_lc_dump 相同的 >=32MB 阈值。
static void aura_memmap_dump(FILE *f) {
    MEMORY_BASIC_INFORMATION mbi;
    uintptr_t cursor = 0x10000;
    int nCommit = 0, nImage = 0, nPrivExec = 0, nImgExec = 0, i;
    int64_t commitB = 0, imageB = 0, privExecB = 0;
    HMODULE exe = GetModuleHandleW(NULL);
    if (!f) return;
    fprintf(f, "[aura] mem map (exe base=%p)\n", (void *)exe);
    while (VirtualQuery((LPCVOID)cursor, &mbi, sizeof(mbi)) == sizeof(mbi)) {
        uintptr_t next = (uintptr_t)mbi.BaseAddress + mbi.RegionSize;
        int exec = (mbi.Protect &
                    (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE |
                     PAGE_EXECUTE_WRITECOPY)) != 0;
        if (mbi.State == MEM_COMMIT) {
            nCommit += 1;
            commitB += (int64_t)mbi.RegionSize;
            if (mbi.Type == MEM_IMAGE) {
                nImage += 1;
                imageB += (int64_t)mbi.RegionSize;
                /* 镜像可执行区域按模块合并展示负担大，只计数量；
                 * RA 归属行（下方）会带模块名，够用。 */
                if (exec) nImgExec += 1;
            } else if (exec) {
                /* 私有可执行 = 不属于任何模块的机器码：JIT/RWX/自改码
                 * 嫌疑人。正常进程应为空，出现即全列。 */
                nPrivExec += 1;
                privExecB += (int64_t)mbi.RegionSize;
                fprintf(f, "   PRIVATE-EXEC base=%p span=%lldMB prot=%s\n",
                        mbi.BaseAddress, (long long)(mbi.RegionSize >> 20),
                        aura_prot_str(mbi.Protect));
            }
        }
        if (next <= cursor) break;   /* 地址空间走完（回绕保护） */
        cursor = next;
    }
    fprintf(f,
        "[aura] mem map summary: regions=%d commitMB=%lld imageRegions=%d "
        "imageExec=%d imageMB=%lld privateExec=%d (%lldMB)\n",
        nCommit, (long long)(commitB >> 20), nImage, nImgExec,
        (long long)(imageB >> 20), nPrivExec, (long long)(privExecB >> 20));
    fprintf(f, "[aura] big-concat RA attribution (sites >=32MB):\n");
    for (i = 0; i < g_lc_n; i++) {
        if (g_lc_bytes[i] < (int64_t)(32 << 20)) continue;
        aura_region_line(f, g_lc_ra[i]);
    }
}

#else  /* 非 Windows：保持可编译，取证只在 Windows 现场有意义 */

static void aura_memmap_dump(FILE *f) { (void)f; }

#endif /* _WIN32 */

static void aura_mem_oom(int64_t need) {
    fflush(stdout);
    fprintf(stderr,
        "[aura] memory limit exceeded: used ~%lld MiB, requesting ~%lld MiB more, "
        "limit %lld MiB.\n"
        "[aura] aborted to avoid exhausting physical memory. "
        "Set AURA_MEM_LIMIT_MB (MiB; 0 or off = unlimited) to change the limit.\n",
        (long long)(g_aura_mem_used / (1024 * 1024)),
        (long long)((need / (1024 * 1024)) + 1),
        (long long)(g_aura_mem_limit / (1024 * 1024)));
    fflush(stderr);
    aura_mem_dump_stats();
    exit(70);
}

/// 记账统计行（仅诊断；当 `AURA_MEM_STATS=1` 时打印在 OOM 消息之后）。
static void aura_mem_dump_stats(void);
static void aura_mem_dump_stats(void) {
    if (!getenv("AURA_MEM_STATS")) {
        return;
    }
    /* 按 **Aura 侧调用点返回地址** 归因：链探测（`aura_chain_note`）只在
     * 「输入 == 上一次输出」这种教科书式 `s = s + x` 上触发；实测自举
     * `Main.aura` 是 `chain=0`（每次拼接之间都夹了别的调用），但 `<=1M` 桶
     * 有 4.3 万次 / 6.1 GB。此时唯一能定位「哪段 Aura 代码在拼」的就是这张
     * 按返回地址聚合的表 —— 原来只在 TRIP（exit 71）时打印，OOM（exit 70）
     * 反而看不到，这里补上。 */
    aura_cs_dump(stderr, "OOM");
    aura_lc_dump(stderr);
    if (aura_sc_enabled()) {
        aura_sc_dump(stderr, "oom-sc");
    }
    fprintf(stderr,
        "[aura] mem stats: alloc_calls=%lld alloc_bytes=%lldMB realloc_calls=%lld "
        "realloc_net=%lldMB free_calls=%lld max_block=%lldMB bad_hdr=%lld\n",
        (long long)g_aura_mem_alloc_calls,
        (long long)(g_aura_mem_alloc_bytes / (1024 * 1024)),
        (long long)g_aura_mem_realloc_calls,
        (long long)(g_aura_mem_realloc_net / (1024 * 1024)),
        (long long)g_aura_mem_free_calls,
        (long long)(g_aura_mem_max_block / (1024 * 1024)),
        (long long)g_aura_mem_bad_hdr);
    fprintf(stderr,
        "[aura] mem buckets (calls/bytes MB): <=64:%lld/%lld <=256:%lld/%lld "
        "<=1K:%lld/%lld <=8K:%lld/%lld <=64K:%lld/%lld <=1M:%lld/%lld >1M:%lld/%lld\n",
        (long long)g_aura_mem_bucket_calls[0], (long long)(g_aura_mem_bucket_bytes[0] / (1024 * 1024)),
        (long long)g_aura_mem_bucket_calls[1], (long long)(g_aura_mem_bucket_bytes[1] / (1024 * 1024)),
        (long long)g_aura_mem_bucket_calls[2], (long long)(g_aura_mem_bucket_bytes[2] / (1024 * 1024)),
        (long long)g_aura_mem_bucket_calls[3], (long long)(g_aura_mem_bucket_bytes[3] / (1024 * 1024)),
        (long long)g_aura_mem_bucket_calls[4], (long long)(g_aura_mem_bucket_bytes[4] / (1024 * 1024)),
        (long long)g_aura_mem_bucket_calls[5], (long long)(g_aura_mem_bucket_bytes[5] / (1024 * 1024)),
        (long long)g_aura_mem_bucket_calls[6], (long long)(g_aura_mem_bucket_bytes[6] / (1024 * 1024)));
    /* 拼接链统计（见 aura_chain_note 的说明）：OOM 时一并给出，便于
     * 判断这次超限是否由 `s = s + x` 累加器造成。 */
    aura_chain_dump(stderr, "oom-chain");
    aura_cs_dump(stderr, "oom-site");
    fprintf(stderr,
        "[aura] attribution: ra0_ok=%lld ra0_null=%lld\n",
        (long long)g_attr_nonnull_ra0, (long long)g_attr_null_ra0);
    aura_attr_dump(stderr, "oom-attr");
    if (getenv("AURA_ATTR_DUMP")) {
        HMODULE exe_base2 = GetModuleHandleW(NULL);
        uintptr_t base2 = (uintptr_t)exe_base2;
        int k2;
        fprintf(stderr, "oom-ra1 (AOT callsites, rva relative to image base)\n");
        for (k2 = 0; k2 < AURA_ATTR_MAX; k2++) {
            if (g_attr_ra1[k2] != NULL) {
                fprintf(stderr, "  ra1[%d] %p rva=0x%llx\n", k2, g_attr_ra1[k2],
                        (unsigned long long)((uintptr_t)g_attr_ra1[k2] - base2));
            }
        }
        /* 区域取证：big-concat RA 落在哪类区域/哪个模块，OOM 现场闭合
         * （事后反查对私有可执行区域和脏 RA 无能为力，见函数头说明）。 */
        aura_memmap_dump(stderr);
    }
    fflush(stderr);
}

// ── 区域分配(阶段回收)[MEM-FIX] 2026-10-10 ──────────────────────────
// 编译器各阶段(前端 link/parse/SSA1)的碎片分配进 bump 区域;阶段边界
// RegionReset 整体释放。无 GC 运行时的逐块 free 无法追回这些中间串
// (57M alloc vs ~960 free),区域整块重置是唯一的批量回收手段。
// 幸存者纪律:阶段产物(如 .hat 文本)必须在 RegionReset 前落盘/复制
// 到区域外(驱动已按此接线:writeText 先于 RegionReset)。
// 安全:区域块无 AuraMemHdr——aura_mem_free/aura_mem_realloc 对区域
// 指针做**地址范围守卫**,直接走跳过/新分配路径,绝不读头、绝不 free
// 区域内部指针(防 free 中间指针破坏堆)。
static int64_t g_rgn_active = 0;
/* [R87] 旧区域全局已由 g_rgn_ret(AuraRgn) 取代;g_rgn_active 仍为区域模式
 * 开关。scratch 并行 arena 见 g_scr(分配目标 RegionScratchTarget 切换)。 */

/* [R87] scratch 并行 arena:分配目标开关(RegionScratchTarget/
 * RegionRetainTarget/ScratchReset)。用途:前端逐模块把 parse 中间物
 * (token/AST/源串)打进 scratch、HIR 产物切回 retain,模块边界
 * ScratchReset 批量回收——region 峰值从「全模块垃圾累加」降到
 * 「retained 总量 + 单模块垃圾」。自举 Main.aura 实测前端 >21.5GB
 * (287 模块全在单一 retain era 里堆到进程死),本机制为正解。 */
typedef struct {
    void **chunks;
    int64_t *sizes;
    int64_t n, cap;
    char *cur;
    int64_t off, cur_cap, used;
    /* [R93] 当前 bump 所在 chunk 下标。rewind 只回拨 (cur_idx, off)，chunk
     * 列表(n)保持不变 —— 列表本身就是复用池；bump 越过当前块时优先复用
     * 下一个已提交块，而不是另 VirtualAlloc（R92 的泄漏根因）。 */
    int64_t cur_idx;
} AuraRgn;
AuraRgn g_rgn_ret = {0};
AuraRgn g_scr = {0};
int64_t g_rgn_target = 0; /* 0=retain(默认), 1=scratch */

/* ── [P0/D2c] 已释放区域区间防线 ──────────────────────────────────
 * 理论上后端 ARC 永远不接触前端区域对象（D1：.hat 序列化即时代边界，
 * 前后端对象不共享），但若该不变量被违反（悬空区域指针泄漏到后端），
 * RegionReset 后页已 unmap ⇒ 直接读头即 AV。这里把 Reset 时归还 OS 的
 * 区块区间登记下来（有界表），ARC 路径读头前先查表：命中即按「非法头」
 * 计数并 no-op，把潜在 AV 降级为可诊断事件。后端时代表不再增长，
 * `g_freed_rgn_n == 0` ⇒ 查表 O(1)。 */
#define AURA_FREED_RGN_MAX 1024
static struct { char *lo; char *hi; } g_freed_rgn[AURA_FREED_RGN_MAX];
static int g_freed_rgn_n = 0;
static int g_freed_rgn_ovf = 0;   /* 区块数超表容量 ⇒ 防线不完整（计数告警） */
/* 全区间的外包络（[min_lo, max_hi)）：窗外指针一次比较即排除，
 * 把后端每次 retain/release 的区间扫描从 O(表长) 降为常态 O(1)。 */
static char *g_freed_rgn_min_lo = NULL;
static char *g_freed_rgn_max_hi = NULL;

/* [P3] 区间命中判定的**页级带标签直映缓存**。
 *
 * 开启释放后实测：自举里 retain/release 进入 C 侧的量级达 4 亿次，而每次
 * 落在包络窗口内都要把 1024 项区间表线性扫一遍（不释放时几乎没有区间登记，
 * 这项成本趋于零，因此 P0/P1 的 A/B 完全看不出来）。同一页会被反复查询，
 * 所以按 4KB 页做带标签缓存即可把摊还代价打成 O(1)：
 *   · 标签存 page+1，0 表示空槽（避免 `page==0` 与空槽混淆）；
 *   · 区间表只在 RegionReset 时新增（≤1024 次），故每次登记整体失效即可，
 *     无需复杂的失效策略 —— 只在原判定上做「保守化」（页相交即命中，
 *     见下面函数），不会少判，只会多走一次页探活。 */
#define AURA_FR_CACHE_BITS 16
#define AURA_FR_CACHE_N (1u << AURA_FR_CACHE_BITS)
static uintptr_t g_fr_cache_tag[AURA_FR_CACHE_N];
static int16_t g_fr_cache_val[AURA_FR_CACHE_N];

static void aura_fr_cache_invalidate(void) {
    memset(g_fr_cache_tag, 0, sizeof(g_fr_cache_tag));
    memset(g_fr_cache_val, 0, sizeof(g_fr_cache_val));
}

static void aura_freed_rgn_record(char *lo, char *hi) {
    if (g_freed_rgn_n < AURA_FREED_RGN_MAX) {
        g_freed_rgn[g_freed_rgn_n].lo = lo;
        g_freed_rgn[g_freed_rgn_n].hi = hi;
        g_freed_rgn_n++;
        if (!g_freed_rgn_min_lo || lo < g_freed_rgn_min_lo) g_freed_rgn_min_lo = lo;
        if (!g_freed_rgn_max_hi || hi > g_freed_rgn_max_hi) g_freed_rgn_max_hi = hi;
        aura_fr_cache_invalidate();
    } else {
        g_freed_rgn_ovf = 1;
    }
}

/// p 是否落在已归还 OS 的区域区块内（RegionReset 后悬空的区域指针）。
static int aura_ptr_in_freed_rgn(const void *p) {
    int i;
    uintptr_t page;
    size_t slot;
    if (g_freed_rgn_n == 0 || (const char *)p < g_freed_rgn_min_lo ||
        (const char *)p >= g_freed_rgn_max_hi) {
        return 0;   /* 后端时代常态：窗外 O(1) */
    }
    page = ((uintptr_t)p) >> 12;              /* 4KB 页粒度 */
    slot = (size_t)(page & (AURA_FR_CACHE_N - 1));
    if (g_fr_cache_tag[slot] == page + 1) {
        return g_fr_cache_val[slot];
    }
    g_fr_cache_tag[slot] = page + 1;
    g_fr_cache_val[slot] = 0;
    /* [P0/D2c] 保守方向：页与任一区间**有交集**即记命中。
     * 缓存是按页粒度的，若只按查询指针 p 的包含判定记账，同页内其它指针
     * 可能被误判为「不在已归还区间」（假阴性）——那正是本防线要拦的情形
     * （跳过 ⇒ 直接读头 ⇒ RegionReset 后页已 unmap ⇒ AV）。
     * 宁可多一次后续页探活，也绝不漏判。 */
    {
        char *ps = (char *)(page << 12);
        char *pe = ps + 4096;
        for (i = 0; i < g_freed_rgn_n; i++) {
            if (g_freed_rgn[i].lo < pe && ps < g_freed_rgn[i].hi) {
                g_fr_cache_val[slot] = 1;
                return 1;
            }
        }
    }
    return 0;
}

/// [P0/D2c] 真悬空判定：p 落在已释放区间内**且该页至今未重新映射**。
///
/// ⚠️ 仅凭区间表判定会**假阳性**：RegionReset 归还的地址空间会被后端
/// 堆段增长（CRT 堆 VirtualAlloc / 大块 malloc）复用——复用处页已提交、
/// 装的是合法堆块，误判为悬空会跳过合法 retain/release（P2 起 rc 少计
/// ⇒ 提前释放 ⇒ UAF）。这里对区间命中者做一次 VirtualQuery 探活：
///   · 页未提交（MEM_RESERVE/free）⇒ 真悬空（RegionReset 后未复用）⇒ no-op；
///   · 页已提交且可读 ⇒ 地址空间已复用 ⇒ 放行到 magic 校验（合法块通
///     过、区间内部指针被 magic 挡下），把潜在 AV 与假阳性同时消除。
/// 只在区间命中时调用（后端 retain/release 的极少数），成本可忽略。
static int aura_freed_rgn_page_gone(const void *p) {
#ifdef _WIN32
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery((LPCVOID)p, &mbi, sizeof(mbi)) != sizeof(mbi)) {
        return 1;   /* 查询失败：保守按悬空处理 */
    }
    if (mbi.State == MEM_COMMIT &&
        (mbi.Protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ |
                        PAGE_EXECUTE_READWRITE | PAGE_WRITECOPY |
                        PAGE_EXECUTE_WRITECOPY))) {
        return 0;   /* 已提交可读 ⇒ 已复用，非悬空 */
    }
    return 1;
#else
    (void)p;
    return 1;   /* 非 Windows 无探活手段：保守按悬空 */
#endif
}

static int aura_rgn_has(AuraRgn *r, int64_t need) {
    (void)need;
    if (r->cap >= r->n + 1) return 1;
    int64_t nc = r->cap ? r->cap * 2 : 64;
    void **nc1 = (void **)realloc(r->chunks, (size_t)(nc * (int64_t)sizeof(void *)));
    if (!nc1) return 0;
    int64_t *nc2 = (int64_t *)realloc(r->sizes, (size_t)(nc * (int64_t)sizeof(int64_t)));
    if (!nc2) { r->chunks = nc1; return 0; }
    r->chunks = (void **)nc1;
    r->sizes = nc2;
    r->cap = nc;
    return 1;
}

static void *aura_rgn_chunk(AuraRgn *r, int64_t need) {
    /* 区块走 VirtualAlloc:Reset 时 VirtualFree(MEM_RELEASE) 把整个区块
     * 直接归还 OS(避免 CRT 堆空闲链长期占用物理页——本轮实测 WS 峰值
     * 比 used 峰值高 ~5GB 的主因);64KB 对齐满足 Plan A。 */
    int64_t cap = (int64_t)64 << 20; /* 64MB 区块 */
    if (cap < need) cap = ((need + 0xFFFF) / 0x10000) * 0x10000;
    void *c = VirtualAlloc(NULL, (unsigned long long)cap, 0x3000 /*RESERVE|COMMIT*/, 4 /*READWRITE*/);
    if (!c) return NULL;
    if (!aura_rgn_has(r, 0)) return NULL;
    r->chunks[r->n] = c;
    r->sizes[r->n] = cap;
    r->n += 1;
    r->cur = (char *)c;
    r->off = 0;
    r->cur_cap = cap;
    r->cur_idx = r->n - 1; /* [R93] */
    return c;
}

static int64_t g_rgn_probe_mark = 0;
static void *aura_rgn_bump(AuraRgn *r, int64_t need) {
    if (r->cur && r->off + need <= r->cur_cap) {
        /* 当前块还有空间：就地 bump */
    } else if (0 && r->cur && r->cur_idx + 1 < r->n && r->sizes[r->cur_idx + 1] >= need) {
        /* [R93] 复用已提交的下一块。rewind 只回拨指针、不释放也不摘链，
         * 高水位以内的块必须复用；否则每轮 rewind 之后再次 bump 都会
         * 另建 64MB 块，旧块既不在链上也无从释放 ⇒ 线性泄漏。 */
        r->cur_idx += 1;
        r->cur = (char *)r->chunks[r->cur_idx];
        r->off = 0;
        r->cur_cap = r->sizes[r->cur_idx];
    } else {
        if (!aura_rgn_chunk(r, need)) return NULL;
    }
    void *p = r->cur + r->off;
    r->off += need;
    r->used += need;
    g_aura_mem_used += need; /* 区域占用计入闸门;Reset 时统一扣回 */
    /* [R87] 探针：每 1GB 打一行 retain/scratch 分布 */
    if (g_aura_mem_used - g_rgn_probe_mark > ((int64_t)1 << 20 << 7)) {
        g_rgn_probe_mark = g_aura_mem_used;
        fprintf(stderr, "[rgn] used=%lldMB retain=%lldMB scratch=%lldMB\n",
            (long long)(g_aura_mem_used >> 20), (long long)(g_rgn_ret.used >> 20),
            (long long)(g_scr.used >> 20));
        fflush(stderr);
    }
    return p;
}

static void aura_rgn_free_all(AuraRgn *r) {
    int64_t i;
    for (i = 0; i < r->n; i++) {
        if (r->chunks[i]) {
            /* [P0/D2c] 归还前登记区间（区域指针防线，见 g_freed_rgn） */
            aura_freed_rgn_record((char *)r->chunks[i],
                                  (char *)r->chunks[i] + r->sizes[i]);
            VirtualFree(r->chunks[i], 0, 0x8000 /*MEM_RELEASE*/);
        }
    }
    r->n = 0;
    r->cur = NULL;
    r->off = 0;
    r->cur_cap = 0;
    r->cur_idx = 0; /* [R93] */
    g_aura_mem_used -= r->used;
    r->used = 0;
}

/* [R89] 家族路由：字符串临时物族（concat/substr/dup_n/strdup）按 target 分流——
   scratch 相回收；retain 相存活。列表结构族（dynlist/list 原语/realloc）走原 target 路由。*/
static void *aura_rgn_alloc_retain(int64_t n); /* [R93] 前置声明（定义在本函数之后） */
static void *aura_scr_alloc(int64_t n) {
    if (!g_rgn_active) {
        /* [R90] 区域未进入（RegionEnter 之前）：走堆分配。
         * 否则字符串拼接（aura_dup_n / aura_string_concat）会落入 region
         * arena，而 RegionReset 释放全部 region 区块 ⇒ 主入口早于
         * regionEnter 构建的 cacheMain / cacheRt / cacheManifest 等路径串
         * 在 regionReset 后悬垂，后端 FileUtils.readText 触发
         * File.__ctor1 → stringToBuffer → strlen AV（2026-10-18 定位）。 */
        return aura_mem_alloc(n);
    }
    if (!g_rgn_target) {
        /* [R93] retain 相必须落 retain，不能一律进 scratch。
         * 反例（2026-10-19 实测）：HirLowerer 的跨模块登记表
         * （knownTypes / typeMethods / funcRetTys 等）与 AotModuleLinker
         * 的 cacheBlob / stdMods 都是字符串拼接构建，且**跨模块存活**；
         * 若落 scratch，模块级 ScratchRewind 后即悬垂 ⇒ 后续模块类型/方法
         * 解析全部退化为 Basic（自举 .hat 出现 1690 处 `: Basic`）。
         * 只有 RegionScratchTarget 期间（parse 中间物）才可回收。 */
        return aura_rgn_alloc_retain(n);
    }
    int64_t need = ((n + 15) / 16) * 16;
    if (g_aura_mem_limit > 0 && g_aura_mem_used + need > g_aura_mem_limit) {
        aura_mem_oom(need);
    }
    /* [R90] scratch arena 启用（供 ScratchMark/ScratchRewind 逐模块回收）。 */
    return aura_rgn_bump(&g_scr, need);
}

static void *aura_rgn_alloc_retain(int64_t n) {
    int64_t need = ((n + 15) / 16) * 16;
    if (g_aura_mem_limit > 0 && g_aura_mem_used + need > g_aura_mem_limit) {
        aura_mem_oom(need);
    }
    return aura_rgn_bump(&g_rgn_ret, need);
}

static void *rgn_alloc(int64_t n) {
    int64_t need = ((n + 15) / 16) * 16; /* 16 
    }
对齐(Plan A:指针低位为 0) */
    if (g_aura_mem_limit > 0 && g_aura_mem_used + need > g_aura_mem_limit) {
        aura_mem_oom(need);
    }
    if (g_rgn_target) {
        return aura_rgn_bump(&g_scr, need);
    }
    return aura_rgn_bump(&g_rgn_ret, need);
}

/// 进入区域:此后 aura_mem_alloc 的块走区域 bump 分配。
int64_t RegionEnter(void) {
    g_rgn_active = 1;
    return 0;
}

/// [R87] 分配目标 → scratch arena(配合 RegionRetainTarget/ScratchReset
/// 做逐模块 parse 中间物回收)。
int64_t RegionScratchTarget(void) {
    g_rgn_target = 1;
    return 0;
}

/// [R87] 分配目标切回 retain arena。
int64_t RegionRetainTarget(void) {
    g_rgn_target = 0;
    return 0;
}

/// [R87] 整体释放 scratch arena(记账同步扣回)。
/// 区块**保留复用**不 VirtualFree:悬空指针仍落在已映射页(free/realloc
/// 的区域守卫可识别),也省掉逐模块 VirtualAlloc/VirtualFree 循环;
/// 真正释放跟随 RegionReset。记账按逻辑用量扣回。
int64_t ScratchReset(void) {
    g_aura_mem_used -= g_scr.used;
    g_scr.used = 0;
    if (g_scr.n > 0) {
        g_scr.cur = (char *)g_scr.chunks[0];
        g_scr.off = 0;
        g_scr.cur_cap = g_scr.sizes[0];
        g_scr.cur_idx = 0; /* [R93] */
    }
    return 0;
}
/// [R87] scratch 栈式标记（配合 ScratchRewind 做嵌套模块回收：
/// 内层 rewind 永不越过外层 mark——栈纪律下外层 ast 跨递归存活）。
/// mark 编码 = (chunk 序号+1) << 40 | 块内偏移。0 = 空。
int64_t ScratchMark(void) {
    if (g_scr.n <= 0 || !g_scr.cur) {
        return 0;
    }
    /* [R93] 直接用 cur_idx，不再扫 chunks 找 cur（扫描在 chunk 复用后
     * 可能命中重复指针，且 O(n)）。 */
    return ((g_scr.cur_idx + 1) << 40) | (g_scr.off & ((1LL << 40) - 1));
}

/// [R87/R93] 回退 scratch 到 mark：bump 指针回拨（块保留复用），mark 之后
/// 分配的记账同步扣回。栈纪律：内层先 rewind，外层后 rewind。
///
/// [R93] 关键修正：**任何分支都不再摘链/丢弃 chunk**（旧实现在 mark==0
/// 或标记失效时置 n=0，块指针被丢弃且从不 VirtualFree ⇒ 每个模块泄漏
/// 自己的 scratch 高水位；且模块入口的 arena 必为空，mark 恒为 0，于是
/// 设计中的"块复用"永不触发）。现在 n 保持不变，仅回拨 (cur_idx, off)，
/// 后续 bump 复用链上的块。
int64_t ScratchRewind(int64_t mark) {
    int64_t idx = (int64_t)(mark >> 40);   /* 1-based chunk 序号；0 = mark 时 arena 为空 */
    int64_t off = mark & ((1LL << 40) - 1);
    int64_t new_used = 0;
    int64_t k;

    if (g_scr.n <= 0) { return 0; }        /* 无 chunk 可回拨 */

    if (idx <= 0 || idx > g_scr.n) {
        /* mark 时 arena 为空（或标记失效）：逻辑清零，回到第 0 块起点，
         * **保留全部 chunk**。 */
        idx = 1;
        off = 0;
    } else if (off > g_scr.sizes[idx - 1]) {
        off = g_scr.sizes[idx - 1];        /* 容错：越界偏移钳到块尾 */
    }

    for (k = 0; k < idx - 1; k++) { new_used += g_scr.sizes[k]; }
    new_used += off;
    if (new_used > g_scr.used) { new_used = g_scr.used; } /* 记账只减不增 */

    g_aura_mem_used -= (g_scr.used - new_used);
    g_scr.used = new_used;
    g_scr.cur_idx = idx - 1;
    g_scr.cur = (char *)g_scr.chunks[idx - 1];
    g_scr.off = off;
    g_scr.cur_cap = g_scr.sizes[idx - 1];
    return 0;
}
/// [R89] 字符串复制进 retain era（跨 scratch rewind 存活）。
/// HIR 节点文本/loader 累加字段等幸存者在写入前经此落 retain。
const char *StrdupRetain(const char *s) {
    int64_t n = s ? (int64_t)strlen(s) : 0;
    char *out = (char *)aura_rgn_alloc_retain(n + 1);
    if (!out) { return (const char *)""; }
    if (n > 0) { memcpy(out, s, (size_t)n); }
    out[n] = 0;
    return out;
}

/// 重置区域:释放全部区块(记账同步扣回),退出区域模式。
int64_t RegionReset(void) {
    /* [PLAN3] 前端→后端时代边界:快照打印前端时代表并清零,
     * 此后 OOM/exit 打出的 all-sites 表即纯后端时代累计。 */
    if (aura_sc_enabled()) {
        fprintf(stderr, "[aura] ===== all-sites snapshot at RegionReset (frontend era) =====\n");
        aura_sc_dump(stderr, "reset-sc");
        aura_sc_clear();
        fflush(stderr);
    }
    int64_t i;
    /* [R87] scratch 一并清(防残留记账);目标复位为 retain。 */
    aura_rgn_free_all(&g_scr);
    g_rgn_target = 0;
    for (i = 0; i < g_rgn_ret.n; i++) {
        if (g_rgn_ret.chunks[i]) {
            /* [P0/D2c] 归还前登记区间（区域指针防线，见 g_freed_rgn） */
            aura_freed_rgn_record((char *)g_rgn_ret.chunks[i],
                                  (char *)g_rgn_ret.chunks[i] + g_rgn_ret.sizes[i]);
            VirtualFree(g_rgn_ret.chunks[i], 0, 0x8000 /*MEM_RELEASE*/);
        }
    }
    g_aura_mem_used -= g_rgn_ret.used;
    g_rgn_ret.n = 0;
    g_rgn_ret.cur = NULL;
    g_rgn_ret.off = 0;
    g_rgn_ret.cur_cap = 0;
    g_rgn_ret.used = 0;
    g_rgn_ret.cur_idx = 0; /* [R93] */
    g_rgn_active = 0;
    return 0;
}

/// 统一分配入口（带上限检查）。
void *aura_mem_alloc(int64_t n) {
    aura_mem_init();
    if (n < 0) {
        n = 0;
    }
    if (g_rgn_active) {
        /* [R87] region åéä¹è¿å½å ï¼ra0 = å­ç¬¦ä¸²/åè¡¨è¾å©å½æ°ï¼ä¸ malloc è·¯åè´¨éï¼ */
        AURA_ATTR_RECORD(__builtin_return_address(0), n);
        return rgn_alloc(n);
    }
    /* [T3-ALLOC] 重复的 `aura_mem_init()` 已删（入口处已调一次）；
     * 旧实现在这里再调一次，叠加 `getenv("AURA_ARC_DEBUG")` 的 12µs，
     * 单次分配固定 24µs。 */
    g_aura_mem_alloc_calls += 1;
    g_aura_mem_alloc_bytes += n;
    aura_mem_bucket(n);
    /* 归因：就地取值（原因见 `AURA_ATTR_RECORD` 上方说明）。 */
    {
        void *r0 = __builtin_return_address(0);
        void *r1 = __builtin_return_address(1);
        if (r0 == NULL) {
            g_attr_null_ra0 += 1;
        } else {
            g_attr_nonnull_ra0 += 1;
            AURA_ATTR_RECORD(r0, n);
            /* ra1 = 调用者的调用者（AOT 产物内的调用点），供定位 Aura 级
             * 分配风暴：exe .text 布局 = obj 段布局，rva-0x1000 双分到
             * llvm-nm obj 符号即可落到 Aura 函数。
             * ⚠️ [R131] 本块仅在 AURA_ATTR_DUMP 下启用，且扫描上限 64 ——
             * 曾随 AURA_ATTR_MAX 扩到 16384 而变成 O(16K)/次分配，
             * 后端 ~4 亿次堆分配被拖慢 6 倍（2026-10-20 定位）。 */
            if (g_attr_enabled < 0) {
                g_attr_enabled = getenv("AURA_ATTR_DUMP") != 0;
            }
            if (g_attr_enabled) {
                int _a1, _f1 = 0;
                for (_a1 = 0; _a1 < 64; _a1++) {
                    if (g_attr_ra1[_a1] == (r1)) {
                        _f1 = 1;
                        break;
                    }
                    if (g_attr_ra1[_a1] == NULL) {
                        g_attr_ra1[_a1] = (r1);
                        _f1 = 1;
                        break;
                    }
                }
            }
        }
    }
    /* 大块追踪：AURA_MEM_STATS=1 时把 >=256KB 的分配打到 stderr（前 200 次），
     * 用于定位「谁在吃内存」。 */
    if (n >= 262144 && getenv("AURA_MEM_STATS") && g_aura_mem_big_trace < 40) {
        g_aura_mem_big_trace += 1;
        /* 打印调用者返回地址（用 `llvm-nm --numeric-sort` 可解析到函数），
         * 用于定位「谁在 O(n²) 地分配大块」。 */
        fprintf(stderr, "[aura] big alloc #%lld: %lld bytes who=%s+0x%zx calls=%lld used=%lldMB\n",
                (long long)g_aura_mem_big_trace, (long long)n,
                aura_attr_who(__builtin_return_address(0)),
                aura_attr_off(__builtin_return_address(0)),
                (long long)g_aura_mem_alloc_calls,
                (long long)(g_aura_mem_used / (1024 * 1024)));
        fflush(stderr);
    }
    if (n > g_aura_mem_max_block) {
        g_aura_mem_max_block = n;
    }
    if (g_aura_mem_limit > 0 && g_aura_mem_used + n > g_aura_mem_limit) {
        aura_mem_oom(n);
    }
    AuraMemHdr *h = (AuraMemHdr *)malloc(sizeof(AuraMemHdr) + (size_t)n);
    if (!h) {
        aura_mem_oom(n);
    }
    /* [P0/D2] 分配即持有：rc=1 + magic（原 `h->h.size = n` 只写 size）。 */
    aura_hdr_init(h, n);
    g_aura_mem_used += n;
    /* [R151] 大块普查（默认关闭 ⇒ 热路径零开销）。 */
    aura_big_record((void *)((char *)h + sizeof(AuraMemHdr)), n);
    return (void *)((char *)h + sizeof(AuraMemHdr));
}

/// 统一重分配入口（带上限检查）。
void *aura_mem_realloc(void *p, int64_t n) {
    aura_mem_init();
    if (!p) {
        return aura_mem_alloc(n);
    }
    if (n < 0) {
        n = 0;
    }
    /* [MEM-FIX] 区域指针守卫(同 aura_mem_free):区域块走「新分配+拷贝」
     * 路径(拷贝长度按新尺寸,区域块内越界读是安全的有意行为)。
     * [R87] retain 与 scratch 两个 arena 都要查。 */
    if (g_rgn_active) {
        int64_t rem = -1;
        int64_t k;
        for (k = 0; k < g_rgn_ret.n && rem < 0; k++) {
            char *cs = (char *)g_rgn_ret.chunks[k];
            if ((char *)p >= cs && (char *)p < cs + g_rgn_ret.sizes[k]) {
                rem = (cs + g_rgn_ret.sizes[k]) - (char *)p;
            }
        }
        for (k = 0; k < g_scr.n && rem < 0; k++) {
            char *cs = (char *)g_scr.chunks[k];
            if ((char *)p >= cs && (char *)p < cs + g_scr.sizes[k]) {
                rem = (cs + g_scr.sizes[k]) - (char *)p;
            }
        }
        if (rem >= 0) {
            /* 拷贝长度按区块剩余截断,避免越界读(区块尾部之后未映射) */
            int64_t cp = (n < rem) ? n : rem;
            char *np = (char *)aura_mem_alloc(n);
            if (!np) return NULL;
            if (cp > 0) memcpy(np, p, (size_t)cp);
            return np;
        }
    }
    /* [PLAN3-FIX] 悬空区域指针(Reset 后):页已 unmap,拷贝必 AV。
     * 返回零填充新块顶住调用方,计数入档。 */
    AuraMemHdr *h = (AuraMemHdr *)((char *)p - sizeof(AuraMemHdr));
    int64_t old = h->h.size;
    g_aura_mem_realloc_calls += 1;
    if (n > g_aura_mem_max_block) {
        g_aura_mem_max_block = n;
    }
    /* 非法头（见 AURA_MEM_MAX_BLOCK 说明）：p 不是本运行时的负载。
     * 绝不能拿 `old` 去记账（会虚增），也不能对陌生指针 realloc —— 改为
     * 新分配 + 拷贝（长度以新旧较小值为准，避免越界读）。
     * [P0/D2] magic 参与强校验：静态缓存 / 字面量 / 已释放块一并识别。 */
    if (!aura_mem_hdr_sane(h)) {
        g_aura_mem_bad_hdr += 1;
        char *np = (char *)aura_mem_alloc(n);
        if (!np) {
            return NULL;
        }
        if (p && n > 0) {
            memcpy(np, p, (size_t)n);
        }
        return np;
    }
    g_aura_mem_realloc_net += (n - old);
    /* 归因只记净增（与上面的记账口径一致），避免把「同一块的扩容」重复计成
     * 全新分配 —— 那样列表/字符串缓冲区的增长会虚高一整个旧尺寸。 */
    {
        int64_t dn = (n > old) ? (n - old) : 0;
        void *r0 = __builtin_return_address(0);
        if (r0 != NULL) {
            AURA_ATTR_RECORD(r0, dn);
        }
    }
    if (g_aura_mem_limit > 0 && g_aura_mem_used - old + n > g_aura_mem_limit) {
        aura_mem_oom(n - old);
    }
    /* [R151] 大块普查：realloc 可能整块搬迁，旧指针先摘除、新指针重新登记。 */
    aura_big_forget(p);
    AuraMemHdr *nh = (AuraMemHdr *)realloc((void *)h, sizeof(AuraMemHdr) + (size_t)n);
    if (!nh) {
        aura_mem_oom(n - old);
    }
    /* [P0/D2] realloc 整块搬迁（含 16B 头）⇒ rc/magic 原样保留，
     * 无需重建 —— dynlist 扩容等热路径零额外开销。 */
    nh->h.size = n;
    g_aura_mem_used += (n - old);
    aura_big_record((void *)((char *)nh + sizeof(AuraMemHdr)), n);
    return (void *)((char *)nh + sizeof(AuraMemHdr));
}

/// 统一释放入口。
void aura_mem_free(void *p) {
    if (!p) {
        return;
    }
    /* [MEM-FIX] 区域指针守卫:区域 bump 块无 AuraMemHdr,读头 = 读垃圾
     * (垃圾若通过 hdr_sane 会被 free 到区域块中间 → 堆破坏)。区域内存
     * 由 RegionReset 整体管理,这里直接跳过。
     * [R87] retain 与 scratch 两个 arena 都要查。 */
    if (g_rgn_active) {
        int64_t k;
        for (k = 0; k < g_rgn_ret.n; k++) {
            char *cs = (char *)g_rgn_ret.chunks[k];
            if ((char *)p >= cs && (char *)p < cs + g_rgn_ret.sizes[k]) {
                g_aura_mem_bad_hdr += 1;
                return;
            }
        }
        for (k = 0; k < g_scr.n; k++) {
            char *cs = (char *)g_scr.chunks[k];
            if ((char *)p >= cs && (char *)p < cs + g_scr.sizes[k]) {
                g_aura_mem_bad_hdr += 1;
                return;
            }
        }
    }
    AuraMemHdr *h = (AuraMemHdr *)((char *)p - sizeof(AuraMemHdr));
    int64_t sz = h->h.size;
    g_aura_mem_free_calls += 1;
    /* 非法头：不是本运行时的负载 —— 既不记账（避免 `used -= 负数` 虚增），
     * 也**不** free（对陌生指针 free 会污染堆）。
     * [P0/D2] magic 参与强校验：静态缓存 / 字面量 / 已释放块一并识别。 */
    if (!aura_mem_hdr_sane(h)) {
        g_aura_mem_bad_hdr += 1;
        return;
    }
    g_aura_mem_used -= sz;
    if (g_aura_mem_used < 0) {
        g_aura_mem_used = 0;
    }
    aura_big_forget(p);   /* [R151] 大块普查：存活集合摘除 */
    free((void *)h);
}

/* ═══════════════════════════════════════════════════════════════════════
 * [P0/D2] ARC 引用计数（AuraMemHdr.rc）
 *
 * 契约（方案 D3 所有权不变量）：任何在 SSA/槽位中存续的 String/List/
 * Object 值，持有者对其计数 +1；覆盖、离开作用域、从容器移除时持有者
 * 负责 release。整数/浮点/布尔走 Plan A 低位装箱（bit0=1），天然不参与。
 *
 * retain/release 的四道防线（任一不过即安全 no-op，**绝不写不该写的
 * 内存**——这是「外来指针不会被写坏」的根基）：
 *   1. D1  时代闸门：`g_rgn_active`（前端区域时代）→ no-op
 *      （区域对象由 RegionReset 整块回收，逐对象释放既不可能也无必要；
 *        前后端对象经 .hat 序列化不共享 ⇒ 后端 ARC 永不接触区域对象）；
 *   2. Plan A 装箱：指针低位为 1 = 装箱整数 → 跳过；
 *   3. D2b .rdata 闸门：字面量永生区间 → no-op；
 *   4. D2c 区域防线：已归还 OS 的区域区块 → 按非法头计数并 no-op；
 *   5. magic 强校验：头不是本运行时的 → 计数并 no-op（静态串缓存 /
 *      外来 / 已释放块落到这里，把潜在 AV 降级为可诊断事件）。
 *
 * D6：单线程非原子 rc（编译器场景）；`-DAURA_ARC_ATOMIC` 切原子版
 * （Interlocked，供多线程场景）。
 * ═══════════════════════════════════════════════════════════════════════ */

/* ARC 计数（诊断用）：retain/release 次数与各闸门 no-op 次数。
 * P1 阶段（尚无发射器 release）这些数字全部来自 C 侧容器操作。 */
static int64_t g_arc_retain_calls = 0;
static int64_t g_arc_release_calls = 0;
static int64_t g_arc_freed = 0;        /* release 归零并真正释放的次数 */
static int64_t g_arc_noop_era = 0;     /* D1 时代闸门 */
static int64_t g_arc_noop_lit = 0;     /* D2b .rdata 字面量 */
static int64_t g_arc_noop_rgn = 0;     /* D2c 已释放区域 */
static int64_t g_arc_noop_boxed = 0;   /* Plan A 装箱整数 */
static int64_t g_arc_bad_magic = 0;    /* 头校验失败（外来/静态指针） */
static int64_t g_arc_rgn_reused = 0;   /* D2c 区间命中但页已复用（假阳性排除） */

/* ── [P2-DEBUG] 释放日志（AURA_ARC_DEBUG=1）───────────────────────────
 * 记录每个「rc 归零并真正 free」的块（ptr + 释放调用点返回地址 + size），
 * 用于在后续过量释放命中时反查「谁先释放了它」（二次释放定位）。
 * 定长环形，仅供诊断；非 debug 环境零成本（写入点由 env 判空）。 */
#define AURA_FREE_LOG_N 65536
static struct {
    const void *p;
    void *ra;
    int64_t sz;
    int32_t rc_seen;
} g_arc_free_log[AURA_FREE_LOG_N];
static int64_t g_arc_free_log_seq = 0;

/// AURA_ARC_DEBUG 只读一次（热路径上避免反复 getenv）。
static int aura_arc_debug_on(void) {
    static int cached = -1;
    if (cached < 0) {
        cached = getenv("AURA_ARC_DEBUG") ? 1 : 0;
    }
    return cached;
}

static void aura_arc_free_log_record(const void *p, void *ra, int64_t sz, int32_t rc) {
    int64_t i = g_arc_free_log_seq & (AURA_FREE_LOG_N - 1);
    g_arc_free_log[i].p = p;
    g_arc_free_log[i].ra = ra;
    g_arc_free_log[i].sz = sz;
    g_arc_free_log[i].rc_seen = rc;
    g_arc_free_log_seq++;
}

static void aura_arc_free_log_find(const void *p) {
    int64_t i;
    int64_t lo = g_arc_free_log_seq - AURA_FREE_LOG_N;
    if (lo < 0) lo = 0;
    for (i = g_arc_free_log_seq - 1; i >= lo; i--) {
        int64_t k = i & (AURA_FREE_LOG_N - 1);
        if (g_arc_free_log[k].p == p) {
            fprintf(stderr,
                    "[aura] ARC   prior free: seq=%lld size=%lld from ra=%p "
                    "(back = %lld releases ago)\n",
                    (long long)i, (long long)g_arc_free_log[k].sz,
                    g_arc_free_log[k].ra, (long long)(g_arc_free_log_seq - 1 - i));
            return;
        }
    }
    fprintf(stderr, "[aura] ARC   prior free: not found in last %d frees\n",
            AURA_FREE_LOG_N);
}

/* ── [P2-DEBUG] 失衡点调用栈转储（AURA_ARC_DEBUG=1）─────────────────
 * 打印进程基址（ASLR 后帧地址 - base = PE 内 RVA）与裸帧地址；
 * 离线用 `llvm-nm PhotonHatCompile-arc.exe` 按 RVA 反查所在函数
 * （驱动模块的函数名是 aura_user_*，可直接读）。 */
static void aura_arc_debug_frames(const char *why, const void *p, int rc, unsigned tag) {
#ifdef _WIN32
    void *frames[20];
    USHORT n;
    int i;
    static int base_shown = 0;
    if (!base_shown) {
        base_shown = 1;
        fprintf(stderr, "[aura] ARC-DEBUG image_base=%p\n",
                (void *)GetModuleHandleA(NULL));
    }
    n = CaptureStackBackTrace(0, 20, frames, NULL);
    fprintf(stderr, "[aura] ARC %s ptr=%p rc=%d tag=%u frames=%u\n",
            why, p, rc, tag, (unsigned)n);    for (i = 0; i < (int)n; i++) {
        fprintf(stderr, "    #%d %p\n", i, frames[i]);
    }
    fflush(stderr);
#else
    (void)why; (void)p; (void)rc; (void)tag;
#endif
}

/* ── [P2-DEBUG] 访问违例捕获（AURA_ARC_DEBUG=1）─────────────────────
 * 崩溃点常常早于「过量释放」可见点（悬空读不经过 release）。装一个
 * vectored handler，把 AV 的指令地址/访问地址与调用栈打出来，配合
 * AURA_LINK_MAP 生成的 map 直接反查函数名。 */
#ifdef _WIN32
static LONG CALLBACK aura_arc_veh(EXCEPTION_POINTERS *ep) {
    if (ep && ep->ExceptionRecord &&
        ep->ExceptionRecord->ExceptionCode == EXCEPTION_ACCESS_VIOLATION) {
        void *frames[24];
        USHORT n = CaptureStackBackTrace(0, 24, frames, NULL);
        int i;
        fprintf(stderr,
                "[aura] CRASH AV at %p kind=%llu addr=%p\n",
                ep->ExceptionRecord->ExceptionAddress,
                (unsigned long long)ep->ExceptionRecord->ExceptionInformation[0],
                (void *)ep->ExceptionRecord->ExceptionInformation[1]);
        for (i = 0; i < (int)n; i++) {
            fprintf(stderr, "    #%d %p\n", i, frames[i]);
        }
        fflush(stderr);
    }
    return EXCEPTION_CONTINUE_SEARCH;
}
#endif

/// [P0/D2] retain：持有一个堆对象（rc++）。
void aura_arc_retain(const void *p) {
    if (!p) {
        return;
    }
    if ((uintptr_t)p & 1) {                    /* Plan A 装箱整数 */
        g_arc_noop_boxed++;
        return;
    }
    if (g_rgn_active) {                        /* D1 时代闸门 */
        g_arc_noop_era++;
        return;
    }
    if (aura_ptr_is_lit(p)) {                  /* D2b .rdata 字面量永生 */
        g_arc_noop_lit++;
        return;
    }
    if (aura_ptr_in_freed_rgn(p)) {            /* D2c 悬空区域指针 */
        if (aura_freed_rgn_page_gone(p)) {
            g_arc_noop_rgn++;
            g_aura_mem_bad_hdr++;
            return;
        }
        g_arc_rgn_reused++;   /* 页已复用 ⇒ 区间表假阳性，放行到 magic */
    }
    AuraMemHdr *h = (AuraMemHdr *)((char *)p - sizeof(AuraMemHdr));
    if (!aura_hdr_magic_ok(h)) {               /* 外来/静态/已释放指针 */
        g_arc_bad_magic++;
        g_aura_mem_bad_hdr++;
        return;
    }
    aura_hdr_rc_inc(h);
    g_arc_retain_calls++;
}

/// [P0/D2] release：释放一个持有（rc--）；归零 → aura_mem_free。
/// 返回值：1 = 本次归零并释放，0 = 仍被引用 / 安全 no-op。
int aura_arc_release(const void *p) {
    if (!p) {
        return 0;
    }
    if ((uintptr_t)p & 1) {                    /* Plan A 装箱整数 */
        g_arc_noop_boxed++;
        return 0;
    }
    if (g_rgn_active) {                        /* D1 时代闸门 */
        g_arc_noop_era++;
        return 0;
    }
    if (aura_ptr_is_lit(p)) {                  /* D2b .rdata 字面量永生 */
        g_arc_noop_lit++;
        return 0;
    }
    if (aura_ptr_in_freed_rgn(p)) {            /* D2c 悬空区域指针 */
        if (aura_freed_rgn_page_gone(p)) {
            g_arc_noop_rgn++;
            g_aura_mem_bad_hdr++;
            return 0;
        }
        g_arc_rgn_reused++;   /* 页已复用 ⇒ 区间表假阳性，放行到 magic */
    }
    AuraMemHdr *h = (AuraMemHdr *)((char *)p - sizeof(AuraMemHdr));
    if (!aura_hdr_magic_ok(h)) {               /* 外来/静态/已释放指针 */
        g_arc_bad_magic++;
        g_aura_mem_bad_hdr++;
        /* [P3-DEBUG] 坏 magic 意味着 release 落在「不是自己分配的块」上
         * （外来静态区 / 区域无头内存 / 已经 free 过的块）。只打前 5 次，
         * 免得把日志冲爆；配合 AURA_LINK_MAP 的 .map 按 RVA 反查函数。 */
        if (aura_arc_debug_on() && g_arc_bad_magic <= 5) {
            fprintf(stderr,
                    "[aura] ARC BAD-MAGIC ptr=%p size=%lld magic=0x%04x flags=0x%04x\n",
                    p, (long long)h->h.size, (unsigned)h->h.magic,
                    (unsigned)h->h.flags);
            aura_arc_free_log_find(p);
            aura_arc_debug_frames("BAD-MAGIC", p, 0, 0);
        }
        return 0;
    }
    /* [P2-DEBUG] 过量释放检测：rc 已 ≤0 仍来 release ⇒ retain/release
     * 配对失衡（会走到二次 free ⇒ 堆损坏）。命中即打印并**不**释放，
     * 把「0xC0000374 堆损坏」前移到第一处失衡点。AURA_ARC_DEBUG=1 启用；
     * 同时打印调用栈（离线用 llvm-nm 反查函数名）。 */
    if (h->h.rc <= 0 && aura_arc_debug_on()) {
        fprintf(stderr, "[aura] ARC OVER-RELEASE size=%lld\n", (long long)h->h.size);
        aura_arc_free_log_find(p);
        aura_arc_debug_frames("OVER-RELEASE", p, (int)h->h.rc,
                              (unsigned)aura_hdr_tag(h));
        return 0;
    }
    int32_t rc = aura_hdr_rc_dec(h);
    if (rc > 0) {
        g_arc_release_calls++;
        return 0;
    }
    /* 归零：先做标签块析构级联（释放容器元素/闭包捕获项——D3），
     * 再经统一释放入口归还块自身（magic 已验证，必中正常释放路径）。
     * rc 停在 0 无需回写——块即将 free。
     * ⚠️ 区域时代的列表/映射未打标签（无头可打），析构不会误触；
     * 它们由 RegionReset 整块回收。 */
    if (aura_arc_debug_on()) {
        aura_arc_free_log_record(p, __builtin_return_address(0), h->h.size, 0);
    }
    /* [P2] 析构级联默认**关闭**（AURA_ARC_DESTRUCT=1 才启用）：
     * 容器析构会 release 元素，前提是「元素的所有别名都 retain 过」。
     * 当前发射器还做不到（字段写/静态注册表/结构体值副本都会留下
     * 未 retain 的别名），启用会把仍然被引用的元素释放掉
     * （实测字符串被回收后再次使用 ⇒ 符号名损坏 / AV）。
     * 关闭后：容器自身块与 items 数组仍会释放，元素按引用泄漏
     * ——与 P1 之前的基线同量级，不影响正确性。 */
    if (getenv("AURA_ARC_DESTRUCT")) {
        aura_arc_destroy_tagged((void *)p, aura_hdr_tag(h));
    }
    aura_mem_free((void *)p);
    g_arc_freed++;
    return 1;
}

/* [P0/D5] 直堆分配（带 ARC 头），**不受区域路由影响**。
 * 用途：返回值必须跨 RegionReset 存活的字符串入口（env_get /
 * fromCharCode / io_readFile / io_readLine）—— 这些入口现行语义就是
 * 「每次独立 malloc 副本、永生存活」，改为带头分配后行为不变，且纳入
 * ARC 管辖（P2 起可被 release）与内存闸门记账。 */
static void *aura_mem_alloc_heap(int64_t n) {
    aura_mem_init();
    if (n < 0) {
        n = 0;
    }
    g_aura_mem_alloc_calls += 1;
    g_aura_mem_alloc_bytes += n;
    aura_mem_bucket(n);
    {
        void *r0 = __builtin_return_address(0);
        if (r0 != NULL) {
            AURA_ATTR_RECORD(r0, n);
        }
    }
    if (n > g_aura_mem_max_block) {
        g_aura_mem_max_block = n;
    }
    if (g_aura_mem_limit > 0 && g_aura_mem_used + n > g_aura_mem_limit) {
        aura_mem_oom(n);
    }
    AuraMemHdr *h = (AuraMemHdr *)malloc(sizeof(AuraMemHdr) + (size_t)n);
    if (!h) {
        aura_mem_oom(n);
    }
    aura_hdr_init(h, n);
    g_aura_mem_used += n;
    return (void *)((char *)h + sizeof(AuraMemHdr));
}

/* ── [R151] 大块普查（AURA_BIG_CENSUS=1）───────────────────────────────
 * P4 的入口问题：自举退出时 4293MB 存活载荷里 **2840MB 落在 3774 个 ≥64KB
 * 的块上**（≤64B 的 3280 万个小块总共才 504MB），而既有的 big-alloc trace
 * 只打前 40 次（采样窗口太窄，看不全景）。
 *
 * 做法：对**所有** ≥阈值 的分配按 `ra0` 分配点建账，并维护存活集合
 * （free / realloc 时摘除），退出时按「存活字节」降序打印 top-20。
 *
 * 成本：≥64KB 的分配在自举里只有几千次（占 4 亿次堆分配的 ~0.001%）；
 * 默认关闭 ⇒ 热路径零开销。阈值可用 `AURA_BIG_MIN_KB` 调（默认 64）。
 * ─────────────────────────────────────────────────────────────────── */
#define AURA_BIG_SITES 1024
#define AURA_BIG_BUCK  1024
#define AURA_BIG_POOL  (1 << 19)

typedef struct AuraBigNode {
    void *p;
    struct AuraBigNode *next;
    int64_t sz;
    int site;
} AuraBigNode;

#define AURA_BIG_FRAMES 8

static int64_t g_big_min = 0;          /* 0 = 未初始化 */
static int     g_big_on  = -1;         /* -1 = 未初始化 */
static void   *g_big_key[AURA_BIG_SITES][AURA_BIG_FRAMES];
static int64_t g_big_calls[AURA_BIG_SITES];
static int64_t g_big_bytes[AURA_BIG_SITES];
static int64_t g_big_live_n[AURA_BIG_SITES];
static int64_t g_big_live_b[AURA_BIG_SITES];
static int64_t g_big_max[AURA_BIG_SITES];
static AuraBigNode  *g_big_buckets[AURA_BIG_BUCK];
static AuraBigNode   g_big_pool[AURA_BIG_POOL];
static int           g_big_pool_n = 0;
static AuraBigNode  *g_big_freelist = NULL;
static int64_t g_big_overflow = 0;
static int64_t g_big_live_total = 0, g_big_live_bytes = 0;
static int64_t g_big_all_total = 0, g_big_all_bytes = 0;

static void aura_big_init(void) {
    if (g_big_on >= 0) return;
    g_big_on = getenv("AURA_BIG_CENSUS") != 0;
    if (!g_big_on) return;
    {
        const char *e = getenv("AURA_BIG_MIN_KB");
        g_big_min = (int64_t)64 * 1024;
        if (e && e[0]) {
            long v = strtol(e, NULL, 10);
            if (v > 0) g_big_min = (int64_t)v * 1024;
        }
    }
    atexit(aura_big_dump);
}

/* 分配点用**栈回溯**而不是 `__builtin_return_address(1)`：后者在无帧指针的
 * 优化产物里常取到垃圾（实测 `caller=(below-all)+0x0`）。回溯只在 ≥阈值 的
 * 分配上做（自举里几千次），成本可忽略。`FramesToSkip=1` 跳过本函数自身，
 * 于是 frames[0] = `aura_mem_alloc`/`aura_mem_realloc` 的调用者 = Aura 级
 * 分配点，frames[1] = 其调用者。 */
static int aura_big_site(void) {
    void *fr[AURA_BIG_FRAMES];
    unsigned long long h;
    int k, f, got;
    for (f = 0; f < AURA_BIG_FRAMES; f++) { fr[f] = NULL; }
#ifdef _WIN32
    /* skip=2：跳过本函数 + 记录的调用者（`aura_mem_alloc`/`aura_mem_realloc`），
     * 于是 fr[0]=分配入口本身、fr[1]=其调用者（多为 C 包装 `aura_malloc`），
     * fr[2..]=Aura 级调用点与其上层。键取 fr[2..4]。 */
    got = (int)CaptureStackBackTrace(2, AURA_BIG_FRAMES, fr, NULL);
#else
    fr[0] = __builtin_return_address(0);
    got = 1;
#endif
    for (f = got; f < AURA_BIG_FRAMES; f++) { fr[f] = NULL; }
    h = ((unsigned long long)fr[2] >> 4) * 0x9E3779B97F4A7C15ULL;
    h ^= ((unsigned long long)fr[3] >> 4) * 0xC2B2AE3D27D4EB4FULL;
    h ^= ((unsigned long long)fr[4] >> 4) * 0x165667B19E3779F9ULL;
    for (k = 0; k < 16; k++) {
        int s = (int)((h + (unsigned long long)k) & (AURA_BIG_SITES - 1));
        if (g_big_key[s][2] == fr[2] && g_big_key[s][3] == fr[3] &&
            g_big_key[s][4] == fr[4]) {
            return s;
        }
        if (g_big_key[s][0] == NULL) {
            for (f = 0; f < AURA_BIG_FRAMES; f++) { g_big_key[s][f] = fr[f]; }
            return s;
        }
    }
    return -1;
}

static int aura_big_bidx(void *p) {
    unsigned long long h = ((unsigned long long)p >> 4) * 0x9E3779B97F4A7C15ULL;
    return (int)(h & (AURA_BIG_BUCK - 1));
}

static void aura_big_record(void *p, int64_t n) {
    int s, b;
    AuraBigNode *nd;
    aura_big_init();
    if (!g_big_on || !p || n < g_big_min) return;
    g_big_all_total += 1;
    g_big_all_bytes += n;
    s = aura_big_site();
    if (s < 0) { g_big_overflow += 1; return; }
    g_big_calls[s] += 1;
    g_big_bytes[s] += n;
    if (n > g_big_max[s]) g_big_max[s] = n;
    if (g_big_freelist) {
        nd = g_big_freelist;
        g_big_freelist = nd->next;
    } else if (g_big_pool_n < AURA_BIG_POOL) {
        nd = &g_big_pool[g_big_pool_n++];
    } else {
        g_big_overflow += 1;
        return;
    }
    nd->p = p;
    nd->sz = n;
    nd->site = s;
    b = aura_big_bidx(p);
    nd->next = g_big_buckets[b];
    g_big_buckets[b] = nd;
    g_big_live_n[s] += 1;
    g_big_live_b[s] += n;
    g_big_live_total += 1;
    g_big_live_bytes += n;
}

static void aura_big_forget(void *p) {
    int b;
    AuraBigNode *nd, *prev = NULL;
    if (g_big_on != 1 || !p) return;
    b = aura_big_bidx(p);
    for (nd = g_big_buckets[b]; nd; nd = nd->next) {
        if (nd->p == p) {
            if (prev) { prev->next = nd->next; } else { g_big_buckets[b] = nd->next; }
            g_big_live_n[nd->site] -= 1;
            g_big_live_b[nd->site] -= nd->sz;
            g_big_live_total -= 1;
            g_big_live_bytes -= nd->sz;
            nd->p = NULL;
            nd->next = g_big_freelist;
            g_big_freelist = nd;
            return;
        }
        prev = nd;
    }
}

static void aura_big_dump(void) {
    int i, j, best;
    if (!g_big_on) return;
    fprintf(stderr,
        "[aura] ==== big alloc census (AURA_BIG_CENSUS=1, >=%lldKB) ====\n",
        (long long)(g_big_min / 1024));
#ifdef _WIN32
    /* AOT 产物被 strip，运行时只能给「最近 C 符号 + 原始地址」；原始地址减去
     * imagebase 即 RVA，可离线对照 `<exe>.map`（AURA_LINK_MAP=1）落到 Aura 函数。 */
    fprintf(stderr, "[aura] imagebase=%p  (RVA = addr - imagebase)\n",
            (void *)GetModuleHandleA((const char *)0));
#endif
    fprintf(stderr,
        "[aura] big allocs: %lld calls / %lld MB ; live: %lld blocks / %lld MB ; overflow=%lld\n",
        (long long)g_big_all_total, (long long)(g_big_all_bytes / (1024 * 1024)),
        (long long)g_big_live_total, (long long)(g_big_live_bytes / (1024 * 1024)),
        (long long)g_big_overflow);
    fprintf(stderr, "[aura] top-20 sites by LIVE bytes (site = 分配点栈帧):\n");
    for (i = 0; i < 20; i++) {
        best = -1;
        for (j = 0; j < AURA_BIG_SITES; j++) {
            if (g_big_key[j][0] && g_big_live_b[j] > 0 &&
                (best < 0 || g_big_live_b[j] > g_big_live_b[best])) {
                best = j;
            }
        }
        if (best < 0) break;
        fprintf(stderr,
            "   #%d live=%lld blk/%lldMB calls=%lld tot=%lldMB max=%lldKB\n"
            "      @2=%p(%s+0x%zx) @3=%p(%s+0x%zx) @4=%p(%s+0x%zx) @5=%p(%s+0x%zx)\n",
            i + 1,
            (long long)g_big_live_n[best],
            (long long)(g_big_live_b[best] / (1024 * 1024)),
            (long long)g_big_calls[best],
            (long long)(g_big_bytes[best] / (1024 * 1024)),
            (long long)(g_big_max[best] / 1024),
            g_big_key[best][2], aura_attr_who(g_big_key[best][2]),
            aura_attr_off(g_big_key[best][2]),
            g_big_key[best][3], aura_attr_who(g_big_key[best][3]),
            aura_attr_off(g_big_key[best][3]),
            g_big_key[best][4], aura_attr_who(g_big_key[best][4]),
            aura_attr_off(g_big_key[best][4]),
            g_big_key[best][5], aura_attr_who(g_big_key[best][5]),
            aura_attr_off(g_big_key[best][5]));
        g_big_live_b[best] = 0;   /* 打印即清，下一组不重复 */
    }
    fprintf(stderr, "[aura] top-10 sites by TOTAL bytes:\n");
    for (i = 0; i < 10; i++) {
        best = -1;
        for (j = 0; j < AURA_BIG_SITES; j++) {
            if (g_big_key[j][0] && g_big_bytes[j] > 0 &&
                (best < 0 || g_big_bytes[j] > g_big_bytes[best])) {
                best = j;
            }
        }
        if (best < 0) break;
        fprintf(stderr,
            "   #%d calls=%lld tot=%lldMB live=%lld blk/%lldMB avg=%lldKB"
            " @2=%p(%s+0x%zx) @3=%p(%s+0x%zx)\n",
            i + 1,
            (long long)g_big_calls[best],
            (long long)(g_big_bytes[best] / (1024 * 1024)),
            (long long)g_big_live_n[best],
            (long long)(g_big_live_b[best] / (1024 * 1024)),
            (long long)(g_big_calls[best] > 0
                        ? g_big_bytes[best] / g_big_calls[best] / 1024 : 0),
            g_big_key[best][2], aura_attr_who(g_big_key[best][2]),
            aura_attr_off(g_big_key[best][2]),
            g_big_key[best][3], aura_attr_who(g_big_key[best][3]),
            aura_attr_off(g_big_key[best][3]));
        g_big_bytes[best] = 0;
    }
    fflush(stderr);
}

/* ── [P0] 退出泄漏报告（AURA_LEAK_REPORT=1）────────────────────────
 * P0 验收设施：退出时走一遍 CRT 进程堆（MSVC malloc 即进程默认堆），
 * 对每个 BUSY 块按 magic 识别「本运行时的负载」，汇总：
 *   · size 直方图（与分配侧同一套桶口径）+ 总块数/总字节；
 *   · rc 直方图（P1 起区分「无 release 发射」与「漏 release」：
 *     rc=1 多为从未进容器的临时物，rc>1 为被容器持有的共享块）；
 *   · top-20 最大存活块（ptr/size/rc， ptr 可离线反查归属）。
 * 热路径零成本：只在退出时走一次堆；默认关闭。
 * ─────────────────────────────────────────────────────────────── */
#define AURA_LEAK_TOPN 20

static void aura_leak_report_atexit(void) {
#ifdef _WIN32
    HANDLE heap = GetProcessHeap();
    PROCESS_HEAP_ENTRY ent;
    int64_t total = 0, nblocks = 0;
    int64_t bucket_calls[7], bucket_bytes[7];
    int64_t rc_hist[6];        /* rc=1, 2, 3-5, 6-31, 32-1023, >=1024 */
    void *top_ptr[AURA_LEAK_TOPN];
    int64_t top_sz[AURA_LEAK_TOPN];
    int32_t top_rc[AURA_LEAK_TOPN];
    int i, j;

    memset(bucket_calls, 0, sizeof(bucket_calls));
    memset(bucket_bytes, 0, sizeof(bucket_bytes));
    memset(rc_hist, 0, sizeof(rc_hist));
    for (i = 0; i < AURA_LEAK_TOPN; i++) {
        top_ptr[i] = NULL;
        top_sz[i] = 0;
        top_rc[i] = 0;
    }

    if (heap) {
        memset(&ent, 0, sizeof(ent));
        while (HeapWalk(heap, &ent)) {
            AuraMemHdr *h;
            int64_t sz;
            int32_t rc;
            int b;
            if (!(ent.wFlags & PROCESS_HEAP_ENTRY_BUSY)) {
                continue;
            }
            if (ent.cbData < (DWORD)(sizeof(AuraMemHdr) + 1)) {
                continue;
            }
            h = (AuraMemHdr *)ent.lpData;
            if (!aura_hdr_magic_ok(h)) {
                continue;   /* 非本运行时负载（CRT 自身/并发原语等） */
            }
            sz = h->h.size;
            rc = h->h.rc;
            total += sz;
            nblocks += 1;
            b = 6;
            if (sz <= 64) b = 0;
            else if (sz <= 256) b = 1;
            else if (sz <= 1024) b = 2;
            else if (sz <= 8192) b = 3;
            else if (sz <= 65536) b = 4;
            else if (sz <= (1 << 20)) b = 5;
            bucket_calls[b] += 1;
            bucket_bytes[b] += sz;
            if (rc <= 1) rc_hist[0] += 1;
            else if (rc == 2) rc_hist[1] += 1;
            else if (rc <= 5) rc_hist[2] += 1;
            else if (rc <= 31) rc_hist[3] += 1;
            else if (rc <= 1023) rc_hist[4] += 1;
            else rc_hist[5] += 1;
            /* top-N 最大块（简单插入排序维护） */
            if (sz > top_sz[AURA_LEAK_TOPN - 1]) {
                for (j = AURA_LEAK_TOPN - 1; j > 0 && sz > top_sz[j - 1]; j--) {
                    top_sz[j] = top_sz[j - 1];
                    top_ptr[j] = top_ptr[j - 1];
                    top_rc[j] = top_rc[j - 1];
                }
                top_sz[j] = sz;
                top_ptr[j] = (void *)((char *)h + sizeof(AuraMemHdr));
                top_rc[j] = rc;
            }
        }
    }

    fprintf(stderr, "[aura] ==== ARC leak report (AURA_LEAK_REPORT=1) ====\n");
    fprintf(stderr,
        "[aura] live heap blocks: %lld (payload %lld MB)\n",
        (long long)nblocks, (long long)(total / (1024 * 1024)));
    fprintf(stderr,
        "[aura] size buckets (blocks/MB): <=64:%lld/%lld <=256:%lld/%lld "
        "<=1K:%lld/%lld <=8K:%lld/%lld <=64K:%lld/%lld <=1M:%lld/%lld >1M:%lld/%lld\n",
        (long long)bucket_calls[0], (long long)(bucket_bytes[0] / (1024 * 1024)),
        (long long)bucket_calls[1], (long long)(bucket_bytes[1] / (1024 * 1024)),
        (long long)bucket_calls[2], (long long)(bucket_bytes[2] / (1024 * 1024)),
        (long long)bucket_calls[3], (long long)(bucket_bytes[3] / (1024 * 1024)),
        (long long)bucket_calls[4], (long long)(bucket_bytes[4] / (1024 * 1024)),
        (long long)bucket_calls[5], (long long)(bucket_bytes[5] / (1024 * 1024)),
        (long long)bucket_calls[6], (long long)(bucket_bytes[6] / (1024 * 1024)));
    fprintf(stderr,
        "[aura] rc histogram (blocks): rc=1:%lld rc=2:%lld rc=3-5:%lld "
        "rc=6-31:%lld rc=32-1023:%lld rc>=1024:%lld\n",
        (long long)rc_hist[0], (long long)rc_hist[1], (long long)rc_hist[2],
        (long long)rc_hist[3], (long long)rc_hist[4], (long long)rc_hist[5]);
    fprintf(stderr, "[aura] top-%d largest live blocks (ptr size rc):\n", AURA_LEAK_TOPN);
    for (i = 0; i < AURA_LEAK_TOPN; i++) {
        if (!top_ptr[i]) {
            break;
        }
        fprintf(stderr, "   live %p %lld %ld\n", top_ptr[i],
                (long long)top_sz[i], (long)top_rc[i]);
    }
    fprintf(stderr,
        "[aura] arc stats: retain=%lld release=%lld freed=%lld "
        "noop(era=%lld lit=%lld rgn=%lld boxed=%lld) bad_magic=%lld "
        "rgn_reused=%lld mem_free_calls=%lld\n",
        (long long)g_arc_retain_calls, (long long)g_arc_release_calls,
        (long long)g_arc_freed, (long long)g_arc_noop_era,
        (long long)g_arc_noop_lit, (long long)g_arc_noop_rgn,
        (long long)g_arc_noop_boxed, (long long)g_arc_bad_magic,
        (long long)g_arc_rgn_reused,
        (long long)g_aura_mem_free_calls);
    fflush(stderr);
#else
    fprintf(stderr, "[aura] ARC leak report: Windows-only (HeapWalk); skipped.\n");
#endif
}

/// 复制一份以 NUL 结尾的字符串（受内存上限管辖）。
char *aura_mem_strdup(const char *s) {
    if (!s) {
        s = "";
    }
    size_t n = strlen(s);
    AURA_ATTR_RECORD(__builtin_return_address(0), (int64_t)(n + 1));
    char *d = (char *)aura_scr_alloc((int64_t)n + 1);
    memcpy(d, s, n + 1);
    return d;
}

/* 分配一份新的、以 NUL 结尾的字符串副本。
 *
 * 为什么不能用共享 static 缓冲：AOT 字符串是「值语义」，返回值可能被调用方
 * 长期持有（如词法器字段 `src`、解析器缓存的 token 文本）。若返回 static
 * 缓冲，后续任意一次字符串运算都会就地覆盖此前所有「字符串」的内容，表现为
 * 字段读出来变成别的文本甚至空串。代价是这些副本不会被释放（AOT 运行时暂无
 * 字符串 GC）。 */
static char *aura_dup_n(const char *s, size_t n) {
    if (aura_sc_enabled()) {
        aura_sc_note(1, (int64_t)n, __builtin_return_address(0));
    }
    char *out = (char *)aura_scr_alloc((int64_t)n + 1);
    if (!out) return (char *)"";
    if (s && n > 0) memcpy(out, s, n);
    out[n] = '\0';
    return out;
}

const char *aura_to_str(int64_t x) {
    /* 小整数缓存：AOT 运行时没有 GC，而 int→String 是最高频的分配之一
     * （编译器 IR 发射会产生千万级此类小串）。0..1023 用静态表以完全避免分配。
     *
     * 缓存槽按 **16 字节对齐**：Plan A 用「真实指针低位为 0」区分装箱整数
     * （见 argv 拷贝处的同类注释），若返回非对齐内部指针会被误判为装箱整数。
     * 槽内容只写一次、此后只读，可安全在多次调用间共享。 */
    if (x >= 0 && x < 1024) {
        /* [R152/D5] 数组已提到文件作用域（g_int_cache），供 ARC 注册长生区间。 */
        if (!g_int_cache_ready[x]) {
            snprintf(g_int_cache[x], 16, "%lld", (long long)x);
            g_int_cache_ready[x] = 1;
        }
        return g_int_cache[x];
    }
    /* 其余按实际位数分配（原先固定 64 字节，对绝大多数数字都过度分配）。 */
    char tmp[24];
    int n = snprintf(tmp, sizeof(tmp), "%lld", (long long)x);
    if (n < 0) return "";
    char *buf = (char *)aura_mem_alloc((int64_t)n + 1);
    if (!buf) return "";
    memcpy(buf, tmp, (size_t)n + 1);
    return buf;
}

const char *aura_to_str_float(double x) {
    char *buf = (char *)aura_mem_alloc(64);
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
    /* [P0/D5] 旧实现返回 static 缓冲 —— 连续两次 readLine 的结果会互相
     * 覆盖（同 aura_env_get 的历史教训）。改为带头直堆分配：每次独立
     * 副本、可被 ARC 管辖；EOF 仍返回 NULL。 */
    static char buf[4096];
    if (fgets(buf, sizeof(buf), stdin)) {
        size_t len = strlen(buf);
        if (len > 0 && buf[len - 1] == '\n') {
            buf[len - 1] = '\0';
        }
        char *out = (char *)aura_mem_alloc_heap((int64_t)len + 1);
        if (!out) return NULL;
        if (len > 0) memcpy(out, buf, len);
        out[len] = '\0';
        return out;
    }
    return NULL;
}

/** IO.readAll() — 读取所有 stdin 输入（不含换行符） */
const char *aura_io_readAll(void) {
    static char buf[65536];
    size_t total = 0;
    int c;
    while ((c = fgetc(stdin)) != EOF && total < sizeof(buf) - 1) {
        if (c != '\n' && c != '\r') {
            buf[total++] = (char)c;
        }
    }
    buf[total] = '\0';
    return buf;
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

/* `CString` 的逆操作：C 字符串指针 → Aura `String`。
 *
 * AOT 下 Aura `String` **就是** NUL 结尾的 `i8*`（项目既定 ABI），所以这里是
 * 恒等返回 —— 与旧的零拷贝路径行为完全一致，不产生任何复制。
 * VM（字节码）路径下由 `seed/compiler/src/vm/native.rs::native_builtin_read_cstr`
 * 实现（拷出一份真 `String`，带类型标签），二者语义对齐。
 *
 * 用途：`EmitBuffer.sbBuild()` 用它把原生缓冲区的字节显式变成 `String`。 */
const char *ReadCStr(const char *p) { return p ? p : ""; }

/* `CString(text)` / `CStr(text)` — Aura `String` → C 字符串指针（FFI 入口）。
 *
 * AOT 下 Aura `String` **就是** NUL 结尾的 `i8*`（与 `ReadCStr` 同一既定 ABI），
 * 因此这里恒等返回、不复制。Aura 侧定义见 `aura/core/aura/lang/prelu.aura`：
 *     fun CString(text: String): String { return Builtin.cstr(text) }
 * 其中 `Builtin.cstr` 是**仅 VM 有实现**的原生（`native_builtin_cstring`）；
 * AOT 自举编译时 `CString` 被当作外部符号发射（`declare i8* @CString(i8*)`），
 * 若 C 运行时没有定义，链接期即报
 *     `lld-link: error: undefined symbol: CString`
 * （自举 `PhotonHatCompile` 实测：`EmitBuffer.sbAdd` 一路都走 `CString`）。 */
const char *CString(const char *s) { return s ? s : ""; }
const char *CStr(const char *s) { return CString(s); }

int64_t aura_string_charCodeAt(const char *s, int64_t idx) {
    if (!s || idx < 0 || (size_t)idx >= strlen(s)) return -1;
    return (unsigned char)s[idx];
}

/* `String.fromCharCode(code)`（companion/静态方法）→ `aura_string_fromCharCode`。
 *
 * 此前没有任何实现：AOT 编译自举驱动时调用点发 `@String_fromCharCode`（未声明、
 * 未定义）→ llc 报 `use of undefined value '@String_fromCharCode'`。
 * 与 `charAt` 一样按字节值处理（Aura 的 Char 是单字节码元），
 * 并且**每次分配独立 2 字节**（不能返回静态缓冲，否则多次调用互相覆盖，
 * 同 `aura_env_get` 的教训）。
 * [P0/D5] 改为 `aura_mem_alloc_heap`（带 ARC 头、跨 RegionReset 存活的
 * 直堆分配）—— 行为与旧 malloc 完全一致（独立副本），且纳入 ARC 管辖。 */
const char *aura_string_fromCharCode(int64_t code) {
    char *p = (char *)aura_mem_alloc_heap(2);
    if (!p) return "";
    p[0] = (char)(code & 0xFF);
    p[1] = 0;
    return p;
}

/* 单字符字符串缓存（256 个字节值）。
 *
 * AOT 发射器把字符串索引 `s[i]` / `.charAt(i)` 落到 `aura_string_charAt`。
 * 词法器对源码的**每个字符**都会反复调用（`peek` / `advance` / `skipTrivia`），
 * 若每次 `aura_dup_n(s + idx, 1)` 分配 1 字节串，AOT（无 GC）下解析 600KB
 * 源码就会产生千万级永久分配 —— 实测「链接阶段 480MB」的主要来源。
 *
 * 槽位 16 字节对齐：Plan A 用「真实指针低位为 0」区分装箱整数，
 * 非对齐的内部指针会被误判（见 argv 拷贝处同类注释）。字符内容只写一次、
 * 之后只读，可在多次调用间安全共享。
 */
static const char *aura_char_cache(unsigned char c) {
    /* [R152/D5] 数组已提到文件作用域（g_char_cache_slots），供 ARC 注册长生区间。 */
    if (!g_char_cache_ready[c]) {
        g_char_cache_slots[c][0] = (char)c;
        g_char_cache_slots[c][1] = '\0';
        g_char_cache_ready[c] = 1;
    }
    return g_char_cache_slots[c];
}

const char *aura_string_charAt(const char *s, int64_t idx) {
    if (!s || idx < 0 || (size_t)idx >= strlen(s)) return "";
    return aura_char_cache((unsigned char)s[idx]);
}

/* [T3-SUBSTR 2026-10-21] 源串长度的单条 memo。
 *
 * `aura_string_substring` 旧实现每次都 `strlen(s)` 求全长（用于把 `end` 夹到
 * 串尾），于是「对一个长串切 N 刀」是 O(N·|s|)：
 *   writer 段实测：重定位串 2.29 MB 切 63,692 条，
 *   `parseRelocEntries` 20.3 s + `composeRelocations` 20.5 s = 占 writer 62%；
 *   前端 `HatParser` 对 10.9 MB .hat 逐 token 切片是同一病灶。
 *
 * 归约到「同一源串连续切 N 刀只算一次 strlen」：命中条件为指针相等 +
 * `s[len] == '\0'`（长度正确的必要条件）。后者 1 字节读取把「地址被复用成
 * 另一个串」的陈旧长度挡在门外 —— 长度真变了则 s[len] 非 NUL，立刻重算。
 * 末位校验读的正是原串自己的终止符位置，不会越过任何合法串的分配。 */
static const char *g_ss_src = NULL;
static size_t g_ss_len = 0;
static int g_ss_memo = -1;   /* -1 未初始化；0 = 关闭（AURA_SS_MEMO=0，回退旧 strlen） */

static size_t aura_str_len_cached(const char *s) {
    if (g_ss_memo < 0) {
        const char *e = getenv("AURA_SS_MEMO");
        g_ss_memo = (e && e[0] == '0') ? 0 : 1;
    }
    if (g_ss_memo == 0) {
        return strlen(s);
    }
    if (s == g_ss_src && s[g_ss_len] == '\0') {
        return g_ss_len;
    }
    size_t n = strlen(s);
    g_ss_src = s;
    g_ss_len = n;
    return n;
}

const char *aura_string_substring(const char *s, int64_t start, int64_t end) {
    if (!s) return "";
    /* 语义与旧实现逐位一致，只是 strlen 走 memo（见上）。 */
    size_t len = aura_str_len_cached(s);
    AURA_ATTR_RECORD(__builtin_return_address(0), 16);
    if (start < 0) start = 0;
    if (end > (int64_t)len) end = len;
    if (start > end) return "";
    return aura_dup_n(s + start, (size_t)(end - start));
}

const char *aura_string_toUpperCase(const char *s) {
    if (!s) return "";
    size_t len = strlen(s);
    char *buf = (char *)aura_mem_alloc((int64_t)len + 1);
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
    char *buf = (char *)aura_mem_alloc((int64_t)len + 1);
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
    /* [P0/D5] 早退路径旧为 `return s`（借用返回）—— 违反 D5 所有权契约
     * （P2 发射器把调用结果当新持有）。改为新副本，语义不变。 */
    if (!s || !from) return aura_dup_n(s ? s : "", s ? strlen(s) : 0);
    size_t from_len = strlen(from);
    if (from_len == 0) return aura_dup_n(s, strlen(s));
    size_t to_len = strlen(to);

    size_t i = 0;
    size_t j = 0;
    /* 栈上临时缓冲（旧为 static —— 重入/并发互相覆盖；64KB 栈帧在
     * 64MB 栈预算内，且本函数为叶子调用）。 */
    char buf[65536];
    /* ⚠️ 长度必须提到循环外：`i < strlen(s)` 会让每次迭代都重扫整个源串（O(n²)）。 */
    size_t slen = strlen(s);
    while (i < slen && j < sizeof(buf) - 1) {
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
    /* [R131-DIAG] concat 由 AOT 直接从 Aura 调用点发射 ⇒ ra0 = Aura 调用点
     * （记录已开关化，AURA_ATTR_DUMP 未设时仅一次静态标志检查）。 */
    AURA_ATTR_RECORD(__builtin_return_address(0), (int64_t)(a_len + b_len + 1));

    // 必须返回新分配内存：同一表达式内的链式拼接（`a + b + c`）会把前一步的
    // 结果再当作输入，若返回共享 static 缓冲则前后互相覆盖。
    int64_t out_len = (int64_t)a_len + (int64_t)b_len;
    if (out_len > 256) {
        aura_lc_note(out_len, __builtin_return_address(0));
    }
    if (aura_sc_enabled()) {
        aura_sc_note(0, out_len, __builtin_return_address(0));
    }
    char *out = (char *)aura_scr_alloc(out_len + 1);
    if (!out) return "";
    if (a && a_len > 0) {
        memcpy(out, a, a_len);
    }
    if (b && b_len > 0) {
        memcpy(out + a_len, b, b_len);
    }
    out[a_len + b_len] = '\0';
    g_chain_last_out = out;
    aura_chain_note(a, out_len, out);
    /* cs_ra = 本函数的调用者返回地址，即「是哪一句 Aura 代码在做拼接」。 */
    aura_chain_post_check(out_len, __builtin_return_address(0));
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
    /* [P0/D5] 旧实现返回 static buf[64] —— 多次调用互相覆盖（同
     * aura_env_get 教训）。改为带头直堆分配：每次独立副本。 */
    char tmp[64];
    time_t t = (time_t)timestamp;
    struct tm tm;
#ifdef _WIN32
    localtime_s(&tm, &t);
#else
    localtime_r(&tm, &t);
#endif
    strftime(tmp, sizeof(tmp), "%Y-%m-%d %H:%M:%S", &tm);
    return aura_dup_n(tmp, strlen(tmp));
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
// AOT IR 生成的新命名别名（aura_lang_std_IO_println 等）
void aura_lang_std_IO_println(const char *s) { aura_println(s); }
void aura_lang_std_IO_print(const char *s) { aura_print(s); }
void aura_lang_std_IO_puts(const char *s) { aura_puts(s); }
/* std 风格包装器：IO.readLine() 的调用点符号 */
const char *aura_lang_std_IO_readLine(void) { return aura_io_readLine(); }
/* std 风格包装器：IO.readAll() 的调用点符号 */
const char *aura_lang_std_IO_readAll(void) { return aura_io_readAll(); }
/* Json 模块存根（LSP/Debugger 使用，返回空字符串或 0） */
const char *aura_lang_std_Json_remove(const char *json, const char *key) { (void)json; (void)key; return ""; }
int64_t aura_abs_wrapper(int64_t x) { return x < 0 ? -x : x; }
double aura_sqrt_wrapper(double x) { return sqrt(x); }
double aura_pow_wrapper(double b, double e) { return pow(b, e); }
int64_t toInt(double x) { return (int64_t)x; }
double toFloat(int64_t x) { return (double)x; }
/* 字符串 → 数值：AOT 侧 `toInt("42")` / `toFloat("3.14")` 的调用点符号。
   非法输入返回 0（与 VM 的宽松语义一致，不抛异常）。 */
int64_t aura_str_to_int(const char *s) {
    if (!s) return 0;
    return (int64_t)strtoll(s, NULL, 10);
}
double aura_str_to_float(const char *s) {
    if (!s) return 0.0;
    return strtod(s, NULL);
}
/* std 风格包装器：String.toInt() / String.toFloat() 的调用点符号 */
int64_t aura_lang_std_String_toInt(const char *s) {
    return aura_str_to_int(s);
}
double aura_lang_std_String_toFloat(const char *s) {
    return aura_str_to_float(s);
}
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
    if (v == 0) return "";  /* null → 空串，避免 strlen(null) 崩溃 */
    if (v & (uint64_t)1) {
        return aura_to_str((int64_t)((int64_t)v >> 1));
    }
    /* 偶数：真实字符串数据指针 */
    return (const char *)(uintptr_t)v;
}

/* `i8* + i8*` 的**运行时分派**（Plan A）。
 *
 * 两个 `i8*` 操作数在编译期不可区分：可能是装箱整数，也可能是真实字符串指针。
 * 静态判定不可靠——调用返回值（如 `s.substring(a, b)` / `s.indexOf(...)`）的 Aura
 * 类型常常在 `func_ret_aura_types` 里缺失，于是 `HirBinOp::Add` 一律落入「装箱整数
 * 相加」分支，把两个字符串指针 `ptrtoint` 后 `add` 再 `(v<<1)|1` 装箱回 `i8*`：
 * 结果是一条野指针，写回对象字段后，下一次 `strlen` 直接 0xC0000005。
 *
 * 判定依据是 Plan A 的低位标记：装箱整数恒为奇数，分配器返回的指针至少 2 字节
 * 对齐故低位恒为 0。两侧都奇 ⇒ 都是装箱整数；否则按字符串拼接。
 * 一奇一偶是源码层的类型错误，此处统一走字符串拼接（与旧的编译期启发式一致）。
 */
const char *aura_add_i8(const char *a, const char *b) {
    uint64_t pa = (uint64_t)(uintptr_t)a;
    uint64_t pb = (uint64_t)(uintptr_t)b;
    if ((pa & (uint64_t)1) && (pb & (uint64_t)1)) {
        int64_t x = (int64_t)((int64_t)pa >> 1);
        int64_t y = (int64_t)((int64_t)pb >> 1);
        int64_t sum = x + y;
        return (const char *)(uintptr_t)(((uint64_t)((int64_t)sum << 1)) | (uint64_t)1);
    }
    return aura_string_concat(a, (int64_t)strlen(a), b, (int64_t)strlen(b));
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
// value: i8*（AOT 统一的不透明值句柄）, typeName: i8*（C 字符串）
//
// AOT 下没有运行时类型标签（数值为 Plan A 低位标记的整数，其余为真实指针），
// 因此这里是**保守近似**：只保证「不崩溃」，并在常见情况下给出正确判定。
// 旧实现无条件把 value 当作「指向类型标签的指针」解引用，遇到装箱整数
// （奇数地址）会直接段错误 —— `when (v) { is Int -> … }` 崩溃的根因。
_Bool aura_isOfType(const void *value, const char *typeName) {
    if (!typeName) return 0;
    // Plan A：低位为 1 → 装箱整数
    if ((uintptr_t)value & (uintptr_t)1) {
        return strcmp(typeName, "Int") == 0
            || strcmp(typeName, "Long") == 0
            || strcmp(typeName, "Short") == 0
            || strcmp(typeName, "Byte") == 0
            || strcmp(typeName, "Number") == 0
            || strcmp(typeName, "Any") == 0;
    }
    if (!value) {
        return strcmp(typeName, "Null") == 0
            || strcmp(typeName, "Nothing") == 0
            || strcmp(typeName, "Any") == 0;
    }
    // 真实指针：字符串 / 集合 / 对象。无法区分，按「最宽泛」处理：
    // 仅当目标是 String/Any/Object 时返回真，其余为假。
    return strcmp(typeName, "String") == 0
        || strcmp(typeName, "Any") == 0
        || strcmp(typeName, "Object") == 0;
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

// [P0/D2] 旧 ARC 入口（`Memory.arcIncrement/arcDecrement` 的 C 侧落点，
// emit.rs 可对其发 atomicrmw）保持名字兼容，语义统一转发到上方门控
// retain/release（D1 时代闸门 + D2b .rdata + D2c 区域防线 + magic）。
//
// ⚠️ 旧实现把 refcount 写在**负载首字节**（ptr 即头，AuraArcHeader），
// 与 D2「头在负载前 16 字节」的布局不符，且无任何闸门 —— 对 .rdata
// 字面量/静态缓存指针调用会直接写坏只读段或他人数据。原全仓零调用者；
// 转发后行为由 D2 流程接管，原子性由 AURA_ARC_ATOMIC 统一控制（D6）。
// aura_arc_decrement 返回递减后的引用计数（<=0 表示已释放）。
void aura_arc_increment(const void *ptr) {
    aura_arc_retain(ptr);
}

int aura_arc_decrement(const void *ptr) {
    return aura_arc_release(ptr);
}

// 协程挂起（AOT 下为 no-op，VM 运行时处理）
// ctx: 协程上下文指针
void aura_coroutine_yield(const void *ctx) {
    (void)ctx;
    // AOT 下协程由 VM 运行时管理，此处为空操作
}

// 堆分配（返回指针）：统一走带上限检查的分配器
void *aura_malloc(int64_t size) {
    if (size <= 0) return NULL;
    return aura_mem_alloc(size);
}

// 堆释放：与 aura_malloc 配对
void aura_free(const void *ptr) {
    aura_mem_free((void *)ptr);
}

/* [R90] 链接补齐：Collections.aura 的 Memory.alloc/copy 经 emit.rs 第一
 * 路径（Builtin 分支）发射 @alloc/@copy，而 HIR 对 extern interface 成员
 * 未带 Builtin 标记时走裸名外调。此处提供同语义桩，避免 /NODEFAULTLIB 下
 * 无 libc 兜底导致未定义符号。
 * StringOps_strcmp 同理（Collections_strEquals 调 StringOps.strcmp）。
 * free 不在这里定义：aura_mem_free 自身调 libc free，重定义会递归。
 * emit.rs 已改为 @aura_free（见 R90 emit.rs 补丁）。 */
void *alloc(int64_t n) { return aura_malloc(n); }
void copy(int64_t dst, int64_t src, int64_t n) {
    memcpy((void *)(uintptr_t)dst, (const void *)(uintptr_t)src, (size_t)n);
}
int StringOps_strcmp(const char *a, const char *b) {
    if (!a || !b) return a == b ? 0 : (a ? -1 : 1);
    return strcmp(a, b);
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
    /* ⚠️ 必须**每次返回独立分配**，不能返回 static 缓冲。
     *
     * 旧实现用 `static char env_buf[512]`：Aura 侧 `Env.get(name, def)` 的返回值
     * 是裸指针，多次调用拿到的是**同一块内存**，后一次调用会覆盖前一次的结果 ——
     * 实测 AOT 编译的
     *     val a = Env.get("TESTA", "?"); val b = Env.get("TESTB", "?"); val c = Env.get("TESTC", "?")
     * 打印 `a=CCC b=CCC c=CCC`（三个变量指向同一缓冲，值都是最后一次的）。
     * 直接自举驱动 `PhotonDriver.aura` 里正是连续三次 `Env.get`
     *（PHIR / OUT / MODULE），导致 `phirPath` 读成模块名、报
     * `Failed to read .phir file: 01_hello_world`。
     *
     * 泄漏量可忽略（环境变量读取次数极少），换来正确的值语义。
     * def 参数是 Aura 侧 `Env.get(name, default)` 的默认值，这里不使用。
     *
     * [P0/D5] 改为 `aura_mem_alloc_heap`（带 ARC 头、不受区域路由影响的
     * 直堆分配）：与旧 malloc 语义逐字节一致（每次独立副本、永生存活），
     * 且 ARC 可管辖（P2 起槽位 release 能真正回收它）。 */
    const char *v = getenv(name ? name : "");
    if (!v) return "";
    size_t n = strlen(v) + 1;
    char *p = (char *)aura_mem_alloc_heap((int64_t)n);
    if (!p) return "";
    memcpy(p, v, n);
    return p;
}

/*
 * env_get: 运行时 obj 中 `Heap_init` 调用的符号名（Photon 编译器映射 `Env.get` → `env_get`）。
 * 原运行时 obj 中是 stub（返回 rdx 或自指针），导致 `Heap_init` 读不到 `AURA_HEAP_MB`，
 * 堆初始化失败（`__list_alloc` 返回 0），所有列表操作输出 "null"。
 * 此实现转发到 `aura_env_get`，提供真正的环境变量读取。
 */
const char *env_get(const char *name) {
    return aura_env_get(name);
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

struct AuraDynList {
    int64_t len;
    int64_t cap;
    const char **items;
};

/* [P0/P1] 元素入场券：元素统一为 i8* 句柄 —— Plan A 低位标记的装箱整数
 * （(v<<1)|1，奇数）不参与 ARC；真实指针（字符串/列表/映射/类实例，均为
 * aura_mem_alloc 产物带头）入表即 retain（容器成为共同持有者，D3）。
 * aura_arc_retain 自带四道防线，外来/静态指针安全 no-op。 */
static void aura_list_elem_retain(const char *s) {
    aura_arc_retain(s);
}

/* [P0/P1] 元素退场：被容器移除/覆盖且所有权不转出时 release。
 * 所有权**转出**的路径（listPop 返回值、removeAt 的语义由调用方接管）
 * 不得调用本函数 —— 见各调用点注释。 */
static void aura_list_elem_release(const char *s) {
    aura_arc_release(s);
}

static AuraDynList *aura_dynlist_new(int64_t cap) {
    /* [P0/D2] 结构体与 items 数组均经 aura_mem_alloc（带头、rc=1）——
     * 列表自身所有权随返回值转移给调用者（D4），无需额外 retain。
     * [P2] 结构体块打 LIST 标签：rc 归零时析构级联（释放 items 数组 +
     * 逐元素 release），把「列表死亡 = 元素引用全部释放」变成机制。 */
    AuraDynList *l = (AuraDynList *)aura_mem_alloc((int64_t)sizeof(AuraDynList));
    if (!l) return NULL;
    if (cap < 4) cap = 4;
    l->len = 0;
    l->cap = cap;
    l->items = (const char **)aura_mem_alloc((int64_t)sizeof(const char *) * cap);
    /* [P2] 结构体块打 LIST 标签：rc 归零时析构级联（释放 items 数组 +
     * 逐元素 release），把「列表死亡 = 元素引用全部释放」变成机制。
     * ⚠️ 仅堆时代可打：区域时代（g_rgn_active）的 aura_mem_alloc 返回
     * region bump 指针，其前方 16B 是别的块的数据/区块元数据，写 flags
     * 会直接破坏区域内存（2026-10-20 实测：前端类型名 "Boolean" 被踩成
     * "Booleam" ⇒ .hat 全量偏移 ⇒ 产物缺符号）。区域对象由 RegionReset
     * 整块回收，无需逐对象析构。 */
    if (l->items && !g_rgn_active) {
        AuraMemHdr *lh = (AuraMemHdr *)((char *)l - sizeof(AuraMemHdr));
        lh->h.flags = (uint16_t)((lh->h.flags & (uint16_t)~AURA_MEM_TAG_MASK) | AURA_MEM_TAG_LIST);
    }
    return l;
}

static void aura_dynlist_push(AuraDynList *l, const char *s) {
    if (!l) return;
    if (l->len >= l->cap) {
        int64_t ncap = l->cap * 2;
        const char **ni =
            (const char **)aura_mem_realloc((void *)l->items, (int64_t)sizeof(const char *) * ncap);
        if (!ni) return;
        l->items = ni;
        l->cap = ncap;
    }
    /* [P1] 入场 retain（装箱整数/字面量/静态指针被 retain 内部防线跳过）。
     * NULL 归一为 ""（.rdata 字面量，永生）—— 无需 retain。
     *
     * ⚠️ 装箱检查内联在调用点：Plan A 下 range 类列表的元素绝大多数是
     * 装箱整数（自举后端实测 3.68 亿次 retain 调用中 ~89% 是装箱跳过），
     * 而本运行时按 -O0 编译（compile_std_cffi 无 -O）⇒ 每次调用都是真实
     * 函数调用。内联这道检查把 retain 调用量降两个数量级；真实指针交给
     * retain 内部的时代/.rdata/magic 防线（语义不变）。 */
    if (s && !((uintptr_t)s & 1)) {
        aura_arc_retain(s);
    } else {
        g_arc_noop_boxed++;
    }
    l->items[l->len++] = s ? s : "";
}

/** 左闭右开的 substr 到新分配的 C 字符串 */
static const char *aura_substr_dup(const char *s, size_t n) {
    char *out = (char *)aura_scr_alloc((int64_t)n + 1);
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

/* substringFrom(start)：1 参 `substring(start)` —— 取 [start, 末尾)。
 *
 * 1 参形态此前在 rust AOT 下没有对应实现：调用点按 2 参 C 符号
 * `aura.lang.std.String.substring(s, from, to)` 发射，缺失的 `to` 落成
 * 未初始化寄存器（实测 HatParser.aura parseRet 注记：「1 参…会得到空串
 * （VM 正常）」，当时靠把全部调用点改写成显式 2 参规避）。补齐语义后，
 * 1 参调用点直接正确，无需再改写。 */
const char *aura_lang_std_String_substringFrom(const char *s, int64_t start) {
    if (!s) return "";
    return aura_string_substring(s, start, (int64_t)strlen(s));
}

/* legacy C 符号别名：AOT 发射端 translate_to_legacy_c 把 std 调用点符号
 * 归一为 aura_string_* 后按此名链接（与其余字符串方法同一约定）。 */
const char *aura_string_substringFrom(const char *s, int64_t start) {
    return aura_lang_std_String_substringFrom(s, start);
}

/* substringAt(from, len)：与 Aura 侧 `String.substringAt` 同语义（[from, from+len)）。
 * 同为映射表缺失项：ProcessNative.aura 等调用点此前发射裸名 `@substringAt`
 * → `llc: use of undefined value`。 */
const char *aura_lang_std_String_substringAt(const char *s, int64_t from, int64_t len) {
    if (!s || len < 0) return "";
    return aura_string_substring(s, from, from + len);
}

const char *aura_string_substringAt(const char *s, int64_t from, int64_t len) {
    return aura_lang_std_String_substringAt(s, from, len);
}

const char *aura_lang_std_String_charAt(const char *s, int64_t idx) {
    return aura_string_charAt(s, idx);
}

/**
 * 字符码点（O(1)：不做 strlen，靠 NUL 判定越界）。
 *
 * 与 `aura_string_charCodeAt` 语义一致（越界返回 -1），但去掉了
 * `strlen(s)`，使 Aura 侧逐字符扫描从 O(n) 降到 O(1)——
 * 否则 `while (i < s.length) { s[i] ... }` 这类惯用法是 O(n²)。
 */
int64_t aura_lang_std_String_charCodeAt(const char *s, int64_t idx) {
    /* 越界闸门：`idx < 0` 只挡住「正确的」负数（0xFFFFFFFFFFFFFFFF）。
     *
     * 自举驱动（Photon 后端 AOT）存在一处 int32→int64 的**零扩展**代码生成缺陷：
     * `text.length - 1` 在 `text.length == 0` 时得 int32 `-1`（0xFFFFFFFF），
     * 传给 `charCodeAt(int64_t)` 时被零扩展为 0xFFFFFFFF（4,294,967,295，正数）
     * 而非符号扩展为 0xFFFFFFFFFFFFFFFF（-1）。于是 `idx < 0` 判定通过，
     * 函数直接读 `s[0xFFFFFFFF]`——偏移远超任何合法字符串 → 跨页访问违例
     * （VEH 现场：`s=0x7FF7F115A410`、`idx=0xFFFFFFFF`、`target=s+idx` 落在
     *  相邻模块镜像 0x7FF8… 的未映射区，code 0xc0000005）。
     *
     * 此处用 int32 上界兜住所有「被零扩展的负数」：合法字符串下标不会超过
     * int32 最大值（Aura 的 String.length 返回 Int），因此该判定零误报、
     * 零开销（单条比较），把崩溃转化为正常的「越界 → -1」语义。
     * 根因（代码生成零扩展）另行修复，此处仅为运行时兜底。 */
    if (!s || idx < 0 || idx > 0x7FFFFFFF) return -1;
    unsigned char c = (unsigned char)s[idx];
    if (c == 0) return -1;
    return (int64_t)c;
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
    /* [P0/D5] 未命中分隔符时旧为 `return s`（借用返回）—— 违反 D5 所有
     * 权契约。改为新副本，语义不变。 */
    if (!s || !sep || !sep[0]) return aura_dup_n(s ? s : "", s ? strlen(s) : 0);
    const char *p = strstr(s, sep);
    if (!p) return aura_dup_n(s, strlen(s));
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
    /* [P0/D5] 三处修正：
     *  ① 旧实现写 static buf[4096] 后返回它 —— 多次调用互相覆盖
     *     （同 aura_env_get 的历史教训）；
     *  ② 旧实现在 `sl >= width` / `pl == 0` 时直接 `return s`（借用返回）
     *     —— 违反 D5「返回的 String 一律是新持有或永生」，P2 发射器会把
     *     结果当新持有存入槽位 ⇒ rc 少 1 ⇒ 提前释放；
     *  ③ 统一改为 aura_dup_n 新副本（区域时代进区域、后端进堆，路由正确）。
     * 语义不变（值语义），仅分配位置与所有权契约修正。 */
    if (!s) return "";
    size_t sl = strlen(s);
    if ((int64_t)sl >= width) {
        return aura_dup_n(s, sl);
    }
    size_t pl = pad ? strlen(pad) : 0;
    if (pl == 0) {
        return aura_dup_n(s, sl);
    }
    char buf[4096];
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
    return aura_dup_n(buf, need);
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

/* 旧命名别名：`translate_to_legacy_c` 会把 `aura_lang_std_String_<m>` 翻译为
 * `aura_string_<m>` 发射调用（见 compiler/src/codegen/aot/runtime.rs 的
 * cffi_signature 表），因此必须同时提供旧名符号。 */
int64_t aura_string_indexOf(const char *s, const char *sub) {
    return aura_lang_std_String_indexOf(s, sub);
}

int64_t aura_string_lastIndexOf(const char *s, const char *sub) {
    return aura_lang_std_String_lastIndexOf(s, sub);
}

int64_t aura_string_countChar(const char *s, const char *ch) {
    return aura_lang_std_String_countChar(s, ch);
}

const char *aura_string_substringBefore(const char *s, const char *sep) {
    return aura_lang_std_String_substringBefore(s, sep);
}

const char *aura_string_substringAfter(const char *s, const char *sep) {
    return aura_lang_std_String_substringAfter(s, sep);
}

const char *aura_string_replaceAll(const char *s, const char *from, const char *to) {
    return aura_lang_std_String_replaceAll(s, from, to);
}

const char *aura_string_padStart(const char *s, int64_t width, const char *pad) {
    return aura_lang_std_String_padStart(s, width, pad);
}

const void *aura_string_split(const char *s, const char *sep) {
    return aura_lang_std_String_split(s, sep);
}

/** 字符串内容相等（AOT 字符串比较统一走这里，避免结构体按位比较） */
int aura_lang_std_String_equals(const char *a, const char *b) {
    if (!a || !b) return a == b ? 1 : 0;
    return strcmp(a, b) == 0 ? 1 : 0;
}

/** Plan A 感知的 Any `==`（emit 的 i8*==i8* 统一比较走这里）。
 *
 *  此前一切 i8* 比较都降到 String_equals：`HashMap.valuesEqual` 的
 *  `a == b`（Any 接收者）在操作数是**装箱整数**（`(v<<1)|1` 的伪指针，
 *  实测 85）时按字符串解引用 → 段错误（AOT 载体 HashMap.get 首次
 *  取桶即崩）。按低位标记区分：
 *    - 奇数（装箱整数）：值相等当且仅当标记值相同；
 *    - 偶数（真实指针）：null 参与时不等（双 null 已在 emit 判空分支处理，
 *      这里 a==b 已覆盖）；同指针相等；否则按字符串内容比较。
 *  真实字符串指针恒为偶数（分配器 ≥2 字节对齐），故对纯 String 操作数
 *  的行为与 String_equals 完全一致（无回归）。 */
int aura_any_equals(uint64_t a, uint64_t b) {
    if (a == b) return 1;
    if ((a & (uint64_t)1) || (b & (uint64_t)1)) return 0;
    if (!a || !b) return 0;
    return strcmp((const char *)(uintptr_t)a, (const char *)(uintptr_t)b) == 0 ? 1 : 0;
}

/* ── Console I/O ── */
void aura_lang_std_Console_print(int64_t msg) {
    if (!msg) return;
    const char *s = (const char *)msg;
    size_t len = strlen(s);
    if (len > 0) {
        fwrite(s, 1, len, stdout);
    }
}
void aura_lang_std_Console_println(int64_t msg) {
    aura_lang_std_Console_print(msg);
    fputc('\n', stdout);
    fflush(stdout);
}

/* ── 极简字符串键值表（`Map<String, Any>`；AOT 下 Map 表示为 i8*） ── */

static const char *aura_strdup(const char *s) {
    return aura_mem_strdup(s);
}

struct AuraDynMap {
    int64_t len;
    int64_t cap;
    const char **keys;
    const char **vals;
};

static AuraDynMap *aura_map_new(int64_t cap) {
    AuraDynMap *m = (AuraDynMap *)aura_mem_alloc((int64_t)sizeof(AuraDynMap));
    if (!m) return NULL;
    if (cap < 4) cap = 4;
    m->len = 0;
    m->cap = cap;
    m->keys = (const char **)aura_mem_alloc((int64_t)sizeof(const char *) * cap);
    m->vals = (const char **)aura_mem_alloc((int64_t)sizeof(const char *) * cap);
    /* [P2] 打 MAP 标签：rc 归零时析构级联（释放 keys/vals 数组 + 逐元素）。
     * 同 dynlist_new：仅堆时代可打（区域时代无头，写 flags = 破坏区域内存）。 */
    if (m->keys && m->vals && !g_rgn_active) {
        AuraMemHdr *mh = (AuraMemHdr *)((char *)m - sizeof(AuraMemHdr));
        mh->h.flags = (uint16_t)((mh->h.flags & (uint16_t)~AURA_MEM_TAG_MASK) | AURA_MEM_TAG_MAP);
    }
    return m;
}

/* [P2] 标签块析构级联：rc 归零时由 aura_arc_release 调用（aura_mem_free
 * 之前）。语义 = D3「容器消失 ⇒ 其持有的元素引用全部释放」：
 *   · LIST：逐元素 release，再释放 items 数组；
 *   · MAP ：逐键/值 release，再释放两个数组；
 *   · ENV ：闭包 env（布局 { ptr fn; i64 count; caps… }），逐捕获项
 *           release（非指针/装箱值被 retain 内部防线跳过）。
 * 级联可递归（列表的列表），深度 = 结构嵌套深度（编译器 IR 的 DAG
 * 父子边是拥有边、owner 反指按 D7 不 retain ⇒ 不成环）。 */
static void aura_arc_destroy_tagged(void *p, uint16_t tag) {
    if (!p) return;
    if (tag == AURA_MEM_TAG_LIST) {
        AuraDynList *l = (AuraDynList *)p;
        int64_t i;
        if (l->items) {
            for (i = 0; i < l->len; i++) {
                aura_arc_release(l->items[i]);
            }
            aura_mem_free((void *)l->items);
        }
    } else if (tag == AURA_MEM_TAG_MAP) {
        AuraDynMap *m = (AuraDynMap *)p;
        int64_t i;
        if (m->keys && m->vals) {
            for (i = 0; i < m->len; i++) {
                aura_arc_release(m->keys[i]);
                aura_arc_release(m->vals[i]);
            }
            aura_mem_free((void *)m->keys);
            aura_mem_free((void *)m->vals);
        }
    } else if (tag == AURA_MEM_TAG_ENV) {
        /* 布局：{ ptr fn; i64 count; T1; T2; … } —— count 在 +8。 */
        int64_t count = *(const int64_t *)((const char *)p + 8);
        int64_t i;
        for (i = 0; i < count; i++) {
            const char *cap = *(const char *const *)((const char *)p + 16 + i * 8);
            aura_arc_release(cap);
        }
    }
}

/// [P2] 闭包 env 分配（发射器 AURA_ARC=1 时使用）：
/// 分配 `{ ptr fn; i64 count; caps… }` 布局的块、写 count、打 ENV 标签。
/// 旧路径（AURA_ARC 未设）保持 `aura_malloc` + 手写字段，产物逐字节不变。
/// ⚠️ 仅堆时代打标签（区域时代 aura_mem_alloc 无 16B 头，见 dynlist_new）。
void *aura_arc_alloc_env(int64_t size, int64_t count) {
    char *raw = (char *)aura_mem_alloc(size);
    if (!raw) return NULL;
    *(int64_t *)(raw + 8) = count;
    if (!g_rgn_active) {
        AuraMemHdr *h = (AuraMemHdr *)((char *)raw - sizeof(AuraMemHdr));
        h->h.flags = (uint16_t)((h->h.flags & (uint16_t)~AURA_MEM_TAG_MASK) | AURA_MEM_TAG_ENV);
    }
    return raw;
}

const void *aura_lang_std_Collections_mutableMapOf(void) {
    return (const void *)aura_map_new(8);
}

const void *aura_lang_std_Collections_emptyMap(void) {
    return (const void *)aura_map_new(4);
}

/** 就地写入（Map 为可变堆对象，调用方拿到的是同一指针）
 * [P1] 覆盖语义：旧值所有权被容器放弃 ⇒ release 旧值 + retain 新值；
 * 键插入时经 aura_strdup 新建（rc=1，所有权随存储转移）无需 retain；
 * 值插入（新键）同样 retain（与键对称，D3 容器持有 +1）。 */
void aura_lang_std_Collections_mapSet(const void *map, const char *key, const void *value) {
    AuraDynMap *m = (AuraDynMap *)map;
    if (!m || !key) return;
    const char *v = value ? (const char *)value : "";
    for (int64_t i = 0; i < m->len; i++) {
        if (m->keys[i] && strcmp(m->keys[i], key) == 0) {
            aura_list_elem_release(m->vals[i]);
            aura_list_elem_retain(v);
            m->vals[i] = v;
            return;
        }
    }
    if (m->len >= m->cap) {
        int64_t ncap = m->cap * 2;
        const char **nk =
            (const char **)aura_mem_realloc((void *)m->keys, (int64_t)sizeof(const char *) * ncap);
        const char **nv =
            (const char **)aura_mem_realloc((void *)m->vals, (int64_t)sizeof(const char *) * ncap);
        if (!nk || !nv) return;
        m->keys = nk;
        m->vals = nv;
        m->cap = ncap;
    }
    m->keys[m->len] = aura_strdup(key);
    aura_list_elem_retain(v);
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

/** Map.getOrDefault(map, key, default) — 返回 key 对应的值，不存在则返回 default */
const void *aura_lang_std_Collections_getOrDefault(const void *map, const char *key, const void *default_val) {
    const void *result = aura_lang_std_Collections_mapGet(map, key);
    if (result == "" || result == NULL) {
        return default_val ? default_val : "";
    }
    return result;
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

/* ── Map 调用点补全：`Collections.mapContainsKey` / `mapContainsValue` /
 *    `mapKeys` / `mapValues` / `hashMapPut` ──────────────────────────────────
 *
 * 这 5 个名字在 **VM（Rust native）** 侧早已存在（`std_collections.rs` 的
 * `nat_map_contains_key` / `nat_map_contains_value` / `nat_map_keys` /
 * `nat_map_values` / `nat_hash_map_put`），但 AOT 的「调用点符号层」此前只导出
 * `Collections.mapContains`。一旦前端把 `m.containsKey(k)` / `m.put(k, v)` 降级为
 * `Collections.mapContainsKey(...)` / `Collections.hashMapPut(...)`，AOT 产物就会在
 * 链接期报 undefined symbol —— 这正是「集合映射表两头不齐」的那半边。
 *
 * 这里按同一 Plan A ABI（`AuraDynMap = { len, cap, keys, vals }`，键/值为 C 字符串）
 * 补齐，语义与 VM native 一一对应：
 *   * `mapContainsKey`  → 存在该键
 *   * `mapContainsValue`→ 存在该值（先比指针，再按字符串内容比）
 *   * `mapKeys` / `mapValues` → 新建 `AuraDynList`（与 VM 返回 `Value::List` 对齐）
 *   * `hashMapPut`      → 存在则覆盖、否则追加，返回同一映射句柄
 *     （AOT 的映射是可变结构体，原地写入即可，无需返回新对象） */
int aura_lang_std_Collections_mapContainsKey(const void *map, const char *key) {
    return aura_lang_std_Collections_mapContains(map, key);
}

int aura_lang_std_Collections_mapContainsValue(const void *map, const void *value) {
    const AuraDynMap *m = (const AuraDynMap *)map;
    if (!m) return 0;
    const char *v = value ? (const char *)value : "";
    for (int64_t i = 0; i < m->len; i++) {
        const char *cur = m->vals[i];
        if (cur == v) return 1;
        if (cur && v && strcmp(cur, v) == 0) return 1;
    }
    return 0;
}

const void *aura_lang_std_Collections_mapKeys(const void *map) {
    const AuraDynMap *m = (const AuraDynMap *)map;
    AuraDynList *out = aura_dynlist_new(4);
    if (!m) return (const void *)out;
    for (int64_t i = 0; i < m->len; i++) {
        aura_dynlist_push(out, m->keys[i] ? m->keys[i] : "");
    }
    return (const void *)out;
}

const void *aura_lang_std_Collections_mapValues(const void *map) {
    const AuraDynMap *m = (const AuraDynMap *)map;
    AuraDynList *out = aura_dynlist_new(4);
    if (!m) return (const void *)out;
    for (int64_t i = 0; i < m->len; i++) {
        aura_dynlist_push(out, m->vals[i] ? m->vals[i] : "");
    }
    return (const void *)out;
}

/* [P1] ARC 化：键/值入表均 retain（容器成为共同持有者）；覆盖旧值时
 * 先 release 旧值（所有权被容器放弃，D3）。键不复制、直接 retain 后存
 * （保留原 ABI 行为，仅补引用计数）。 */
const void *aura_lang_std_Collections_hashMapPut(const void *map, const void *key, const void *value) {
    AuraDynMap *m = (AuraDynMap *)map;
    if (!m) {
        m = aura_map_new(4);
        if (!m) return 0;
    }
    const char *k = key ? (const char *)key : "";
    const char *v = value ? (const char *)value : "";
    for (int64_t i = 0; i < m->len; i++) {
        if (m->keys[i] && strcmp(m->keys[i], k) == 0) {
            aura_list_elem_release(m->vals[i]);
            aura_list_elem_retain(v);
            m->vals[i] = v;
            return (const void *)m;
        }
    }
    if (m->len >= m->cap) {
        int64_t ncap = m->cap * 2;
        const char **nk = (const char **)aura_mem_alloc((int64_t)sizeof(const char *) * ncap);
        const char **nv = (const char **)aura_mem_alloc((int64_t)sizeof(const char *) * ncap);
        if (!nk || !nv) return (const void *)m;
        for (int64_t i = 0; i < m->len; i++) {
            nk[i] = m->keys[i];
            nv[i] = m->vals[i];
        }
        /* [P2] 旧数组归还（旧实现在此处直接覆盖指针 ⇒ 每次翻倍泄漏一份
         * 旧 keys/vals；map 扩容频繁时是可观滞留）。 */
        aura_mem_free((void *)m->keys);
        aura_mem_free((void *)m->vals);
        m->keys = nk;
        m->vals = nv;
        m->cap = ncap;
    }
    aura_list_elem_retain(k);
    aura_list_elem_retain(v);
    m->keys[m->len] = k;
    m->vals[m->len] = v;
    m->len++;
    return (const void *)m;
}

/** 列表按下标写入（越界则追加）
 * [P1] 覆盖语义：旧值所有权被容器放弃 ⇒ release 旧值 + retain 新值
 *（D3/D4 字段存/列表 set 规则）；越界追加路径经 push 已 retain。 */
void aura_lang_std_Collections_listSet(const void *list, int64_t idx, const void *value) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l) return;
    const char *v = value ? (const char *)value : "";
    if (idx >= 0 && idx < l->len) {
        aura_list_elem_release(l->items[idx]);
        aura_list_elem_retain(v);
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

/** 旧式名称兼容：aura_collections_pairOf */
const void *aura_collections_pairOf(const void *a, const void *b) {
    return aura_lang_std_Collections_pairOf(a, b);
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

/* listRemoveAt(list, idx)：删除下标 idx 处的元素并左移后续元素（len--）。
 * 2026-10-02 补齐：HIR 把 `remove(i)`/`removeAt(i)` 重路由到 `__list_remove_at`
 * （见 codegen/hir.rs），而 AOT 侧此前没有实现 → 自举编译器链接期
 * `undefined symbol: __list_remove_at`（实测 PhotonHatCompile 驱动构建）。
 *
 * [P1] 被移除元素的所有权处置：本函数返回**列表**而非被移除元素 ⇒ 元素
 * 所有权被容器放弃且无转出对象 ⇒ 容器职责 release（D3「从容器移除时持有者
 * 负责 release」）。对照：listPop 返回被弹出的元素 ⇒ 所有权转出给调用方，
 * C 侧**不得** release（见该函数注释）。 */
const void *aura_lang_std_Collections_listRemoveAt(const void *list, int64_t idx) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l || idx < 0 || idx >= l->len) return (const void *)l;
    aura_list_elem_release(l->items[idx]);
    for (int64_t i = idx; i + 1 < l->len; i++) {
        l->items[i] = l->items[i + 1];
    }
    if (l->len > 0) {
        l->len--;
    }
    return (const void *)l;
}

/* ── 迭代器链：filter / map / take ──
 *
 * 闭包实参是「env 句柄」（`i8*`）：首字段为函数指针，调用约定与发射器生成的
 * `__lambda_N(i8* env, <param>)` 一致（见 aot/Emit.aura 的闭包 ABI）。
 * 集合元素统一为 `i8*` 句柄：整数按 Plan A 低位标记装箱 `(v<<1)|1`，
 * 故先解箱成 int64_t 传给闭包，再把闭包结果装箱回写。 */
typedef int64_t (*AuraIterFn)(void *env, int64_t x);

static AuraIterFn aura_iter_fn(void *clo) {
    if (!clo) return NULL;
    return (AuraIterFn)(*(void **)clo);
}

/** Plan A 解箱：奇数（低位标记）→ `v >> 1`，偶数视为真实指针（此处按整数原值处理）。 */
static int64_t aura_iter_unbox(int64_t v) {
    if ((v & 1) != 0) return v >> 1;
    return v;
}

/** Plan A 装箱：`(v << 1) | 1`。 */
static const char *aura_iter_box(int64_t v) {
    return (const char *)(intptr_t)((((uint64_t)v) << 1) | 1ULL);
}

const void *aura_lang_std_Collections_filter(const void *list, const void *clo) {
    AuraDynList *out = aura_dynlist_new(4);
    const AuraDynList *l = (const AuraDynList *)list;
    AuraIterFn fn = aura_iter_fn((void *)clo);
    if (!l || !fn) return (const void *)out;
    int64_t i = 0;
    while (i < l->len) {
        int64_t arg = aura_iter_unbox((int64_t)(intptr_t)l->items[i]);
        /* 谓词闭包的返回类型是 `i1`（如 `icmp`），ABI 只保证低字节有效；
         * 直接按 int32 读取会把高位垃圾当「真」→ filter 恒全通过。 */
        int64_t keep = (int64_t)(uint8_t)fn((void *)clo, arg);
        if (keep != 0) {
            aura_dynlist_push(out, l->items[i]);
        }
        i = i + 1;
    }
    return (const void *)out;
}

const void *aura_lang_std_Collections_map(const void *list, const void *clo) {
    AuraDynList *out = aura_dynlist_new(4);
    const AuraDynList *l = (const AuraDynList *)list;
    AuraIterFn fn = aura_iter_fn((void *)clo);
    if (!l || !fn) return (const void *)out;
    int64_t i = 0;
    while (i < l->len) {
        int64_t arg = aura_iter_unbox((int64_t)(intptr_t)l->items[i]);
        int64_t r = (int64_t)(int32_t)fn((void *)clo, arg);
        aura_dynlist_push(out, aura_iter_box(r));
        i = i + 1;
    }
    return (const void *)out;
}

const void *aura_lang_std_Collections_take(const void *list, int64_t n) {
    AuraDynList *out = aura_dynlist_new(4);
    const AuraDynList *l = (const AuraDynList *)list;
    if (!l || n <= 0) return (const void *)out;
    int64_t i = 0;
    while (i < l->len && i < n) {
        aura_dynlist_push(out, l->items[i]);
        i = i + 1;
    }
    return (const void *)out;
}

/** list.pop()：弹出并返回末尾元素（AOT 下列表元素为 i8* 句柄）。
 *  空列表返回 NULL；返回值经 getAt 的逆转换还原为原类型。
 *
 *  [P1] 所有权**转出**：返回值即被弹出元素，调用方槽位成为新持有者
 *  （D4「函数返回值：所有权随返回值转移给调用者」）⇒ C 侧**不** release
 *  （与 listRemoveAt 的处置相反，那里所有权无转出对象）。 */
const void *aura_lang_std_Collections_listPop(const void *list) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l || l->len <= 0) return (const void *)0;
    l->len--;
    return (const void *)l->items[l->len];
}

/** 构造整数区间列表（对应 Aura `start..end` / `start..<end` / `start..=end`）。
 *  start/end/inclusive 均为 i32；元素以 **Plan A 低位标记** 装箱
 *  `((i<<1)|1)` 存入 AuraDynList —— 与 `aura_to_int_any` / 列表元素装箱约定
 *  一致。旧实现存原始整数：偶数被当成真实指针、奇数被当成已标记整数
 *  （`aura_to_int_any` 再右移一位），`for (i in 1..5)` 读出的值全错。 */
const void *aura_lang_std_Collections_range(int32_t start, int32_t end, int32_t inclusive) {
    AuraDynList *l = aura_dynlist_new(16);
    if (!l) return NULL;
    if (start <= end) {
        int64_t i = start;
        for (;;) {
            if (i > (int64_t)end) break;
            if (i == (int64_t)end && !inclusive) break;
            aura_dynlist_push(l, (const char *)(intptr_t)((i << 1) | 1));
            if (i == (int64_t)end) break;
            i++;
        }
    } else {
        int64_t i = start;
        for (;;) {
            if (i < (int64_t)end) break;
            if (i == (int64_t)end && !inclusive) break;
            aura_dynlist_push(l, (const char *)(intptr_t)((i << 1) | 1));
            if (i == (int64_t)end) break;
            i--;
        }
    }
    return (const void *)l;
}

/**
 * 集合元素句柄相等判定。
 *
 * 列表元素统一以 `i8*` 句柄存储（Plan A：整数为低位标记 `(v<<1)|1`）。
 * 旧实现直接 `strcmp` 比较：元素是整数句柄时会把 `0x3d` 之类当指针解引用，
 * `list.indexOf(30)` 因此触发访问违例（0xC0000005）。这里按句柄类型分流：
 * 双方都是标记整数 → 比数值；一方是整数 → 不等；否则按 C 字符串比内容。
 */
static int aura_handle_equals(const char *a, const char *b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    uintptr_t ua = (uintptr_t)a;
    uintptr_t ub = (uintptr_t)b;
    int ia = (ua & 1) != 0;
    int ib = (ub & 1) != 0;
    if (ia || ib) return ia == ib && ua == ub;
    return strcmp(a, b) == 0;
}

int64_t aura_lang_std_Collections_indexOf(const void *list, const void *value) {
    const AuraDynList *l = (const AuraDynList *)list;
    const char *v = (const char *)value;
    if (!l || !v) return -1;
    for (int64_t i = 0; i < l->len; i++) {
        const char *it = l->items[i];
        if (aura_handle_equals(it, v)) return i;
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

/* 旧命名别名：AOT 的 `translate_to_legacy_c` 把
 * `aura_lang_std_FileSystem_mkdirP` 翻译为 `aura_fs_mkdirP` 发射调用，
 * 因此必须同时提供旧名符号，否则链接期 undefined symbol。 */
int64_t aura_fs_mkdirP(const char *path) {
    return (int64_t)aura_lang_std_FileSystem_mkdirP(path);
}

/* ── aura.lang.std.Process.* ── */

/** 同步执行命令，返回退出码（AOT 下用系统 shell） */
int64_t aura_lang_std_Process_run(const char *cmd) {
    if (!cmd) return -1;
    int rc = system(cmd);
    return (int64_t)rc;
}

/* ── 命令行参数（AOT） ──
 * AOT 发射的 C 入口 `main` 在函数体最开始 store 宿主 argv 到 IR 全局变量
 * `aura_argc_global` / `aura_argv_global`（见 Emit.aura Phase C）。
 * `Process.arg(i)` / `Process.argCount()` 据此读取。 */

extern int aura_argc_global;
extern char **aura_argv_global;

int64_t aura_lang_std_Process_argCount(void) {
    return (int64_t)aura_argc_global;
}

const char *aura_lang_std_Process_arg(int64_t index) {
    if (index < 0 || index >= (int64_t)aura_argc_global) return "";
    if (!aura_argv_global || !aura_argv_global[index]) return "";
    /* 必须返回 malloc 副本：argv[i] 是 OS 命令行缓冲的内部指针，低 bit 奇偶不可控，
     * 而 Plan A 依赖「真实字符串指针恒为偶数」来区分装箱整数 ((v<<1)|1)。
     * 返回内部指针会被 aura_to_str_any 误判成装箱整数，打印出地址数字。 */
    return aura_strdup(aura_argv_global[index]);
}

/** 所有 argv 以 '\n' 连接（AOT 下的简化表示；VM 侧返回 List）。 */
const char *aura_lang_std_Process_args(void) {
    static char *joined = NULL;
    size_t total = 1;
    int i;
    if (joined) {
        aura_mem_free(joined);
        joined = NULL;
    }
    if (!aura_argv_global) return "";
    for (i = 0; i < aura_argc_global; i++) {
        if (aura_argv_global[i]) total += strlen(aura_argv_global[i]) + 1;
    }
    joined = (char *)aura_mem_alloc((int64_t)total);
    if (!joined) return "";
    joined[0] = '\0';
    for (i = 0; i < aura_argc_global; i++) {
        if (i > 0) strcat(joined, "\n");
        if (aura_argv_global[i]) strcat(joined, aura_argv_global[i]);
    }
    return joined;
}

/* 旧命名别名：`translate_to_legacy_c` 发射 `aura_process_*` 调用
 * （见 compiler/src/codegen/aot/runtime.rs 的 cffi_signature 表）。 */
int64_t aura_process_run(const char *cmd) {
    return aura_lang_std_Process_run(cmd);
}

int64_t aura_process_argCount(void) {
    return aura_lang_std_Process_argCount();
}

const char *aura_process_arg(int64_t index) {
    return aura_lang_std_Process_arg(index);
}

const char *aura_process_args(void) {
    return aura_lang_std_Process_args();
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
double aura_lang_std_Math_sqrt(double x) { return aura_math_sqrt(x); }
double aura_lang_std_Math_round(double x) { return aura_math_round(x); }
int64_t aura_lang_std_Math_abs(double x) { return aura_math_abs(x); }

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
    buf = (char *)aura_mem_alloc((int64_t)cap);
    if (!buf) {
        fclose(f);
        return "";
    }
    for (;;) {
        size_t n;
        if (len + 1 >= cap) {
            char *nb = (char *)aura_mem_realloc(buf, (int64_t)cap * 2);
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

/* ── aura.lang.std.StringBuilder.* ──
 *
 * 原生可变字符串缓冲区：句柄（i64，实为 AuraSb*）+ append/appendChar/appendInt
 * + length/finish/reset。用于在 AOT 无 GC 的运行时里避免「不可变 String 反复拼接」
 * 产生的海量中间串（编译器 IR 发射路径）。
 *
 * 契约（与 Rust VM std_sb.rs 对齐）：
 *   create()                 -> i64 句柄
 *   append(h, s)             -> i64 句柄（返回自身，便于链式）
 *   appendChar(h, ch)        -> i64 句柄
 *   appendInt(h, v)          -> i64 句柄
 *   length(h)                -> i64 当前字节长度
 *   finish(h)                -> i8* 字符串（**转移缓冲区所有权**，零拷贝；原句柄失效）
 *   reset(h)                 -> i64 句柄（清空，保留容量）
 *
 * 所有分配都走 aura_mem_alloc/realloc，保持内存闸门记账一致。
 * 句柄用 i64 承载指针，规避 Plan A「i8* 低位标记装箱整数」的历史坑。 */

typedef struct {
    char *buf;    /* 数据区（aura_mem_alloc 负载，16 字节对齐） */
    int64_t len;  /* 已用字节（不含结尾 NUL） */
    int64_t cap;  /* 容量（字节，含结尾可用位） */
} AuraSb;

/** 初始容量（字节）。 */
#define AURA_SB_INIT_CAP 256

static AuraSb *aura_sb_from_handle(int64_t handle) {
    return (AuraSb *)(intptr_t)handle;
}

/** 确保容量 >= need（几何增长，保证 append 摊还 O(1)）。 */
static void aura_sb_reserve(AuraSb *sb, int64_t need) {
    int64_t newcap;
    char *nb;
    if (!sb || sb->cap >= need) return;
    newcap = sb->cap > 0 ? sb->cap : AURA_SB_INIT_CAP;
    while (newcap < need) {
        newcap *= 2;
    }
    nb = (char *)aura_mem_realloc(sb->buf, newcap);
    if (!nb) return;
    sb->buf = nb;
    sb->cap = newcap;
}

/** 底层追加 n 字节（不含 NUL）。 */
static int64_t aura_sb_append_n(AuraSb *sb, const char *s, int64_t n) {
    if (!sb || !sb->buf) return 0;
    if (n < 0) n = 0;
    aura_sb_reserve(sb, sb->len + n + 1);
    if (s && n > 0) memcpy(sb->buf + sb->len, s, (size_t)n);
    sb->len += n;
    sb->buf[sb->len] = '\0';
    return sb->len;
}

int64_t aura_lang_std_StringBuilder_create(void) {
    AuraSb *sb = (AuraSb *)aura_mem_alloc((int64_t)sizeof(AuraSb));
    if (!sb) return 0;
    sb->buf = (char *)aura_mem_alloc(AURA_SB_INIT_CAP);
    if (!sb->buf) {
        sb->len = 0;
        sb->cap = 0;
        return (int64_t)(intptr_t)sb;
    }
    sb->buf[0] = '\0';
    sb->len = 0;
    sb->cap = AURA_SB_INIT_CAP;
    return (int64_t)(intptr_t)sb;
}

int64_t aura_lang_std_StringBuilder_append(int64_t handle, const char *text) {
    AuraSb *sb = aura_sb_from_handle(handle);
    if (!text) return handle;
    (void)aura_sb_append_n(sb, text, (int64_t)strlen(text));
    return handle;
}

int64_t aura_lang_std_StringBuilder_appendChar(int64_t handle, int16_t ch) {
    AuraSb *sb = aura_sb_from_handle(handle);
    char c = (char)ch;
    (void)aura_sb_append_n(sb, &c, 1);
    return handle;
}

int64_t aura_lang_std_StringBuilder_appendInt(int64_t handle, int32_t value) {
    AuraSb *sb = aura_sb_from_handle(handle);
    char tmp[32];
    int n = snprintf(tmp, sizeof(tmp), "%d", (int)value);
    if (n < 0) return handle;
    (void)aura_sb_append_n(sb, tmp, (int64_t)n);
    return handle;
}

int64_t aura_lang_std_StringBuilder_length(int64_t handle) {
    AuraSb *sb = aura_sb_from_handle(handle);
    return sb ? sb->len : 0;
}

const char *aura_lang_std_StringBuilder_finish(int64_t handle) {
    AuraSb *sb = aura_sb_from_handle(handle);
    char *out;
    if (!sb) return "";
    out = sb->buf;
    /* 转移所有权：句柄置为失效（buf=NULL）。缓冲区由调用方长期持有，
     * 结构体本身保留（AOT 无 GC；如需续用请 reset 重新分配）。 */
    sb->buf = NULL;
    sb->len = 0;
    sb->cap = 0;
    return out ? out : "";
}

int64_t aura_lang_std_StringBuilder_reset(int64_t handle) {
    AuraSb *sb = aura_sb_from_handle(handle);
    if (!sb) return handle;
    if (sb->buf) {
        sb->buf[0] = '\0';
        sb->len = 0;
    } else {
        sb->buf = (char *)aura_mem_alloc(AURA_SB_INIT_CAP);
        if (sb->buf) {
            sb->buf[0] = '\0';
            sb->cap = AURA_SB_INIT_CAP;
        } else {
            sb->cap = 0;
        }
        sb->len = 0;
    }
    return handle;
}

// ─────────────────────────────────────────────────────────────────────────────
// 缺失的纯 Aura 函数 stub（AOT 链接用）
// 这些函数现在是纯 Aura 实现，但 AOT 编译器仍生成外部符号引用。
// 长期方案：AOT 编译嵌入 stdlib 模块到原生代码。
// ─────────────────────────────────────────────────────────────────────────────

// aura.lang.concurrent.Coroutine.actorAlive
_Bool aura_lang_concurrent_Coroutine_actorAlive(int64_t id) {
    (void)id;
    return 1; // stub: 返回 true
}

// aura.lang.concurrent.Channel.newChannel(cap: Int) → Int
int64_t aura_lang_concurrent_Channel_newChannel(int64_t cap) {
    (void)cap;
    return (int64_t)aura_concurrent_newChannel(0);
}

// aura.lang.concurrent.Channel.channelSend(ch: Int, val: Any) → Unit
void aura_lang_concurrent_Channel_channelSend(int64_t ch, const char *val) {
    aura_concurrent_channelSend((int32_t)ch, val);
}

// aura.lang.concurrent.Channel.channelRecv(ch: Int) → Any
const char *aura_lang_concurrent_Channel_channelRecv(int64_t ch) {
    return (const char *)aura_concurrent_channelRecv((int32_t)ch);
}

// aura.lang.std.IO.fileExists (new-style name)
int aura_lang_std_IO_fileExists(const char *path) {
    return aura_io_fileExists(path);
}

// aura.lang.std.Ascii.isAlpha (new-style name)
_Bool aura_lang_std_Ascii_isAlpha(int16_t c) {
    return aura_ascii_isAlpha(c);
}

// aura.lang.std.Ascii.isDigit (new-style name)
_Bool aura_lang_std_Ascii_isDigit(int16_t c) {
    return aura_ascii_isDigit(c);
}

// aura.lang.std.Ascii.toUpper (new-style name)
int64_t aura_lang_std_Ascii_toUpper(int16_t c) {
    return aura_ascii_toUpper(c);
}

// aura.lang.std.Ascii.toLower (new-style name)
int64_t aura_lang_std_Ascii_toLower(int16_t c) {
    return aura_ascii_toLower(c);
}

// Builtin: intToPtr / ptrToInt
int64_t aura_lang_std_Builtin_intToPtr(int32_t value) {
    return (int64_t)value;
}

int32_t aura_lang_std_Builtin_ptrToInt(int64_t ptr) {
    return (int32_t)ptr;
}

// 旧式名称（兼容）
int64_t intToPtr(int32_t value) {
    return (int64_t)value;
}

int32_t ptrToInt(int64_t ptr) {
    return (int32_t)ptr;
}

// Builtin: ptrIsNull
_Bool ptrIsNull(int64_t ptr) {
    return ptr == 0;
}

// aura.lang.std.Collections.listContains (new-style name)
_Bool aura_lang_std_Collections_listContains(const void *list, const void *val) {
    return aura_collections_listContains(list, val);
}

// aura.lang.std.Collections.listIndexOf (new-style name)
int64_t aura_lang_std_Collections_listIndexOf(const void *list, const void *val) {
    return aura_collections_listIndexOf(list, val);
}

// ═════════════════════════════════════════════════════════════════════════════
// 并发运行时 — AOT 原生支持（registry-based API）
// 为 AOT 编译的并发原语提供 C 实现。
// 使用全局注册表将整数 ID 映射到实际对象指针。
// 底层调用 aura_syscalls.c 中的平台相关实现（Win32/POSIX）。
// ═════════════════════════════════════════════════════════════════════════════

#include <stdlib.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

// 外部声明（aura_syscalls.c 中的实现）
extern int64_t aura_mutex_new(void);
extern void aura_mutex_lock(int64_t id);
extern void aura_mutex_unlock(int64_t id);
extern int aura_mutex_trylock(int64_t id);
extern void aura_mutex_destroy(int64_t id);

extern int64_t aura_atomic_load(volatile int64_t *addr);
extern void aura_atomic_store(volatile int64_t *addr, int64_t val);
extern int64_t aura_atomic_add(volatile int64_t *addr, int64_t delta);
extern int64_t aura_atomic_sub(volatile int64_t *addr, int64_t delta);
extern int aura_atomic_cas(volatile int64_t *addr, int64_t expected, int64_t desired);

extern int64_t aura_rwlock_new(void);
extern void aura_rwlock_read_lock(int64_t id);
extern void aura_rwlock_write_lock(int64_t id);
extern void aura_rwlock_read_unlock(int64_t id);
extern void aura_rwlock_write_unlock(int64_t id);
extern void aura_rwlock_destroy(int64_t id);

extern int64_t aura_condvar_new(void);
extern void aura_condvar_wait(int64_t cv_id, int64_t mutex_id);
extern void aura_condvar_signal(int64_t cv_id);
extern void aura_condvar_broadcast(int64_t cv_id);
extern void aura_condvar_destroy(int64_t cv_id);

extern int64_t aura_barrier_new(int64_t count);
extern int64_t aura_barrier_wait(int64_t id);
extern void aura_barrier_destroy(int64_t id);

extern int64_t aura_thread_create(int64_t fn_id, int64_t arg);
/* 返回工作线程的返回值（Aura `Thread.join` 语义；实现见 aura_syscalls.c
 * 的 handle→结果槽注册表）。 */
extern int64_t aura_thread_join(int64_t id);
extern void aura_thread_sleep(int64_t ms);
extern int64_t aura_thread_id(void);
extern int64_t aura_thread_available_parallelism(void);

#define CONCURRENCY_MAX_IDS 256

// ── 注册表基础设施 ─────────────────────────────────────────

static void *concurrent_registry[CONCURRENCY_MAX_IDS];

// 句柄是**从 1 开始**的槽位号：0 保留为「无效句柄」。
//
// 契约来源（三条独立证据，全部要求 0 无效）：
//   1. 本文件/`aura_syscalls.c` 的每个消费者都以 `id <= 0` 判定无效
//      （`aura_mutex_lock`、`aura_barrier_wait`、`aura_thread_join` …）；
//   2. VM 侧 `concurrent_native.rs` 的 `NEXT_THREAD_ID` 初值为 1，
//      `aura/core/.../Future.aura` 的 `spawn` 返回堆指针并把 0 备注为失败；
//   3. 测试按 `handle > 0` 判有效（`future_chain.aura` 的 `f > 0`、
//      `promise_test.aura` 的 `fid > 0L`）。
// 旧实现返回裸下标 0 ⇒ AOT 下 `Future.spawn` 的首个句柄为 0 ⇒ 被误判为创建
// 失败（AOT 独有的 `Future.spawn 返回无效 ID`）。
static int64_t concurrent_alloc(void *obj) {
    for (int i = 0; i < CONCURRENCY_MAX_IDS; i++) {
        if (concurrent_registry[i] == NULL) {
            concurrent_registry[i] = obj;
            return (int64_t)i + 1;
        }
    }
    return -1;
}

static void *concurrent_get(int64_t id) {
    if (id < 1 || id > CONCURRENCY_MAX_IDS) return NULL;
    return concurrent_registry[id - 1];
}

static void concurrent_free_slot(int64_t id) {
    if (id < 1 || id > CONCURRENCY_MAX_IDS) return;
    concurrent_registry[id - 1] = NULL;
}

// ── Mutex (aura_lang_concurrent_Mutex_*) ────────────────────

int64_t aura_lang_concurrent_Mutex_new(void) {
    int64_t ptr = aura_mutex_new();
    if (ptr <= 0) return -1;
    int64_t id = concurrent_alloc((void *)(uintptr_t)ptr);
    return id;
}

void aura_lang_concurrent_Mutex_lock(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) aura_mutex_lock((int64_t)(uintptr_t)obj);
}

void aura_lang_concurrent_Mutex_unlock(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) aura_mutex_unlock((int64_t)(uintptr_t)obj);
}

_Bool aura_lang_concurrent_Mutex_tryLock(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) return aura_mutex_trylock((int64_t)(uintptr_t)obj);
    return 0;
}

void aura_lang_concurrent_Mutex_destroy(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) aura_mutex_destroy((int64_t)(uintptr_t)obj);
    concurrent_free_slot(id);
}

// ── Atomic (aura_lang_concurrent_Atomic_*) ──────────────────

int64_t aura_lang_concurrent_Atomic_new(int64_t initial) {
    volatile int64_t *p = (volatile int64_t *)malloc(sizeof(int64_t));
    if (!p) return -1;
    aura_atomic_store(p, initial);
    int64_t id = concurrent_alloc((void *)p);
    return id;
}

int64_t aura_lang_concurrent_Atomic_load(int64_t id) {
    volatile int64_t *p = (volatile int64_t *)concurrent_get(id);
    if (p) return aura_atomic_load(p);
    return 0;
}

void aura_lang_concurrent_Atomic_store(int64_t id, int64_t val) {
    volatile int64_t *p = (volatile int64_t *)concurrent_get(id);
    if (p) aura_atomic_store(p, val);
}

int64_t aura_lang_concurrent_Atomic_add(int64_t id, int64_t delta) {
    volatile int64_t *p = (volatile int64_t *)concurrent_get(id);
    if (p) return aura_atomic_add(p, delta);
    return delta;
}

int64_t aura_lang_concurrent_Atomic_sub(int64_t id, int64_t delta) {
    volatile int64_t *p = (volatile int64_t *)concurrent_get(id);
    if (p) return aura_atomic_sub(p, delta);
    return -delta;
}

_Bool aura_lang_concurrent_Atomic_cas(int64_t id, int64_t expected, int64_t desired) {
    volatile int64_t *p = (volatile int64_t *)concurrent_get(id);
    if (p) return aura_atomic_cas(p, expected, desired);
    return 0;
}

void aura_lang_concurrent_Atomic_destroy(int64_t id) {
    volatile int64_t *p = (volatile int64_t *)concurrent_get(id);
    if (p) free((void *)p);
    concurrent_free_slot(id);
}

// ── RwLock (aura_lang_concurrent_RwLock_*) ──────────────────

int64_t aura_lang_concurrent_RwLock_new(void) {
    int64_t ptr = aura_rwlock_new();
    if (ptr <= 0) return -1;
    return concurrent_alloc((void *)(uintptr_t)ptr);
}

void aura_lang_concurrent_RwLock_readLock(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) aura_rwlock_read_lock((int64_t)(uintptr_t)obj);
}

void aura_lang_concurrent_RwLock_writeLock(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) aura_rwlock_write_lock((int64_t)(uintptr_t)obj);
}

void aura_lang_concurrent_RwLock_readUnlock(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) aura_rwlock_read_unlock((int64_t)(uintptr_t)obj);
}

void aura_lang_concurrent_RwLock_writeUnlock(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) aura_rwlock_write_unlock((int64_t)(uintptr_t)obj);
}

void aura_lang_concurrent_RwLock_destroy(int64_t id) {
    void *obj = concurrent_get(id);
    if (obj) aura_rwlock_destroy((int64_t)(uintptr_t)obj);
    concurrent_free_slot(id);
}

// ── Condvar (aura_lang_concurrent_Condvar_*) ────────────────

int64_t aura_lang_concurrent_Condvar_new(void) {
    int64_t ptr = aura_condvar_new();
    if (ptr <= 0) return -1;
    return concurrent_alloc((void *)(uintptr_t)ptr);
}

void aura_lang_concurrent_Condvar_wait(int64_t cv_id, int64_t mutex_id) {
    void *cv_obj = concurrent_get(cv_id);
    void *m_obj = concurrent_get(mutex_id);
    if (cv_obj && m_obj) aura_condvar_wait((int64_t)(uintptr_t)cv_obj, (int64_t)(uintptr_t)m_obj);
}

void aura_lang_concurrent_Condvar_signal(int64_t cv_id) {
    void *cv_obj = concurrent_get(cv_id);
    if (cv_obj) aura_condvar_signal((int64_t)(uintptr_t)cv_obj);
}

void aura_lang_concurrent_Condvar_broadcast(int64_t cv_id) {
    void *cv_obj = concurrent_get(cv_id);
    if (cv_obj) aura_condvar_broadcast((int64_t)(uintptr_t)cv_obj);
}

void aura_lang_concurrent_Condvar_destroy(int64_t cv_id) {
    void *cv_obj = concurrent_get(cv_id);
    if (cv_obj) aura_condvar_destroy((int64_t)(uintptr_t)cv_obj);
    concurrent_free_slot(cv_id);
}

// ── Barrier (aura_lang_concurrent_Barrier_*) ────────────────

int64_t aura_lang_concurrent_Barrier_new(int64_t count) {
    int64_t ptr = aura_barrier_new(count);
    if (ptr <= 0) return -1;
    return concurrent_alloc((void *)(uintptr_t)ptr);
}

int64_t aura_lang_concurrent_Barrier_wait(int64_t barrier_id) {
    void *obj = concurrent_get(barrier_id);
    if (obj) return aura_barrier_wait((int64_t)(uintptr_t)obj);
    return -1;
}

void aura_lang_concurrent_Barrier_destroy(int64_t barrier_id) {
    void *obj = concurrent_get(barrier_id);
    if (obj) aura_barrier_destroy((int64_t)(uintptr_t)obj);
    concurrent_free_slot(barrier_id);
}

// ── Semaphore (aura_lang_concurrent_Semaphore_*) ────────────
// Semaphore 使用 Mutex + Condvar 模拟实现

typedef struct {
    int64_t count;
    int64_t mutex_id;  // aura_lang_concurrent_Mutex_* 的 ID
    int64_t cv_id;     // aura_lang_concurrent_Condvar_* 的 ID
} AuraSemaphore;

int64_t aura_lang_concurrent_Semaphore_new(int64_t permits) {
    AuraSemaphore *sem = (AuraSemaphore *)malloc(sizeof(AuraSemaphore));
    if (!sem) return -1;
    sem->count = permits;
    sem->mutex_id = aura_lang_concurrent_Mutex_new();
    sem->cv_id = aura_lang_concurrent_Condvar_new();
    if (sem->mutex_id < 0 || sem->cv_id < 0) {
        free(sem);
        return -1;
    }
    return concurrent_alloc((void *)sem);
}

void aura_lang_concurrent_Semaphore_acquire(int64_t sem_id) {
    AuraSemaphore *sem = (AuraSemaphore *)concurrent_get(sem_id);
    if (!sem) return;
    aura_lang_concurrent_Mutex_lock(sem->mutex_id);
    while (sem->count <= 0) {
        aura_lang_concurrent_Condvar_wait(sem->cv_id, sem->mutex_id);
    }
    sem->count--;
    aura_lang_concurrent_Mutex_unlock(sem->mutex_id);
}

_Bool aura_lang_concurrent_Semaphore_tryAcquire(int64_t sem_id) {
    AuraSemaphore *sem = (AuraSemaphore *)concurrent_get(sem_id);
    if (!sem) return 0;
    aura_lang_concurrent_Mutex_lock(sem->mutex_id);
    if (sem->count > 0) {
        sem->count--;
        aura_lang_concurrent_Mutex_unlock(sem->mutex_id);
        return 1;
    }
    aura_lang_concurrent_Mutex_unlock(sem->mutex_id);
    return 0;
}

void aura_lang_concurrent_Semaphore_release(int64_t sem_id) {
    AuraSemaphore *sem = (AuraSemaphore *)concurrent_get(sem_id);
    if (!sem) return;
    aura_lang_concurrent_Mutex_lock(sem->mutex_id);
    sem->count++;
    aura_lang_concurrent_Condvar_signal(sem->cv_id);
    aura_lang_concurrent_Mutex_unlock(sem->mutex_id);
}

int64_t aura_lang_concurrent_Semaphore_count(int64_t sem_id) {
    AuraSemaphore *sem = (AuraSemaphore *)concurrent_get(sem_id);
    if (!sem) return 0;
    aura_lang_concurrent_Mutex_lock(sem->mutex_id);
    int64_t c = sem->count;
    aura_lang_concurrent_Mutex_unlock(sem->mutex_id);
    return c;
}

void aura_lang_concurrent_Semaphore_destroy(int64_t sem_id) {
    AuraSemaphore *sem = (AuraSemaphore *)concurrent_get(sem_id);
    if (sem) {
        aura_lang_concurrent_Mutex_destroy(sem->mutex_id);
        aura_lang_concurrent_Condvar_destroy(sem->cv_id);
        free(sem);
    }
    concurrent_free_slot(sem_id);
}

// ── Future (aura_lang_concurrent_Future_*) ──────────────────
// Future 基于 Thread 实现：spawn 创建线程，await 等待结果。
// 使用全局注册表存储 future 状态。

typedef struct {
    int64_t thread_id;  // aura_thread_create 返回的线程 ID
    int64_t result;
    int done;
    int cancelled;
} AuraFuture;

// aura.lang.concurrent.Future.spawn(fn_id, arg) → Int
// 复用 aura_thread_create 创建线程，通过 thread_dispatch 分派
int64_t aura_lang_concurrent_Future_spawn(int64_t fn_id, int64_t arg) {
    AuraFuture *fut = (AuraFuture *)malloc(sizeof(AuraFuture));
    if (!fut) return -1;
    fut->result = 0;
    fut->done = 0;
    fut->cancelled = 0;
    int64_t id = concurrent_alloc((void *)fut);
    if (id < 0) { free(fut); return -1; }
    // 通过 aura_thread_create 创建线程（fn_id 为函数索引）
    fut->thread_id = aura_thread_create(fn_id, arg);
    if (fut->thread_id < 0) {
        concurrent_free_slot(id);
        free(fut);
        return -1;
    }
    return id;
}

// aura.lang.concurrent.Future.await(future_id) → Int
int64_t aura_lang_concurrent_FutureAwait(int64_t future_id) {
    AuraFuture *fut = (AuraFuture *)concurrent_get(future_id);
    if (!fut) return 0;
    if (fut->thread_id > 0) fut->result = aura_thread_join(fut->thread_id);
    fut->done = 1;
    return fut->result;
}

// aura.lang.concurrent.Future.isDone(future_id) → Boolean
_Bool aura_lang_concurrent_Future_isDone(int64_t future_id) {
    AuraFuture *fut = (AuraFuture *)concurrent_get(future_id);
    if (!fut) return 0;
    return fut->done;
}

// aura.lang.concurrent.Future.cancel(future_id) → Unit
void aura_lang_concurrent_Future_cancel(int64_t future_id) {
    AuraFuture *fut = (AuraFuture *)concurrent_get(future_id);
    if (fut) fut->cancelled = 1;
}

// aura.lang.concurrent.Future.all / Future.any — stubs
void *aura_lang_concurrent_Future_all(void *future_ids) {
    (void)future_ids;
    return NULL;
}
int64_t aura_lang_concurrent_Future_any(void *future_ids) {
    (void)future_ids;
    return 0;
}

// ── Promise (aura_lang_concurrent_Promise_*) ────────────────
// 可外部写入的异步结果占位符。Aura 侧 `aura/lang/concurrent/Promise.aura`
// 是纯 Aura object（基于 Memory.alloc/read64/write64）；VM 走嵌入镜像派发，
// **AOT 没有嵌入镜像可用**，故这里给出等价的 C 实现（与 Future 同一风格）。
// 语义与 Promise.aura 对齐：
//   create()            → 句柄（0 表示失败）
//   resolve(id, v)      → 完成，结果 = v
//   reject(id, code)    → 拒绝，结果 = -code
//   await(id)           → 阻塞到完成，返回结果（拒绝时为负的错误码）
//   isDone(id)          → 完成（resolve 或 reject）为 true
//   isRejected(id)      → 被拒绝为 true
//   tryAwait(id)        → 未完成返回 -1，否则返回结果
typedef struct {
    int64_t state;  // 0 未完成 / 1 已完成 / -1 已拒绝
    int64_t value;
} AuraPromise;

int64_t aura_lang_concurrent_Promise_create(void) {
    AuraPromise *p = (AuraPromise *)malloc(sizeof(AuraPromise));
    if (!p) return 0;
    p->state = 0;
    p->value = 0;
    return (int64_t)(uintptr_t)p;
}

void aura_lang_concurrent_Promise_resolve(int64_t fid, int64_t value) {
    AuraPromise *p = (AuraPromise *)(uintptr_t)fid;
    if (!p) return;
    p->value = value;
    p->state = 1;
}

void aura_lang_concurrent_Promise_reject(int64_t fid, int64_t error_code) {
    AuraPromise *p = (AuraPromise *)(uintptr_t)fid;
    if (!p) return;
    p->value = -error_code;
    p->state = -1;
}

int64_t aura_lang_concurrent_Promise_await(int64_t fid) {
    AuraPromise *p = (AuraPromise *)(uintptr_t)fid;
    if (!p) return 0;
    while (p->state == 0) aura_thread_sleep(1);
    return p->value;
}

_Bool aura_lang_concurrent_Promise_isDone(int64_t fid) {
    AuraPromise *p = (AuraPromise *)(uintptr_t)fid;
    if (!p) return 0;
    return p->state != 0;
}

_Bool aura_lang_concurrent_Promise_isRejected(int64_t fid) {
    AuraPromise *p = (AuraPromise *)(uintptr_t)fid;
    if (!p) return 0;
    return p->state == -1;
}

int64_t aura_lang_concurrent_Promise_tryAwait(int64_t fid) {
    AuraPromise *p = (AuraPromise *)(uintptr_t)fid;
    if (!p) return -1;
    if (p->state == 0) return -1;
    return p->value;
}

// ── Thread (aura_lang_concurrent_Thread_*) ──────────────────
// Thread 函数由 translate_to_legacy_c 映射到已有 C 实现，
// 此处提供 aura_lang_concurrent_Thread_* 别名以兼容直接调用。

int64_t aura_lang_concurrent_Thread_spawn(int64_t fn_id, int64_t arg) {
    return aura_thread_create(fn_id, arg);
}
int64_t aura_lang_concurrent_Thread_join(int64_t id) {
    return aura_thread_join(id);
}
void aura_lang_concurrent_Thread_sleep(int64_t ms) {
    aura_thread_sleep(ms);
}
int64_t aura_lang_concurrent_Thread_id(void) {
    return aura_thread_id();
}
int64_t aura_lang_concurrent_Thread_parallelism(void) {
    return aura_thread_available_parallelism();
}
int64_t aura_lang_concurrent_Thread_availableCores(void) {
    return aura_thread_available_parallelism();
}

/* ──────────────────────────────────────────────────────────────
 * ra0 → C 函数名解析（诊断用）
 *
 * AOT 产物被 strip：`llvm-nm build/hat-native/PhotonHatCompile.exe` 报
 * "no symbols"，`dumpbin` 不存在，`cur.ll` 只有 Aura 侧 IR（无 C 函数体）——
 * 返回地址事后无从解析。但归因代码与全部热点 C 函数在**同一翻译单元**，
 * 所以运行时取函数地址即可：同一进程内地址可直接比较，ASLR 不影响。
 *
 * 表必须定义在文件**末尾**——那里所有函数才都已声明（`(void*)f` 需要原型）。
 * 表在首次打印时按地址排序一次；`ra0` 是「call 指令下一条」，必然落在调用者
 * 函数体内部，故取「地址 <= ra0 的最大项」即调用者。若调用者不在本表
 * （例如 Aura 生成代码或别的 .c），偏移会异常大，用 `?!` 标出。
 * ────────────────────────────────────────────────────────────── */
#define AURA_FN(sym) { (void *)(sym), #sym }
static AuraFnEnt g_fn_table[AURA_FN_COUNT] = {
    AURA_FN(aura_abs),
    AURA_FN(aura_arc_decrement),
    AURA_FN(aura_arc_increment),
    AURA_FN(aura_ch_pop),
    AURA_FN(aura_ch_push),
    AURA_FN(aura_char_cache),
    AURA_FN(aura_clock),
    AURA_FN(aura_collections_arrayListOf),
    AURA_FN(aura_collections_arrayListSize),
    AURA_FN(aura_collections_hashMapGet),
    AURA_FN(aura_collections_hashMapOf),
    AURA_FN(aura_collections_hashMapPut),
    AURA_FN(aura_collections_hashMapRemove),
    AURA_FN(aura_collections_hashSetAdd),
    AURA_FN(aura_collections_hashSetContains),
    AURA_FN(aura_collections_hashSetOf),
    AURA_FN(aura_collections_hashSetRemove),
    AURA_FN(aura_collections_linkedAddFirst),
    AURA_FN(aura_collections_linkedAddLast),
    AURA_FN(aura_collections_linkedHashMapFirstKey),
    AURA_FN(aura_collections_linkedHashMapKeys),
    AURA_FN(aura_collections_linkedHashMapLastKey),
    AURA_FN(aura_collections_linkedHashMapOf),
    AURA_FN(aura_collections_linkedListOf),
    AURA_FN(aura_collections_linkedRemoveFirst),
    AURA_FN(aura_collections_linkedRemoveLast),
    AURA_FN(aura_collections_listContains),
    AURA_FN(aura_collections_listIndexOf),
    AURA_FN(aura_collections_listOf),
    AURA_FN(aura_collections_pairOf),
    AURA_FN(aura_concurrent_actorAlive),
    AURA_FN(aura_concurrent_ask),
    AURA_FN(aura_concurrent_channelRecv),
    AURA_FN(aura_concurrent_channelSend),
    AURA_FN(aura_concurrent_channelTryRecv),
    AURA_FN(aura_concurrent_newChannel),
    AURA_FN(aura_concurrent_select),
    AURA_FN(aura_concurrent_send),
    AURA_FN(aura_concurrent_spawn),
    AURA_FN(aura_concurrent_spawnActor),
    AURA_FN(aura_concurrent_supervise),
    AURA_FN(aura_coroutine_yield),
    AURA_FN(aura_dup_n),
    AURA_FN(aura_dynlist_new),
    AURA_FN(aura_dynlist_push),
    AURA_FN(aura_encoding_base64Decode),
    AURA_FN(aura_encoding_base64Encode),
    AURA_FN(aura_env_get),
    AURA_FN(env_get),
    AURA_FN(aura_env_has),
    AURA_FN(aura_env_platform),
    AURA_FN(aura_free),
    AURA_FN(aura_fs_exists),
    AURA_FN(aura_fs_isDirectory),
    AURA_FN(aura_fs_isFile),
    AURA_FN(aura_fs_mkdirP),
    AURA_FN(aura_fs_readText),
    AURA_FN(aura_fs_writeText),
    AURA_FN(aura_handle_equals),
    AURA_FN(aura_io_fileExists),
    AURA_FN(aura_io_fileRead),
    AURA_FN(aura_io_fileWrite),
    AURA_FN(aura_io_readAll),
    AURA_FN(aura_io_readLine),
    AURA_FN(aura_isOfType),
    AURA_FN(aura_iter_box),
    AURA_FN(aura_iter_fn),
    AURA_FN(aura_iter_unbox),
    AURA_FN(aura_lang_concurrent_Atomic_add),
    AURA_FN(aura_lang_concurrent_Atomic_cas),
    AURA_FN(aura_lang_concurrent_Atomic_destroy),
    AURA_FN(aura_lang_concurrent_Atomic_load),
    AURA_FN(aura_lang_concurrent_Atomic_new),
    AURA_FN(aura_lang_concurrent_Atomic_store),
    AURA_FN(aura_lang_concurrent_Atomic_sub),
    AURA_FN(aura_lang_concurrent_Barrier_destroy),
    AURA_FN(aura_lang_concurrent_Barrier_new),
    AURA_FN(aura_lang_concurrent_Barrier_wait),
    AURA_FN(aura_lang_concurrent_Channel_channelRecv),
    AURA_FN(aura_lang_concurrent_Channel_channelSend),
    AURA_FN(aura_lang_concurrent_Channel_newChannel),
    AURA_FN(aura_lang_concurrent_Condvar_broadcast),
    AURA_FN(aura_lang_concurrent_Condvar_destroy),
    AURA_FN(aura_lang_concurrent_Condvar_new),
    AURA_FN(aura_lang_concurrent_Condvar_signal),
    AURA_FN(aura_lang_concurrent_Condvar_wait),
    AURA_FN(aura_lang_concurrent_Coroutine_actorAlive),
    AURA_FN(aura_lang_concurrent_Future_all),
    AURA_FN(aura_lang_concurrent_Future_any),
    AURA_FN(aura_lang_concurrent_Future_cancel),
    AURA_FN(aura_lang_concurrent_Future_isDone),
    AURA_FN(aura_lang_concurrent_Future_spawn),
    AURA_FN(aura_lang_concurrent_FutureAwait),
    AURA_FN(aura_lang_concurrent_Mutex_destroy),
    AURA_FN(aura_lang_concurrent_Mutex_lock),
    AURA_FN(aura_lang_concurrent_Mutex_new),
    AURA_FN(aura_lang_concurrent_Mutex_tryLock),
    AURA_FN(aura_lang_concurrent_Mutex_unlock),
    AURA_FN(aura_lang_concurrent_RwLock_destroy),
    AURA_FN(aura_lang_concurrent_RwLock_new),
    AURA_FN(aura_lang_concurrent_RwLock_readLock),
    AURA_FN(aura_lang_concurrent_RwLock_readUnlock),
    AURA_FN(aura_lang_concurrent_RwLock_writeLock),
    AURA_FN(aura_lang_concurrent_RwLock_writeUnlock),
    AURA_FN(aura_lang_concurrent_Semaphore_acquire),
    AURA_FN(aura_lang_concurrent_Semaphore_count),
    AURA_FN(aura_lang_concurrent_Semaphore_destroy),
    AURA_FN(aura_lang_concurrent_Semaphore_new),
    AURA_FN(aura_lang_concurrent_Semaphore_release),
    AURA_FN(aura_lang_concurrent_Semaphore_tryAcquire),
    AURA_FN(aura_lang_concurrent_Thread_availableCores),
    AURA_FN(aura_lang_concurrent_Thread_id),
    AURA_FN(aura_lang_concurrent_Thread_join),
    AURA_FN(aura_lang_concurrent_Thread_parallelism),
    AURA_FN(aura_lang_concurrent_Thread_sleep),
    AURA_FN(aura_lang_concurrent_Thread_spawn),
    AURA_FN(aura_lang_std_Ascii_isAlpha),
    AURA_FN(aura_lang_std_Ascii_isDigit),
    AURA_FN(aura_lang_std_Ascii_toLower),
    AURA_FN(aura_lang_std_Ascii_toUpper),
    AURA_FN(aura_lang_std_Builtin_intToPtr),
    AURA_FN(aura_lang_std_Builtin_ptrToInt),
    AURA_FN(aura_lang_std_Collections_contains),
    AURA_FN(aura_lang_std_Collections_count),
    AURA_FN(aura_lang_std_Collections_emptyList),
    AURA_FN(aura_lang_std_Collections_emptyMap),
    AURA_FN(aura_lang_std_Collections_filter),
    AURA_FN(aura_lang_std_Collections_getAt),
    AURA_FN(aura_lang_std_Collections_getOrDefault),
    AURA_FN(aura_lang_std_Collections_hashMapPut),
    AURA_FN(aura_lang_std_Collections_indexOf),
    AURA_FN(aura_lang_std_Collections_isEmpty),
    AURA_FN(aura_lang_std_Collections_listAppend),
    AURA_FN(aura_lang_std_Collections_listContains),
    AURA_FN(aura_lang_std_Collections_listGet),
    AURA_FN(aura_lang_std_Collections_listIndexOf),
    AURA_FN(aura_lang_std_Collections_listOf),
    AURA_FN(aura_lang_std_Collections_listPop),
    AURA_FN(aura_lang_std_Collections_listSet),
    AURA_FN(aura_lang_std_Collections_listSize),
    AURA_FN(aura_lang_std_Collections_map),
    AURA_FN(aura_lang_std_Collections_mapContains),
    AURA_FN(aura_lang_std_Collections_mapContainsKey),
    AURA_FN(aura_lang_std_Collections_mapContainsValue),
    AURA_FN(aura_lang_std_Collections_mapGet),
    AURA_FN(aura_lang_std_Collections_mapKeys),
    AURA_FN(aura_lang_std_Collections_mapSet),
    AURA_FN(aura_lang_std_Collections_mapSize),
    AURA_FN(aura_lang_std_Collections_mapValues),
    AURA_FN(aura_lang_std_Collections_mutableMapOf),
    AURA_FN(aura_lang_std_Collections_pairOf),
    AURA_FN(aura_lang_std_Collections_range),
    AURA_FN(aura_lang_std_Collections_set),
    AURA_FN(aura_lang_std_Collections_take),
    AURA_FN(aura_lang_std_FileSystem_exists),
    AURA_FN(aura_lang_std_FileSystem_mkdirP),
    AURA_FN(aura_lang_std_FileSystem_readText),
    AURA_FN(aura_lang_std_FileSystem_writeText),
    AURA_FN(aura_lang_std_IO_fileExists),
    AURA_FN(aura_lang_std_Process_arg),
    AURA_FN(aura_lang_std_Process_argCount),
    AURA_FN(aura_lang_std_Process_args),
    AURA_FN(aura_lang_std_Process_run),
    AURA_FN(aura_lang_std_String_charAt),
    AURA_FN(aura_lang_std_String_charCodeAt),
    AURA_FN(aura_lang_std_String_contains),
    AURA_FN(aura_lang_std_String_countChar),
    AURA_FN(aura_lang_std_String_endsWith),
    AURA_FN(aura_lang_std_String_equals),
    AURA_FN(aura_lang_std_Console_print),
    AURA_FN(aura_lang_std_Console_println),
    AURA_FN(aura_lang_std_String_indexOf),
    AURA_FN(aura_lang_std_String_lastIndexOf),
    AURA_FN(aura_lang_std_String_length),
    AURA_FN(aura_lang_std_String_padStart),
    AURA_FN(aura_lang_std_String_replace),
    AURA_FN(aura_lang_std_String_replaceAll),
    AURA_FN(aura_lang_std_String_split),
    AURA_FN(aura_lang_std_String_startsWith),
    AURA_FN(aura_lang_std_String_substring),
    AURA_FN(aura_lang_std_String_substringAfter),
    AURA_FN(aura_lang_std_String_substringAt),
    AURA_FN(aura_lang_std_String_substringBefore),
    AURA_FN(aura_lang_std_String_substringFrom),
    AURA_FN(aura_lang_std_String_toFloat),
    AURA_FN(aura_lang_std_String_toInt),
    AURA_FN(aura_lang_std_String_toLowerCase),
    AURA_FN(aura_lang_std_String_toUpperCase),
    AURA_FN(aura_lang_std_String_trim),
    AURA_FN(aura_lang_std_StringBuilder_append),
    AURA_FN(aura_lang_std_StringBuilder_appendChar),
    AURA_FN(aura_lang_std_StringBuilder_appendInt),
    AURA_FN(aura_lang_std_StringBuilder_create),
    AURA_FN(aura_lang_std_StringBuilder_finish),
    AURA_FN(aura_lang_std_StringBuilder_length),
    AURA_FN(aura_lang_std_StringBuilder_reset),
    AURA_FN(aura_malloc),
    AURA_FN(aura_map_new),
    AURA_FN(aura_mem_alloc),
    AURA_FN(aura_mem_alloc_calls),
    AURA_FN(aura_mem_bucket),
    AURA_FN(aura_mem_dump_stats),
    AURA_FN(aura_mem_free),
    AURA_FN(aura_mem_hdr_sane),
    AURA_FN(aura_mem_init),
    AURA_FN(aura_mem_limit_bytes),
    AURA_FN(aura_mem_oom),
    AURA_FN(aura_mem_realloc),
    AURA_FN(aura_mem_set_limit_mb),
    AURA_FN(aura_mem_strdup),
    AURA_FN(aura_scr_alloc),
    AURA_FN(StrdupRetain),
    AURA_FN(aura_string_concat),
    AURA_FN(aura_mem_used_bytes),
    AURA_FN(aura_mem_used_mb),
    AURA_FN(aura_path_basename),
    AURA_FN(aura_path_dirname),
    AURA_FN(aura_path_join),
    AURA_FN(aura_pow),
    AURA_FN(aura_print),
    AURA_FN(aura_println),
    AURA_FN(aura_process_arg),
    AURA_FN(aura_process_argCount),
    AURA_FN(aura_process_args),
    AURA_FN(aura_process_run),
    AURA_FN(aura_puts),
    AURA_FN(aura_random_init),
    AURA_FN(aura_random_nextFloat),
    AURA_FN(aura_random_nextInt),
    AURA_FN(aura_sb_append_n),
    AURA_FN(aura_sb_from_handle),
    AURA_FN(aura_sb_reserve),
    AURA_FN(aura_sqrt),
    AURA_FN(aura_str_to_float),
    AURA_FN(aura_str_to_int),
    AURA_FN(aura_strdup),
    AURA_FN(aura_string_charAt),
    AURA_FN(aura_string_charCodeAt),
    AURA_FN(aura_string_concat),
    AURA_FN(aura_string_contains),
    AURA_FN(aura_string_countChar),
    AURA_FN(aura_string_data),
    AURA_FN(aura_string_endsWith),
    AURA_FN(aura_string_fromCharCode),
    AURA_FN(aura_string_indexOf),
    AURA_FN(aura_string_lastIndexOf),
    AURA_FN(aura_string_length),
    AURA_FN(aura_string_new),
    AURA_FN(aura_string_padStart),
    AURA_FN(aura_string_replace),
    AURA_FN(aura_string_replaceAll),
    AURA_FN(aura_string_split),
    AURA_FN(aura_string_startsWith),
    AURA_FN(aura_string_substring),
    AURA_FN(aura_string_substringAfter),
    AURA_FN(aura_string_substringBefore),
    AURA_FN(aura_string_toLowerCase),
    AURA_FN(aura_string_toUpperCase),
    AURA_FN(aura_string_trim),
    AURA_FN(aura_strlen),
    AURA_FN(aura_substr_dup),
    AURA_FN(aura_time_epoch),
    AURA_FN(aura_time_epochMillis),
    AURA_FN(aura_time_format),
    AURA_FN(aura_to_float),
    AURA_FN(aura_to_int),
    AURA_FN(aura_to_int_any),
    AURA_FN(aura_add_i8),
    AURA_FN(aura_to_str),
    AURA_FN(aura_to_str_any),
    AURA_FN(aura_to_str_bool),
    AURA_FN(aura_to_str_float)
};

static int aura_fn_cmp(const void *x, const void *y) {
    size_t a = (size_t)(((const AuraFnEnt *)x)->a);
    size_t b = (size_t)(((const AuraFnEnt *)y)->a);
    if (a < b) return -1;
    if (a > b) return 1;
    return 0;
}

/* 最近一次成功匹配到的表项下标（<0 = ra0 低于所有函数地址）。 */
static int aura_attr_who_idx(void *ra) {
    static int g_fn_sorted = 0;
    int lo, hi, best = -1, mid;
    if (!g_fn_sorted) {
        qsort(g_fn_table, AURA_FN_COUNT, sizeof(AuraFnEnt), aura_fn_cmp);
        g_fn_sorted = 1;
    }
    lo = 0;
    hi = AURA_FN_COUNT - 1;
    while (lo <= hi) {
        mid = (lo + hi) / 2;
        if ((size_t)g_fn_table[mid].a <= (size_t)ra) {
            best = mid;
            lo = mid + 1;
        } else {
            hi = mid - 1;
        }
    }
    return best;
}

const char *aura_attr_who(void *ra) {
    int i = aura_attr_who_idx(ra);
    return (i < 0) ? "(below-all)" : g_fn_table[i].n;
}

size_t aura_attr_off(void *ra) {
    int i = aura_attr_who_idx(ra);
    if (i < 0) return 0;
    return (size_t)ra - (size_t)g_fn_table[i].a;
}

/* ═════════════════════════════════════════════════════════════════════════
 * 崩溃现场转储（AURA_CRASH_DUMP=1 时安装顶层异常处理器）
 *
 * 背景：自举后端在 Phase E 稳定以 `0xC0000005`（访问违例）终止，但既非 OOM
 * （RSS ~2.5 GB ≪ 8 GiB 闸门）也无 C 侧诊断输出。此前只有「进程退出码」这
 * 一条线索，无法判断是空指针、越界读还是堆破坏。
 *
 * 做法：用 VEH（Vectored Exception Handler）在最外层捕获未处理异常，打印
 *   - `code`            异常码（0xC0000005 / 0xC00000FD 栈溢出 / 0xC0000005 …）
 *   - `addr`            出错的**指令**地址 → 经 `aura_attr_who` 归因到 C 函数
 *   - `info0/info1`     访问类型（0 读 / 1 写）与**被访问的野地址**
 *   - 24 帧调用栈       `CaptureStackBackTrace` + 同一张 fn 表归因
 *
 * 归因表只覆盖 `AURA_FN(...)` 登记的 C 运行时函数；AOT 编译出的 Aura 函数
 * 不在此列，会显示 `(below-all)` 或落到最近的更早函数上——但**只要命中任意
 * 一帧 C 运行时函数**，就能立刻把「是谁在读野指针」定位到函数级。
 *
 * `dbghelp.dll` 通过 `LoadLibrary` 延迟加载，不引入链接期依赖。
 * 默认关闭（无环境变量则完全不安装），对正常路径零开销。
 * ═════════════════════════════════════════════════════════════════════════ */
#ifdef _WIN32

/* AURA_ATTR_DUMP=1：进程退出时把「分配调用点归因表」（ra0 → 次数/字节）
 * 打到 stderr——定位小对象分配风暴的头号工具（ra0 是 AOT 产物内的返回
 * 地址，减去镜像基址后用 llvm-nm obj 符号表即可落到 Aura 函数）。 */
static void aura_attr_dump_atexit(void) {
    aura_attr_dump(stderr, "exit-attr");
    if (aura_sc_enabled()) {
        aura_sc_dump(stderr, "exit-sc");
        fflush(stderr);
    }
    if (getenv("AURA_ATTR_DUMP")) {
        HMODULE exe_base = GetModuleHandleW(NULL);
        uintptr_t base = (uintptr_t)exe_base;
        int k, cnt = 0;
        fprintf(stderr, "exit-ra1 (AOT callsites, rva relative to image base)\n");
        for (k = 0; k < AURA_ATTR_MAX; k++) {
            if (g_attr_ra1[k] != NULL) {
                fprintf(stderr, "  ra1[%d] %p rva=0x%llx\n", k, g_attr_ra1[k],
                        (unsigned long long)((uintptr_t)g_attr_ra1[k] - base));
                cnt++;
            }
        }
        fflush(stderr);
    }
}

static void aura_dump_stack_backtrace(void) {
    typedef ULONG (WINAPI *AuraCaptureFn)(ULONG, ULONG, PVOID *, PULONG);
    /* RtlCaptureStackBackTrace 在 kernel32：dbghelp 并不导出同名函数
     *（实测 GetProcAddress 恒 NULL → 回溯恒缺失）。 */
    HMODULE h = GetModuleHandleW(L"kernel32.dll");
    AuraCaptureFn csb = h ? (AuraCaptureFn)GetProcAddress(h, "RtlCaptureStackBackTrace") : NULL;
    if (!csb) {
        fprintf(stderr, "[aura]   (RtlCaptureStackBackTrace unavailable)\n");
        return;
    }
    PVOID frames[24];
    UINT n = csb(0, 24, frames, NULL);
    /* 同时打印 RVA（相对主模块基址）：AOT Aura 函数不在 C 归因表内，
     * 但 exe 的 .text 布局 = obj 段布局（.text RVA 起点 0x1000），用
     * `llvm-nm --numeric-sort aotrepro.obj` 的 .text 符号表即可把
     * rva-0x1000 双分到确切的 Aura 函数（见 docs/photon 排查手册）。 */
    HMODULE exe_base = GetModuleHandleW(NULL);
    uintptr_t base = (uintptr_t)exe_base;
    for (UINT i = 0; i < n; i++) {
        fprintf(stderr, "  #%-2u %p rva=0x%llx %s+0x%zx\n", i, frames[i],
                (unsigned long long)((uintptr_t)frames[i] - base),
                aura_attr_who(frames[i]), aura_attr_off(frames[i]));
    }
}

static LONG WINAPI aura_crash_handler(EXCEPTION_POINTERS *ep) {
    if (!ep || !ep->ExceptionRecord) return EXCEPTION_CONTINUE_SEARCH;
    EXCEPTION_RECORD *er = ep->ExceptionRecord;
    /* 只处理「致命且无更好归属」的异常：访问违例 / 栈溢出 / 非法指令 /
     * 数据执行保护。别的异常（如除零、用户 throw）交给默认处理。 */
    LONG code = (LONG)er->ExceptionCode;
    if (code != EXCEPTION_ACCESS_VIOLATION && code != EXCEPTION_STACK_OVERFLOW &&
        code != EXCEPTION_ILLEGAL_INSTRUCTION && code != EXCEPTION_DATATYPE_MISALIGNMENT) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    fprintf(stderr,
            "[aura] CRASH code=0x%08lx instr=%p (%s+0x%zx) "
            "access=%llu target=%p\n",
            (unsigned long)code, er->ExceptionAddress,
            aura_attr_who(er->ExceptionAddress), aura_attr_off(er->ExceptionAddress),
            (unsigned long long)(er->NumberParameters > 0
                                     ? er->ExceptionInformation[0] : 0),
            er->NumberParameters > 1 ? (void *)er->ExceptionInformation[1] : NULL);
    /* x64 Windows 调用约定：rcx/rdx/r8/r9 承载前四个参数。崩溃现场寄存器
     * 因此能直接还原「是哪个指针被解引用、下标是多少」——对定位悬垂指针
     * 与越界下标最有用。 */
    if (ep->ContextRecord) {
        CONTEXT *cx = ep->ContextRecord;
        fprintf(stderr,
                "[aura]   rcx=%p rdx=%p r8=%p r9=%p "
                "rax=%p rbx=%p rsi=%p rdi=%p rbp=%p rsp=%p rip=%p\n",
                (void *)cx->Rcx, (void *)cx->Rdx, (void *)cx->R8, (void *)cx->R9,
                (void *)cx->Rax, (void *)cx->Rbx, (void *)cx->Rsi, (void *)cx->Rdi,
                (void *)cx->Rbp, (void *)cx->Rsp, (void *)cx->Rip);
        fprintf(stderr,
                "[aura]   r10=%p r11=%p r12=%p r13=%p r14=%p r15=%p\n",
                (void *)cx->R10, (void *)cx->R11, (void *)cx->R12,
                (void *)cx->R13, (void *)cx->R14, (void *)cx->R15);
    }
    fflush(stderr);
    aura_dump_stack_backtrace();
    fflush(stderr);
    /* 不吞异常：只观测，随后让默认处理（终止进程 + WER）接管。 */
    return EXCEPTION_CONTINUE_SEARCH;
}

__attribute__((constructor)) static void aura_maybe_install_crash_handler(void) {
    if (getenv("AURA_CRASH_DUMP")) {
        AddVectoredExceptionHandler(1, aura_crash_handler);
    }
}

#else /* !_WIN32 */
/* POSIX 侧暂不实现：崩溃诊断目前只在 Windows 自举路径需要。 */
#endif

// ─────────────────────────────────────────────────────────────────────────────
// 线程操作（ThreadOps_*）— 供 AOT 编译的编译器 VM 使用
// ─────────────────────────────────────────────────────────────────────────────

#ifdef _WIN32
#include <windows.h>
#include <process.h>
#define AURA_MAX_THREADS 256
typedef struct {
    DWORD handle;
    volatile long active;
} AuraThreadSlot;
static AuraThreadSlot g_aura_threads[AURA_MAX_THREADS];
static volatile long g_aura_thread_next = 0;

int64_t ThreadOps_currentId(void) {
    return (int64_t)(uintptr_t)GetCurrentThreadId();
}

int64_t ThreadOps_cores(void) {
    SYSTEM_INFO si;
    GetSystemInfo(&si);
    return (int64_t)si.dwNumberOfProcessors;
}

// 线程入口函数（简化版：直接调用 fn_id 对应的函数指针）
typedef int64_t (*AuraThreadFn)(int64_t);
static unsigned int __stdcall aura_thread_entry(void *arg) {
    int64_t *params = (int64_t *)arg;
    AuraThreadFn fn = (AuraThreadFn)(uintptr_t)params[0];
    int64_t arg_val = params[1];
    params[2] = fn(arg_val);
    return 0;
}

int64_t ThreadOps_create(int64_t fn_id, int64_t arg) {
    long idx = g_aura_thread_next;
    if (idx >= AURA_MAX_THREADS) return -1;
    g_aura_thread_next++;
    int64_t params[3] = { fn_id, arg, 0 };
    g_aura_threads[idx].handle = (DWORD)_beginthreadex(NULL, 0,
        aura_thread_entry, params, 0, NULL);
    if (g_aura_threads[idx].handle == 0) return -1;
    return (int64_t)(idx + 1);
}

int64_t ThreadOps_join(int64_t thread_id) {
    long idx = (long)thread_id - 1;
    if (idx < 0 || idx >= AURA_MAX_THREADS) return 0;
    if (g_aura_threads[idx].handle) {
        WaitForSingleObject((HANDLE)g_aura_threads[idx].handle, INFINITE);
        CloseHandle((HANDLE)g_aura_threads[idx].handle);
        g_aura_threads[idx].handle = 0;
    }
    return 0;
}

void ThreadOps_sleepMs(int64_t ms) {
    Sleep((DWORD)ms);
}

#else /* !_WIN32 */
#include <pthread.h>
#include <unistd.h>

int64_t ThreadOps_currentId(void) {
    return (int64_t)(uintptr_t)pthread_self();
}

int64_t ThreadOps_cores(void) {
    return (int64_t)sysconf(_SC_NPROCESSORS_ONLN);
}

int64_t ThreadOps_create(int64_t fn_id, int64_t arg) {
    // Stub: POSIX thread creation not implemented yet
    return -1;
}

int64_t ThreadOps_join(int64_t thread_id) {
    return 0;
}

void ThreadOps_sleepMs(int64_t ms) {
    usleep((useconds_t)(ms * 1000));
}
#endif

// ─────────────────────────────────────────────────────────────────────────────
// I/O 操作（aura_io_readFile / aura_io_writeFile）
// ─────────────────────────────────────────────────────────────────────────────

const char *aura_io_readFile(const char *path) {
    if (!path) return NULL;
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (size < 0) { fclose(f); return NULL; }
    /* [P0/D5] 带头直堆分配（旧为裸 malloc）：文件内容是 String，须可被
     * ARC 管辖；语义不变（每次独立副本）。 */
    char *buf = (char *)aura_mem_alloc_heap((int64_t)size + 1);
    if (!buf) { fclose(f); return NULL; }
    size_t read = fread(buf, 1, size, f);
    fclose(f);
    buf[read] = '\0';
    return buf;
}

void aura_io_writeFile(const char *path, const char *content) {
    if (!path || !content) return;
    FILE *f = fopen(path, "wb");
    if (!f) return;
    size_t len = strlen(content);
    fwrite(content, 1, len, f);
    fclose(f);
}

// ─────────────────────────────────────────────────────────────────────────────
// JIT 执行（JitExec_call*）— 简化 stub，返回 0
// ─────────────────────────────────────────────────────────────────────────────

int64_t JitExec_callI64(void *fn, int64_t arg) {
    (void)fn; (void)arg;
    return 0;
}

void JitExec_call0(void *fn) {
    (void)fn;
}

// ─────────────────────────────────────────────────────────────────────────────
// 闭包调用（__callClosure）— 简化 stub
// 实际实现需要加载闭包函数指针并间接调用
// ─────────────────────────────────────────────────────────────────────────────

int64_t __callClosure(int64_t closure, const char *sig, int64_t arg) {
    (void)sig;
    // closure 是函数指针地址，直接调用
    if (closure == 0) return 0;
    typedef int64_t (*fn_t)(int64_t);
    fn_t fn = (fn_t)(uintptr_t)closure;
    return fn(arg);
}

// ─────────────────────────────────────────────────────────────────────────────
// 集合操作（size）— 简化 stub
// 实际实现需要根据集合类型返回元素数量
// ─────────────────────────────────────────────────────────────────────────────

int32_t size(void *list) {
    (void)list;
    // 简化：返回 0（实际实现需要解析集合结构）
    return 0;
}





