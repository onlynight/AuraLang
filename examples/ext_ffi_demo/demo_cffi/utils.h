/*
 * utils.h — C 头文件（由 aura export-header 生成）
 *
 * 此文件是 Aura AOT + --cabi --shared 产物的 C ABI 接口定义。
 * 导出符号前缀为 aura_c_，调用约定为 C ABI (ccc)。
 *
 * 生成命令：
 *   aura export-header libs/utils/src/lib.aura --out demo_cffi/utils.h
 *
 * 对应 Aura 源文件：libs/utils/src/lib.aura
 * 对应构建命令：loom build --member utils --aot --shared --cabi
 * 产物：utils.dll / utils.so / utils.dylib
 */

#ifndef UTILS_H
#define UTILS_H

#ifdef __cplusplus
extern "C" {
#endif

/* 两数相加 */
int32_t aura_c_add(int32_t a, int32_t b);

/* 两数相乘 */
int32_t aura_c_multiply(int32_t a, int32_t b);

/* 阶乘 */
int32_t aura_c_factorial(int32_t n);

/* 幂运算 */
int32_t aura_c_power(int32_t base, int32_t exp);

#ifdef __cplusplus
}
#endif

#endif /* UTILS_H */
