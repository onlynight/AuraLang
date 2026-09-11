//! Runtime 库链接声明
//!
//! 对应 技术方案 §9.6 Runtime 库集成。
//!
//! Aura 的 runtime 库（`aura-runtime`）用 Rust 实现，通过 C ABI 导出：
//! - `aura_arc_increment(ptr)`：原子引用计数 +1
//! - `aura_arc_decrement(ptr)`：原子引用计数 -1，归零时释放
//! - `aura_coroutine_yield(ctx)`：协程挂起
//! - `aura_malloc(size)`：堆分配
//! - `aura_free(ptr)`：堆释放
//! - `aura_string_new(ptr, len)`：创建字符串对象
//! - `aura_string_len(s)`：获取字符串长度
//! - `aura_string_data(s)`：获取字符串数据指针
//!
//! 代码生成时自动注入这些函数的外部声明，使生成的 LLVM IR 可以引用它们。

use crate::codegen::aot::types::TypeMapper;

/// Runtime 函数声明
struct RuntimeFn {
    name: &'static str,
    ret: &'static str,
    params: &'static [(&'static str, &'static str)], // (name, llvm_type)
}

/// 所有 runtime 函数的声明
const RUNTIME_FUNCTIONS: &[RuntimeFn] = &[
    RuntimeFn {
        name: "aura_arc_increment",
        ret: "void",
        params: &[("ptr", "i8*")],
    },
    RuntimeFn {
        name: "aura_arc_decrement",
        ret: "void",
        params: &[("ptr", "i8*")],
    },
    RuntimeFn {
        name: "aura_coroutine_yield",
        ret: "void",
        params: &[("ctx", "i8*")],
    },
    RuntimeFn {
        name: "aura_malloc",
        ret: "i8*",
        params: &[("size", "i64")],
    },
    RuntimeFn {
        name: "aura_free",
        ret: "void",
        params: &[("ptr", "i8*")],
    },
    RuntimeFn {
        name: "aura_string_new",
        ret: "i8*",
        params: &[
            ("data", "i8*"),
            ("len", "i64"),
        ],
    },
    RuntimeFn {
        name: "aura_string_length",
        ret: "i64",
        params: &[("s", "i8*")],
    },
    RuntimeFn {
        name: "toStringFloat",
        ret: "i8*",
        params: &[("x", "double")],
    },
    RuntimeFn {
        name: "aura_string_data",
        ret: "i8*",
        params: &[("s", "i8*")],
    },
    RuntimeFn {
        name: "aura_lang_std_String_equals",
        ret: "i1",
        params: &[
            ("a", "i8*"),
            ("b", "i8*"),
        ],
    },
    // `s.charAt(i)`：emit.rs 会直接发起 `call i8* @aura_lang_std_String_charAt(i8*, i64)`，
    // 但该函数此前未在 runtime 表中登记 → llc 报 `@aura_lang_std_String_charAt` 未定义。
    // C 侧实现见 aura_std_cffi.c 的 aura_lang_std_String_charAt。
    RuntimeFn {
        name: "aura_lang_std_String_charAt",
        ret: "i8*",
        params: &[
            ("s", "i8*"),
            ("idx", "i64"),
        ],
    },
    // ── AOT 直接调用的 C 运行时辅助函数（实现已存在于 aura_std_cffi.c，此前未登记声明）──
    RuntimeFn {
        name: "aura_strlen",
        ret: "i64",
        params: &[("s", "i8*")],
    },
    RuntimeFn {
        name: "aura_to_str",
        ret: "i8*",
        params: &[("x", "i64")],
    },
    RuntimeFn {
        name: "aura_to_str_float",
        ret: "i8*",
        params: &[("x", "double")],
    },
    // `list.pop()`：emit_call 会把 `pop(list)` 重写到该函数，返回被弹出的元素。
    RuntimeFn {
        name: "aura_lang_std_Collections_listPop",
        ret: "i8*",
        params: &[("list", "i8*")],
    },
    // Map<String, Any> 下标读写（AOT 下 Map 为不透明指针）
    RuntimeFn {
        name: "aura_lang_std_Collections_mutableMapOf",
        ret: "i8*",
        params: &[],
    },
    RuntimeFn {
        name: "aura_lang_std_Collections_mapGet",
        ret: "i8*",
        params: &[
            ("map", "i8*"),
            ("key", "i8*"),
        ],
    },
    RuntimeFn {
        name: "aura_lang_std_Collections_mapSet",
        ret: "void",
        params: &[
            ("map", "i8*"),
            ("key", "i8*"),
            ("value", "i8*"),
        ],
    },
    RuntimeFn {
        name: "aura_lang_std_Collections_listSet",
        ret: "void",
        params: &[
            ("list", "i8*"),
            ("idx", "i64"),
            ("value", "i8*"),
        ],
    },
    RuntimeFn {
        name: "aura_string_concat",
        ret: "i8*",
        params: &[
            ("a", "i8*"),
            ("alen", "i64"),
            ("b", "i8*"),
            ("blen", "i64"),
        ],
    },
    // 集合/列表内建（对应 Aura 自举编译器中大量使用的列表/数组操作）。
    // AOT 下 List/Array 用不透明指针 `i8*`（底层 AuraList 结构，元素以 i64 句柄存储），
    // 这些内建由 C 运行时 `aura_std_cffi.c` 中的 aura_lang_std_Collections_* 实现。
    // 调用点（emit_call）会把 `__list_len`/`__size`/`__get`/`__list_push`/`__list_new`/`__range`
    // 重写到下面这些函数，从而复用同一套 C 实现。
    RuntimeFn {
        name: "aura_lang_std_Collections_emptyList",
        ret: "i8*",
        params: &[],
    },
    RuntimeFn {
        name: "aura_lang_std_Collections_count",
        ret: "i64",
        params: &[("list", "i8*")],
    },
    RuntimeFn {
        name: "aura_lang_std_Collections_getAt",
        ret: "i8*",
        params: &[
            ("list", "i8*"),
            ("idx", "i64"),
        ],
    },
    RuntimeFn {
        name: "aura_lang_std_Collections_listAppend",
        ret: "i8*",
        params: &[
            ("list", "i8*"),
            ("value", "i8*"),
        ],
    },
    RuntimeFn {
        name: "aura_lang_std_Collections_range",
        ret: "i8*",
        params: &[
            ("start", "i32"),
            ("end", "i32"),
            ("inclusive", "i32"),
        ],
    },
];

/// 生成所有 runtime 函数的 LLVM 外部声明
pub fn generate_runtime_declarations(_type_mapper: &TypeMapper) -> String {
    let mut s = String::new();
    s.push_str("; ---- Aura Runtime Declarations ----\n");
    for fn_decl in RUNTIME_FUNCTIONS {
        let params_str: Vec<&str> = fn_decl.params.iter().map(|(_, ty)| *ty).collect();
        let params_str = if params_str.is_empty() { String::new() } else { params_str.join(", ") };
        s.push_str(&format!(
            "declare {} @{}({})\n",
            fn_decl.ret, fn_decl.name, params_str
        ));
    }
    s
}

/// 生成运行时内置函数的定义（与声明不同，这些是 LLVM IR 内联实现，无需链接 C 符号）。
///
/// 目前包含：
/// - `Runtime(msg)`：取出 Aura String 结构体的数据指针作为异常值返回，交给
///   `__throw(i8*)` 打印。原先未在 runtime 表中声明，导致 AOT 生成的
///   `call i32 @Runtime(...)` 引用未定义符号。
pub fn runtime_definitions() -> String {
    let mut s = String::new();
    s.push_str("; ---- Aura Runtime Definitions ----\n");
    s.push_str("define i8* @Runtime(i8* %arg.msg) {\n");
    s.push_str("  ret i8* %arg.msg\n");
    s.push_str("}\n");
    s
}

/// 获取所有 runtime 函数名（供内联 / 消引用使用）
pub fn runtime_function_names() -> Vec<&'static str> {
    RUNTIME_FUNCTIONS.iter().map(|f| f.name).collect()
}

/// 检查一个函数名是否为 runtime 函数
pub fn is_runtime_function(name: &str) -> bool {
    RUNTIME_FUNCTIONS.iter().any(|f| f.name == name)
}

/// 获取 runtime 函数签名（返回类型 + 参数类型列表），供原生函数签名覆盖使用
pub fn runtime_signature(name: &str) -> Option<(&'static str, Vec<&'static str>)> {
    RUNTIME_FUNCTIONS
        .iter()
        .find(|f| f.name == name)
        .map(|f| (f.ret, f.params.iter().map(|(_, ty)| *ty).collect()))
}

/// 将新命名下的 sanitizellvm 结果翻译回旧 C FFI 符号名
///
/// 例如：`aura_lang_std_Math_sin` → `aura_math_sin`
///       `aura_lang_std_IO_readLine` → `aura_io_readLine`
///       `aura_lang_std_Filesystem_exists` → `aura_fs_exists`
///       `aura_lang_std_Network_tcpConnect` → `aura_net_tcpConnect`
///
/// 若输入不是新命名形式，返回原值（用于兼容旧命名下的 C 符号）。
fn translate_to_legacy_c(name: &str) -> String {
    let prefix = "aura_lang_std_";
    if !name.starts_with(prefix) {
        return name.to_string();
    }
    let rest = &name[prefix.len()..];
    let parts: Vec<&str> = rest.splitn(2, '_').collect();
    if parts.len() != 2 {
        return name.to_string();
    }
    let (class, fn_name) = (parts[0], parts[1]);
    // 类名到 C 前缀的映射（与旧命名一致）
    let c_prefix = match class {
        "Math" => "math",
        "IO" => "io",
        "Ascii" => "ascii",
        "Assert" => "assert",
        "Builtin" => "builtin",
        "Collections" => "collections",
        "Console" => "console",
        "Encoding" => "encoding",
        "Env" => "env",
        "FileSystem" => "fs",
        "Iter" => "iter",
        "Json" => "json",
        "Network" => "net",
        "Path" => "path",
        "Process" => "process",
        "Random" => "random",
        "String" => "string",
        "Test" => "test",
        "Time" => "time",
        "Coroutine" => "concurrent",
        "Actor" => "concurrent",
        "Channel" => "concurrent",
        _ => return name.to_string(),
    };
    format!("aura_{}_{}", c_prefix, fn_name)
}

/// aura_std_cffi.c 中实现的 C FFI 函数签名覆盖
/// （HIR 侧原生函数的 Any 类型在 AOT 退化为 i8*，这里用真实 C ABI 类型覆盖，
///  保证调用点、declare 与 C 实现（如 aura_math_sin(double)）三者一致）
pub fn cffi_signature(name: &str) -> Option<(&'static str, Vec<&'static str>)> {
    // 将新命名（sanitizellvm 后的 aura_lang_std_Class_fn）翻译回旧 C 符号名（aura_class_fn）
    let translated = translate_to_legacy_c(name);
    let name = if translated != name { &translated } else { name };

    const D: &str = "double";
    const P: &str = "i8*";
    let (ret, params): (&'static str, &[&str]) = match name {
        "aura_math_sin" | "aura_math_cos" | "aura_math_tan" | "aura_math_asin"
        | "aura_math_acos" | "aura_math_atan" | "aura_math_atan2" | "aura_math_log"
        | "aura_math_log2" | "aura_math_log10" | "aura_math_exp" | "aura_math_sqrt"
        | "aura_math_cbrt" | "aura_math_round" | "aura_math_trunc" | "aura_math_sign" => (D, &[D]),
        "aura_math_pow" | "aura_math_min" | "aura_math_max" => (D, &[D, D]),
        "aura_math_clamp" => (D, &[D, D, D]),
        "aura_math_ceil" | "aura_math_floor" | "aura_math_abs" => ("i64", &[D]),
        // aura.string
        "aura_string_length" => ("i64", &[P]),
        "aura_string_contains" => ("i1", &[P, P]),
        "aura_string_toUpperCase" | "aura_string_toLowerCase" | "aura_string_trim" => (P, &[P]),
        "aura_string_substring" => (
            P,
            &[
                P, "i64", "i64",
            ],
        ),
        "aura_string_charAt" => (P, &[P, "i64"]),
        "aura_string_replace" | "aura_string_replaceAll" => (P, &[P, P, P]),
        "aura_string_padStart" => (
            P,
            &[
                P, "i64", P,
            ],
        ),
        "aura_string_indexOf" | "aura_string_lastIndexOf" | "aura_string_countChar" => {
            ("i64", &[P, P])
        }
        "aura_string_substringBefore" | "aura_string_substringAfter" | "aura_string_split" => {
            (P, &[P, P])
        }
        "aura_string_startsWith" | "aura_string_endsWith" => ("i1", &[P, P]),
        // aura.ascii（Char → i16）
        "aura_ascii_isAlpha"
        | "aura_ascii_isDigit"
        | "aura_ascii_isAlphaNumeric"
        | "aura_ascii_isWhitespace"
        | "aura_ascii_isUpper"
        | "aura_ascii_isLower" => ("i1", &["i16"]),
        "aura_ascii_toUpper" | "aura_ascii_toLower" | "aura_ascii_codeAt" => ("i64", &["i16"]),
        // aura.collections（快照用 3 参 listOf；元素为 Any→i8*）
        "aura_collections_listOf" => (P, &[P, P, P]),
        "aura_collections_listContains" => ("i1", &[P, P]),
        "aura_collections_listIndexOf" => ("i64", &[P, P]),
        // aura.collections — AOT 动态列表（调用点符号：sanitize(aura.lang.std.Collections.*)）
        "aura_collections_emptyList" => (P, &[]),
        "aura_collections_count" | "aura_collections_listSize" => ("i64", &[P]),
        "aura_collections_isEmpty" => ("i1", &[P]),
        "aura_collections_getAt" | "aura_collections_listGet" => (P, &[P, "i64"]),
        "aura_collections_listAppend" => (P, &[P, P]),
        "aura_collections_indexOf" => ("i64", &[P, P]),
        "aura_collections_contains" => ("i1", &[P, P]),
        "aura_collections_set" => (
            P,
            &[
                P, "i64", P,
            ],
        ),
        // Map<String, Any>
        "aura_collections_mutableMapOf" | "aura_collections_emptyMap" => (P, &[]),
        "aura_collections_mapGet" => (P, &[P, P]),
        "aura_collections_mapSet" => ("void", &[P, P, P]),
        "aura_collections_mapSize" => ("i64", &[P]),
        "aura_collections_mapContains" => ("i1", &[P, P]),
        "aura_collections_listSet" => (
            "void",
            &[
                P, "i64", P,
            ],
        ),
        // aura.collections — 特化集合
        "aura_collections_arrayListOf" => (
            P,
            &[
                P, P, P, P, P, P, P, P, P, P,
            ],
        ),
        "aura_collections_arrayListSize" => ("i64", &[P]),
        "aura_collections_linkedListOf" => (
            P,
            &[
                P, P, P, P, P, P, P, P, P, P,
            ],
        ),
        "aura_collections_linkedAddFirst" => (P, &[P, P]),
        "aura_collections_linkedAddLast" => (P, &[P, P]),
        "aura_collections_linkedRemoveFirst" => (P, &[P]),
        "aura_collections_linkedRemoveLast" => (P, &[P]),
        "aura_collections_hashSetOf" => (
            P,
            &[
                P, P, P, P, P, P, P, P, P, P,
            ],
        ),
        "aura_collections_hashSetContains" => ("i1", &[P, P]),
        "aura_collections_hashSetAdd" => (P, &[P, P]),
        "aura_collections_hashSetRemove" => ("i1", &[P, P]),
        "aura_collections_hashMapOf" => (
            P,
            &[
                P, P, P, P, P, P, P, P, P, P,
            ],
        ),
        "aura_collections_hashMapGet" => (P, &[P, P]),
        "aura_collections_hashMapPut" => (P, &[P, P, P]),
        "aura_collections_hashMapRemove" => (P, &[P, P]),
        "aura_collections_linkedHashMapOf" => (
            P,
            &[
                P, P, P, P, P, P, P, P, P, P,
            ],
        ),
        "aura_collections_linkedHashMapKeys" => (P, &[P]),
        "aura_collections_linkedHashMapFirstKey" => (P, &[P]),
        "aura_collections_linkedHashMapLastKey" => (P, &[P]),
        // aura.time / random
        "aura_time_epoch" | "aura_time_epochMillis" | "aura_random_nextInt" => ("i64", &[]),
        // aura.encoding
        "aura_encoding_base64Encode"
        | "aura_encoding_base64Decode"
        | "aura_encoding_hexEncode"
        | "aura_encoding_hexDecode"
        | "aura_encoding_urlEncode"
        | "aura_encoding_urlDecode" => (P, &[P]),
        // aura.path
        "aura_path_join" => (P, &[P, P]),
        "aura_path_basename"
        | "aura_path_dirname"
        | "aura_path_extname"
        | "aura_path_normalize"
        | "aura_path_resolve" => (P, &[P]),
        "aura_path_isAbsolute" | "aura_path_isRelative" => ("i1", &[P]),
        // aura.env
        "aura_env_platform" | "aura_env_os" | "aura_env_arch" | "aura_env_home"
        | "aura_env_tmp" | "aura_env_pwd" => (P, &[]),
        "aura_env_get" => (P, &[P]),
        "aura_env_has" => ("i1", &[P]),
        // aura.fs
        "aura_fs_exists" | "aura_fs_isFile" | "aura_fs_isDirectory" => ("i1", &[P]),
        "aura_fs_readText" => (P, &[P]),
        "aura_fs_writeText" => (P, &[P, P]),
        "aura_fs_mkdirP" => ("i64", &[P]),
        // aura.process
        "aura_process_run" => ("i64", &[P]),
        // aura.io
        "aura_io_fileExists" => ("i1", &[P]),
        "aura_io_fileWrite" => ("void", &[P, P]),
        _ => return None,
    };
    Some((ret, params.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_declarations() {
        let tm = TypeMapper::new(true);
        let decls = generate_runtime_declarations(&tm);
        assert!(decls.contains("aura_arc_increment"));
        assert!(decls.contains("aura_arc_decrement"));
        assert!(decls.contains("aura_coroutine_yield"));
        assert!(decls.contains("aura_malloc"));
        assert!(decls.contains("aura_free"));
        assert!(decls.contains("aura_string_new"));
    }

    #[test]
    fn test_is_runtime() {
        assert!(is_runtime_function("aura_arc_increment"));
        assert!(is_runtime_function("aura_malloc"));
        assert!(!is_runtime_function("main"));
        assert!(!is_runtime_function("add"));
    }

    #[test]
    fn test_runtime_names() {
        let names = runtime_function_names();
        assert_eq!(names.len(), RUNTIME_FUNCTIONS.len());
        assert!(names.contains(&"aura_arc_increment"));
    }
}
