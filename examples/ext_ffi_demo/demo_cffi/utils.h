/*
* lib.h — C 头文件（由 `aura export-header` 生成）
*
* 对应 Aura AOT + --cabi --shared 产物的 C ABI 接口。
* 导出符号前缀为 aura_c_，调用约定为 C ABI (ccc)。
*
* 源文件：D:/Code/AuraLang/examples/ext_ffi_demo/libs/utils/src/lib.aura
* 构建：aura build D:/Code/AuraLang/examples/ext_ffi_demo/libs/utils/src/lib.aura --aot --shared --cabi --output <lib>.dll
*/

#ifndef LIB_H
#define LIB_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* add */
int32_t aura_c_add(int32_t a, int32_t b);

/* multiply */
int32_t aura_c_multiply(int32_t a, int32_t b);

/* factorial */
int32_t aura_c_factorial(int32_t n);

/* power */
int32_t aura_c_power(int32_t base, int32_t exp);

#ifdef __cplusplus
}
#endif

#endif /* LIB_H */
