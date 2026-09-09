/**
 * Aura std C FFI — 标准库 C ABI 导出
 *
 * 供 AOT 编译后端链接使用。每个函数对应 Aura 的 std 函数。
 *
 * 使用方式：
 *   #include "aura_std_cffi.h"
 *   aura_println("hello");
 *
 * 编译：
 *   clang -c aura_std_cffi.c -o aura_std_cffi.o
 *   clang main.o aura_std_cffi.o -o main
 */

#ifndef AURA_STD_CFFI_H
#define AURA_STD_CFFI_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// ─────────────────────────────────────────────────────────────────────────────
// AuraString 结构体（AOT 字符串表示：{ data, len }）
// ─────────────────────────────────────────────────────────────────────────────

/** AOT 字符串结构体：与 LLVM IR 的 { i8*, i64 } 对应 */
typedef struct {
    const char *data;
    int64_t len;
} AuraString;

// ─────────────────────────────────────────────────────────────────────────────
// Prelude（17 个全局内置）
// ─────────────────────────────────────────────────────────────────────────────

/** 输出字符串（带换行） */
void aura_println(const char *s);

/** 输出字符串（不带换行） */
void aura_print(const char *s);

/** C 风格输出（支持格式化） */
void aura_puts(const char *s);

/** 绝对值 */
int64_t aura_abs(int64_t x);

/** 平方根 */
double aura_sqrt(double x);

/** 幂运算 */
double aura_pow(double base, double exp);

/** 转整数（截断） */
int64_t aura_to_int(double x);

/** 转浮点 */
double aura_to_float(int64_t x);

/** 转字符串（返回 C 风格字符串） */
const char *aura_to_str(int64_t x);

/** 转字符串（浮点数，返回 C 风格字符串） */
const char *aura_to_str_float(double x);

/** 时钟（微秒） */
double aura_clock(void);

/** 字符串长度 */
int64_t aura_strlen(const char *s);

// ─────────────────────────────────────────────────────────────────────────────
// aura.io — 标准输入输出
// ─────────────────────────────────────────────────────────────────────────────

/** 读取一行 */
const char *aura_io_readLine(void);

/** 检查文件是否存在 */
int aura_io_fileExists(const char *path);

/** 读取文件内容 */
const char *aura_io_fileRead(const char *path);

/** 写入文件 */
void aura_io_fileWrite(const char *path, const char *content);

// ─────────────────────────────────────────────────────────────────────────────
// aura.math — 数学函数
// ─────────────────────────────────────────────────────────────────────────────

double aura_math_sin(double x);
double aura_math_cos(double x);
double aura_math_tan(double x);
double aura_math_asin(double x);
double aura_math_acos(double x);
double aura_math_atan(double x);
double aura_math_log(double x);
double aura_math_exp(double x);

double aura_math_min(double a, double b);
double aura_math_max(double a, double b);
int64_t aura_math_ceil(double x);
int64_t aura_math_floor(double x);

// 常量
extern const double aura_math_PI;
extern const double aura_math_E;

// ─────────────────────────────────────────────────────────────────────────────
// aura.string — 字符串操作
// ─────────────────────────────────────────────────────────────────────────────

/** 检查是否包含子串 */
int aura_string_contains(const char *s, const char *sub);

/** 获取字符串长度 */
int64_t aura_string_length(const char *s);

/** 获取字符码 */
int64_t aura_string_charCodeAt(const char *s, int64_t idx);

/** 获取字符 */
const char *aura_string_charAt(const char *s, int64_t idx);

/** 子串 */
const char *aura_string_substring(const char *s, int64_t start, int64_t end);

/** 转大写 */
const char *aura_string_toUpperCase(const char *s);

/** 转小写 */
const char *aura_string_toLowerCase(const char *s);

/** 去除空白 */
const char *aura_string_trim(const char *s);

/** 检查前缀 */
int aura_string_startsWith(const char *s, const char *prefix);

/** 检查后缀 */
int aura_string_endsWith(const char *s, const char *suffix);

/** 替换 */
const char *aura_string_replace(const char *s, const char *from, const char *to);

// ─────────────────────────────────────────────────────────────────────────────
// AOT 字符串操作（{ i8*, i64 } 结构体表示）
// ─────────────────────────────────────────────────────────────────────────────

/** 字符串拼接（返回 C 风格字符串） */
const char *aura_string_concat(const char *a, int64_t alen, const char *b, int64_t blen);

/** 字符串转 C 字符串（返回 data 指针） */
const char *aura_string_data(AuraString s);

// ─────────────────────────────────────────────────────────────────────────────
// aura.time — 时间
// ─────────────────────────────────────────────────────────────────────────────

/** 当前 Unix 时间戳（秒） */
int64_t aura_time_epoch(void);

/** 当前 Unix 时间戳（毫秒） */
int64_t aura_time_epochMillis(void);

/** 格式化为字符串 */
const char *aura_time_format(int64_t timestamp);

// ─────────────────────────────────────────────────────────────────────────────
// aura.random — 随机数
// ─────────────────────────────────────────────────────────────────────────────

/** 随机整数 */
int64_t aura_random_nextInt(void);

/** 随机浮点 [0, 1) */
double aura_random_nextFloat(void);

// ─────────────────────────────────────────────────────────────────────────────
// aura.collections — 特化集合（ArrayList / LinkedList / HashSet / HashMap / LinkedHashMap）
// ─────────────────────────────────────────────────────────────────────────────

// ArrayList
const void *aura_collections_arrayListOf(const void *a, const void *b, const void *c,
    const void *d, const void *e, const void *f, const void *g,
    const void *h, const void *i, const void *j);
int64_t aura_collections_arrayListSize(const void *list);

// LinkedList
const void *aura_collections_linkedListOf(const void *a, const void *b, const void *c,
    const void *d, const void *e, const void *f, const void *g,
    const void *h, const void *i, const void *j);
const void *aura_collections_linkedAddFirst(const void *list, const void *value);
const void *aura_collections_linkedAddLast(const void *list, const void *value);
const void *aura_collections_linkedRemoveFirst(const void *list);
const void *aura_collections_linkedRemoveLast(const void *list);

// HashSet
const void *aura_collections_hashSetOf(const void *a, const void *b, const void *c,
    const void *d, const void *e, const void *f, const void *g,
    const void *h, const void *i, const void *j);
_Bool aura_collections_hashSetContains(const void *set, const void *item);
const void *aura_collections_hashSetAdd(const void *set, const void *item);
_Bool aura_collections_hashSetRemove(const void *set, const void *item);

// HashMap
const void *aura_collections_hashMapOf(const void *k0, const void *v0,
    const void *k1, const void *v1, const void *k2, const void *v2,
    const void *k3, const void *v3, const void *k4, const void *v4);
const void *aura_collections_hashMapGet(const void *map, const void *key);
const void *aura_collections_hashMapPut(const void *map, const void *key, const void *value);
const void *aura_collections_hashMapRemove(const void *map, const void *key);

// LinkedHashMap
const void *aura_collections_linkedHashMapOf(const void *k0, const void *v0,
    const void *k1, const void *v1, const void *k2, const void *v2,
    const void *k3, const void *v3, const void *k4, const void *v4);
const void *aura_collections_linkedHashMapKeys(const void *map);
const void *aura_collections_linkedHashMapFirstKey(const void *map);
const void *aura_collections_linkedHashMapLastKey(const void *map);

// ─────────────────────────────────────────────────────────────────────────────
// Runtime 运行时函数（ARC / 内存 / 协程 / 字符串）
// ─────────────────────────────────────────────────────────────────────────────

/** ARC 引用计数 +1（原子操作） */
void aura_arc_increment(const void *ptr);

/** ARC 引用计数 -1，归零时释放（原子操作） */
void aura_arc_decrement(const void *ptr);

/** 协程挂起（AOT 下为 no-op，VM 运行时处理） */
void aura_coroutine_yield(const void *ctx);

/** 堆分配（返回指针） */
void *aura_malloc(int64_t size);

/** 堆释放 */
void aura_free(const void *ptr);

/** 创建字符串对象（返回字符串指针） */
const char *aura_string_new(const char *data, int64_t len);

/** 获取字符串长度 */
int64_t aura_string_length(const AuraString *s);

/** 获取字符串数据指针 */
const char *aura_string_data(const AuraString *s);

// ─────────────────────────────────────────────────────────────────────────────
// 短名称包装函数（供 AOT IR 直接调用）
// ─────────────────────────────────────────────────────────────────────────────

void println(const char *s);
void print(const char *s);
int64_t aura_abs_wrapper(int64_t x);
double aura_sqrt_wrapper(double x);
double aura_pow_wrapper(double b, double e);
int64_t toInt(double x);
double toFloat(int64_t x);
const char *toString(int64_t x);
double aura_clock_wrapper(void);
int64_t aura_strlen_wrapper(const char *s);
const char *toStringFloat(double x);

/** 类型检查：isOfType(value, typeName) → _Bool */
_Bool aura_isOfType(const void *value, const AuraString *typeName);

/** throw 表达式（AOT）：打印异常值到 stderr */
void __throw(const void *value);

#ifdef __cplusplus
}
#endif

#endif /* AURA_STD_CFFI_H */
