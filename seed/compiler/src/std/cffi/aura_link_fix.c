/* aura_link_fix.c — R92/R93 链接补齐（只提供 runtime/CFFI 缺失的桥接符号）
 *
 * 背景：AOT 发射器对若干原语发射 PascalCase / 裸名外调，而 /NODEFAULTLIB
 * 下没有 libc 兜底。本文件只负责补齐真正缺失的符号。
 *
 * [R93] 区域管理（RegionEnter / RegionReset / RegionScratchTarget /
 * RegionRetainTarget / ScratchMark / ScratchRewind / StrdupRetain 以及
 * g_scr / g_rgn_ret / g_rgn_active / g_rgn_target）**已全部从此文件删除**：
 * aura_std_cffi.c 已完整提供同一批符号。此前两处重复定义在
 * /FORCE:MULTIPLE 下由链接器静默取先出现者（aura_link_fix.obj 在命令行
 * 更靠前），得到一个"状态变量共享、函数实现分裂"的嵌合体——其中
 * ScratchRewind 不回拨 chunk 复用，导致自举编译内存线性泄漏、205s 冲到
 * 20.7GB（2026-10-19 实测）。现在只保留 Memory_* 前缀转发，
 * 保证每个区域符号在整个链接单元里只有一个定义。
 *
 * 编译：clang -c aura_link_fix.c -o aura_link_fix.obj
 */
#include <stdint.h>

/* ═══════════════════════════════════════════════════════════════
 * 两种编译形态（同一份源码，两个 .obj）：
 *   ① 默认（驱动用，AURA_LINK_FIX_EXE 未定义）：只提供 String_* 补齐。
 *      区域符号由 aura_std_cffi.obj 提供 —— 驱动链接里必须**唯一**，
 *      否则 /FORCE:MULTIPLE 会静默取先出现者，状态变量按不同结构体布局
 *      互相踩踏（2026-10-19 自举 20.7GB 泄漏的根源）。
 *   ② -DAURA_LINK_FIX_EXE（产出 exe 用）：额外提供完整的区域实现 +
 *      Memory_* 包装。产出 exe 不链接 aura_std_cffi.obj，运行期
 *      Memory.regionEnter/scratchMark/strdupRetain 全靠本文件。
 *
 * ⚠ ②中的实现必须与 aura_std_cffi.c 的同名语义保持一致（chunk 复用、
 *   target 分流、记账）。两边行为不一致会导致「驱动编译正确、产出编译器
 *   运行错误」的隐蔽分歧。
 * ═══════════════════════════════════════════════════════════════ */

#ifdef AURA_LINK_FIX_EXE

typedef void* LfHandle;
typedef unsigned long long LfSizeT;
extern LfHandle VirtualAlloc(LfHandle lpAddress, LfSizeT dwSize,
                             unsigned long flAllocationType, unsigned long flProtect);
extern int VirtualFree(LfHandle lpAddress, LfSizeT dwSize, unsigned long dwFreeType);

/* /NODEFAULTLIB：不用 libc，自带最小字节操作 */
static void *lf_memcpy(void *dst, const void *src, int64_t n) {
    char *d = (char *)dst;
    const char *s = (const char *)src;
    int64_t i = 0;
    while (i < n) { d[i] = s[i]; i = i + 1; }
    return dst;
}
static int64_t lf_strlen(const char *s) {
    int64_t n = 0;
    if (!s) { return 0; }
    while (s[n]) { n = n + 1; }
    return n;
}

/* 区域 arena：64MB 块 + bump；rewind 只回拨 (cur_idx, off)，块保留复用。 */
typedef struct {
    void **chunks;
    int64_t *sizes;
    int64_t n, cap;
    char *cur;
    int64_t off, cur_cap, used;
    int64_t cur_idx;
} LfRgn;

static LfRgn g_ret;
static LfRgn g_scr;
static int64_t g_active = 0;
static int64_t g_target = 0; /* 0=retain, 1=scratch */

static int lf_rgn_grow(LfRgn *r) {
    int64_t nc = r->cap ? r->cap * 2 : 64;
    void *nch = (void *)VirtualAlloc(0, (LfSizeT)(nc * (int64_t)sizeof(void *)), 0x3000u, 4u);
    int64_t *nsz = (int64_t *)VirtualAlloc(0, (LfSizeT)(nc * (int64_t)sizeof(int64_t)), 0x3000u, 4u);
    if (!nch || !nsz) { return 0; }
    if (r->chunks) { lf_memcpy(nch, r->chunks, r->n * (int64_t)sizeof(void *)); }
    if (r->sizes)  { lf_memcpy(nsz, r->sizes, r->n * (int64_t)sizeof(int64_t)); }
    r->chunks = (void **)nch;
    r->sizes = nsz;
    r->cap = nc;
    return 1;
}

static void *lf_rgn_chunk(LfRgn *r, int64_t need) {
    int64_t cap = (int64_t)64 << 20;
    if (cap < need) { cap = (need + 0xFFFF) & ~0xFFFFLL; }
    void *c = VirtualAlloc(0, (LfSizeT)cap, 0x3000u, 4u);
    if (!c) { return 0; }
    if (r->n >= r->cap && !lf_rgn_grow(r)) { return 0; }
    r->chunks[r->n] = c;
    r->sizes[r->n] = cap;
    r->n += 1;
    r->cur = (char *)c;
    r->off = 0;
    r->cur_cap = cap;
    r->cur_idx = r->n - 1;
    return c;
}

static void *lf_rgn_bump(LfRgn *r, int64_t need) {
    if (need <= 0) { need = 1; }
    if ((need & 15) != 0) { need = (need + 15) & ~15LL; }
    if (r->cur && r->off + need <= r->cur_cap) {
        /* 就地 bump */
    } else if (r->cur && r->cur_idx + 1 < r->n && r->sizes[r->cur_idx + 1] >= need) {
        r->cur_idx += 1;
        r->cur = (char *)r->chunks[r->cur_idx];
        r->off = 0;
        r->cur_cap = r->sizes[r->cur_idx];
    } else {
        if (!lf_rgn_chunk(r, need)) { return 0; }
    }
    char *p = r->cur + r->off;
    r->off += need;
    r->used += need;
    return p;
}

int64_t RegionEnter(void) { g_active = 1; return 0; }
int64_t RegionScratchTarget(void) { g_target = 1; return 0; }
int64_t RegionRetainTarget(void) { g_target = 0; return 0; }
int64_t ScratchReset(void) {
    g_scr.used = 0;
    if (g_scr.n > 0) {
        g_scr.cur = (char *)g_scr.chunks[0];
        g_scr.off = 0;
        g_scr.cur_cap = g_scr.sizes[0];
        g_scr.cur_idx = 0;
    }
    return 0;
}
int64_t ScratchMark(void) {
    if (g_scr.n <= 0 || !g_scr.cur) { return 0; }
    return ((g_scr.cur_idx + 1) << 40) | (g_scr.off & ((1LL << 40) - 1));
}
int64_t ScratchRewind(int64_t mark) {
    int64_t idx = (int64_t)(mark >> 40);
    int64_t off = mark & ((1LL << 40) - 1);
    int64_t new_used = 0;
    int64_t k;
    if (g_scr.n <= 0) { return 0; }
    if (idx <= 0 || idx > g_scr.n) { idx = 1; off = 0; }
    else if (off > g_scr.sizes[idx - 1]) { off = g_scr.sizes[idx - 1]; }
    for (k = 0; k < idx - 1; k++) { new_used += g_scr.sizes[k]; }
    new_used += off;
    if (new_used > g_scr.used) { new_used = g_scr.used; }
    g_scr.used = new_used;
    g_scr.cur_idx = idx - 1;
    g_scr.cur = (char *)g_scr.chunks[idx - 1];
    g_scr.off = off;
    g_scr.cur_cap = g_scr.sizes[idx - 1];
    return 0;
}
const char *StrdupRetain(const char *s) {
    int64_t n = lf_strlen(s);
    char *out = (char *)lf_rgn_bump(&g_ret, n + 1);
    if (!out) { return ""; }
    if (n > 0) { lf_memcpy(out, s, n); }
    out[n] = 0;
    return out;
}
int64_t RegionReset(void) {
    int64_t i;
    for (i = 0; i < g_scr.n; i++) {
        if (g_scr.chunks[i]) { VirtualFree(g_scr.chunks[i], 0, 0x8000u); }
    }
    g_scr.n = 0; g_scr.cur = 0; g_scr.off = 0; g_scr.cur_cap = 0; g_scr.used = 0; g_scr.cur_idx = 0;
    for (i = 0; i < g_ret.n; i++) {
        if (g_ret.chunks[i]) { VirtualFree(g_ret.chunks[i], 0, 0x8000u); }
    }
    g_ret.n = 0; g_ret.cur = 0; g_ret.off = 0; g_ret.cur_cap = 0; g_ret.used = 0; g_ret.cur_idx = 0;
    g_active = 0;
    g_target = 0;
    return 0;
}

/* 发射器按 @native 声明调用 PascalCase 符号 */
int64_t Memory_regionEnter(void)      { return RegionEnter(); }
int64_t Memory_regionReset(void)      { return RegionReset(); }
int64_t Memory_regionScratchTarget(void) { return RegionScratchTarget(); }
int64_t Memory_regionRetainTarget(void)  { return RegionRetainTarget(); }
int64_t Memory_scratchMark(void)      { return ScratchMark(); }
int64_t Memory_scratchRewind(int64_t mark) { return ScratchRewind(mark); }
const char *Memory_strdupRetain(const char *s) { return StrdupRetain(s); }

#endif /* AURA_LINK_FIX_EXE */

/* ── 本文件私有的保留区（供 String_* 补齐返回长期有效字符串） ──
 * 产出 exe 不链接 aura_std_cffi.obj，拿不到 StrdupRetain；这里自带一个
 * 只增不减的 bump 区（1MB 块，VirtualAlloc）。仅 String.trim/split/
 * substringAt 这类低频补齐调用使用，全程有界。不导出任何符号。 */
typedef void* LfHandle;
typedef unsigned long long LfSizeT;
extern LfHandle VirtualAlloc(LfHandle lpAddress, LfSizeT dwSize,
                             unsigned long flAllocationType, unsigned long flProtect);

typedef struct { char *cur; int64_t cap, off; } LfArena;
static LfArena g_lf = {0};

static char *lf_alloc(int64_t n) {
    if (n <= 0) n = 1;
    if ((n & 7) != 0) n = (n + 7) & ~7LL;
    if (!g_lf.cur || g_lf.off + n > g_lf.cap) {
        int64_t cap = (int64_t)1 << 20;
        if (cap < n) cap = (n + 0xFFFF) & ~0xFFFFLL;
        void *blk = VirtualAlloc(NULL, (LfSizeT)cap, 0x3000u /*RESERVE|COMMIT*/, 4u /*READWRITE*/);
        if (!blk) { return NULL; }
        g_lf.cur = (char *)blk;
        g_lf.cap = cap;
        g_lf.off = 0;
    }
    char *p = g_lf.cur + g_lf.off;
    g_lf.off += n;
    return p;
}

static const char *lf_strdup(const char *s) {
    int64_t n = 0;
    if (s) { while (s[n]) { n++; } }
    char *out = lf_alloc(n + 1);
    if (!out) { return ""; }
    if (n > 0) {
        int64_t i;
        for (i = 0; i < n; i++) { out[i] = s[i]; }
    }
    out[n] = 0;
    return out;
}

/* String_substringAt — 从字符串中提取子串（返回新分配的字符串） */
const char *String_substringAt(const char *s, int64_t start, int64_t len) {
    if (!s) return "";
    int64_t sl = 0;
    while (s[sl]) sl++;
    if (start < 0) start = 0;
    if (start >= sl) return "";
    if (len < 0) len = sl - start;
    if (start + len > sl) len = sl - start;
    char buf[1024];
    int64_t n = len;
    if (n > 1023) n = 1023;
    for (int64_t i = 0; i < n; i++) buf[i] = s[start + i];
    buf[n] = 0;
    return lf_strdup(buf);
}

/* String_String_trim — 去除首尾空白 */
static int64_t my_strlen(const char *s) {
    int64_t n = 0;
    while (*s) { s++; n++; }
    return n;
}

const char *String_String_trim(const char *s) {
    if (!s) return "";
    while (*s == ' ' || *s == '\t' || *s == '\r' || *s == '\n') s++;
    if (*s == 0) return "";
    const char *end = s + my_strlen(s) - 1;
    while (end > s && (*end == ' ' || *end == '\t' || *end == '\r' || *end == '\n')) end--;
    int64_t len = end - s + 1;
    char buf[1024];
    if (len > 1023) len = 1023;
    for (int64_t i = 0; i < len; i++) buf[i] = s[i];
    buf[len] = 0;
    return lf_strdup(buf);
}

/* String_countChar — 统计字符出现次数 */
int64_t String_countChar(const char *s, char c) {
    if (!s) return 0;
    int64_t count = 0;
    while (*s) {
        if (*s == c) count++;
        s++;
    }
    return count;
}

/* String_String_endsWith — 判断字符串是否以指定后缀结尾 */
int String_String_endsWith(const char *s, const char *suffix) {
    if (!s || !suffix) return 0;
    int64_t sl = my_strlen(s);
    int64_t ll = my_strlen(suffix);
    if (ll > sl) return 0;
    const char *p = s + sl - ll;
    while (*p && *suffix) {
        if (*p != *suffix) return 0;
        p++; suffix++;
    }
    return *suffix == 0;
}

/* String_String_split — 按分隔符拆分字符串（返回第一个 token） */
const char *String_String_split(const char *s, const char *delim) {
    if (!s) return "";
    if (!delim || !*delim) return lf_strdup(s);
    /* 找到第一个分隔符 */
    const char *p = s;
    int found = 0;
    while (*p) {
        int isDelim = 0;
        const char *d = delim;
        while (*d) {
            if (*p == *d) { isDelim = 1; break; }
            d++;
        }
        if (isDelim) { found = 1; break; }
        p++;
    }
    if (!found) return lf_strdup(s);
    int64_t len = p - s;
    char buf[1024];
    if (len > 1023) len = 1023;
    for (int64_t i = 0; i < len; i++) buf[i] = s[i];
    buf[len] = 0;
    return lf_strdup(buf);
}

/* ═══════════════════════════════════════════════════════════════
 * StringOps_strcmp / alloc / copy 已由 aura_std_cffi.c 提供
 * （不在此重复定义，避免 lld-link duplicate symbol）。
 * ═══════════════════════════════════════════════════════════════ */
