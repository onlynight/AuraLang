/**
 * Aura Collections C FFI — 集合列表操作 C ABI 实现
 *
 * 供 AOT 编译后端链接使用。仅包含 Collections 列表操作函数，
 * 避免与 Aura IR 中已定义的函数（println/toStr 等）产生重复符号。
 *
 * 编译：
 *   clang -c aura_collections_cffi.c -o aura_collections_cffi.o
 */

#ifdef _WIN32
#ifndef _CRT_SECURE_NO_WARNINGS
#define _CRT_SECURE_NO_WARNINGS
#endif
#endif

#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <string.h>

// ── 内存分配（使用 malloc/realloc，不依赖 aura_mem_alloc）──

static void *aura_mem_alloc(int64_t size) {
    return malloc((size_t)size);
}

static void *aura_mem_realloc(void *ptr, int64_t size) {
    return realloc(ptr, (size_t)size);
}

// ── 极简动态列表 ──

typedef struct {
    int64_t len;
    int64_t cap;
    const char **items;
} AuraDynList;

static AuraDynList *aura_dynlist_new(int64_t cap) {
    AuraDynList *l = (AuraDynList *)aura_mem_alloc((int64_t)sizeof(AuraDynList));
    if (!l) return NULL;
    if (cap < 4) cap = 4;
    l->len = 0;
    l->cap = cap;
    l->items = (const char **)aura_mem_alloc((int64_t)sizeof(const char *) * cap);
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
    l->items[l->len++] = s ? s : "";
}

// ── 迭代器链辅助 ──

typedef int64_t (*AuraIterFn)(void *env, int64_t x);

static AuraIterFn aura_iter_fn(void *clo) {
    if (!clo) return NULL;
    return (AuraIterFn)(*(void **)clo);
}

static int64_t aura_iter_unbox(int64_t v) {
    if ((v & 1) != 0) return v >> 1;
    return v;
}

static const char *aura_iter_box(int64_t v) {
    return (const char *)(intptr_t)((((uint64_t)v) << 1) | 1ULL);
}

// ── 集合元素句柄相等判定 ──

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

// ═══════════════════════════════════════════════════════════════════
// aura.lang.std.Collections.*
// ═══════════════════════════════════════════════════════════════════

const void *aura_lang_std_Collections_emptyList(void) {
    return (const void *)aura_dynlist_new(4);
}

const void *aura_lang_std_Collections_listAppend(const void *list, const void *value) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l) {
        l = aura_dynlist_new(4);
    }
    aura_dynlist_push(l, (const char *)value);
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

const void *aura_lang_std_Collections_listRemove(const void *list, int64_t idx) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l || idx < 0 || idx >= l->len) return (const void *)l;
    // 左移元素
    for (int64_t i = idx; i < l->len - 1; i++) {
        l->items[i] = l->items[i + 1];
    }
    l->len--;
    return (const void *)l;
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

int64_t aura_lang_std_Collections_listIndexOf(const void *list, const void *val) {
    return aura_lang_std_Collections_indexOf(list, val);
}

int aura_lang_std_Collections_contains(const void *list, const void *value) {
    return aura_lang_std_Collections_indexOf(list, value) >= 0 ? 1 : 0;
}

_Bool aura_lang_std_Collections_listContains(const void *list, const void *val) {
    return aura_lang_std_Collections_contains(list, val);
}

const void *aura_lang_std_Collections_filter(const void *list, const void *clo) {
    AuraDynList *out = aura_dynlist_new(4);
    const AuraDynList *l = (const AuraDynList *)list;
    AuraIterFn fn = aura_iter_fn((void *)clo);
    if (!l || !fn) return (const void *)out;
    int64_t i = 0;
    while (i < l->len) {
        int64_t arg = aura_iter_unbox((int64_t)(intptr_t)l->items[i]);
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

const void *aura_lang_std_Collections_listPop(const void *list) {
    AuraDynList *l = (AuraDynList *)list;
    if (!l || l->len <= 0) return (const void *)0;
    l->len--;
    return (const void *)l->items[l->len];
}

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

const void *aura_lang_std_Collections_pairOf(const void *a, const void *b) {
    AuraDynList *l = aura_dynlist_new(4);
    aura_dynlist_push(l, (const char *)a);
    aura_dynlist_push(l, (const char *)b);
    return (const void *)l;
}

const void *aura_lang_std_Collections_listOf(const void *a, const void *b, const void *c) {
    AuraDynList *l = aura_dynlist_new(4);
    aura_dynlist_push(l, (const char *)a);
    aura_dynlist_push(l, (const char *)b);
    aura_dynlist_push(l, (const char *)c);
    return (const void *)l;
}
