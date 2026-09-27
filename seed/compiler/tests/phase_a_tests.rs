//! Phase A: Rust AOT 后端加固 — 单元测试
//!
//! 运行：cargo test -p compiler --features llvm --test phase_a_tests
//!
//! 对应改造方案 A.1（类按引用）、A.2（String 统一 i8*）、A.3（返回类型尊重）、
//! A.2-follow-up（String/List 类型判定，移除变量名启发式）

use compiler::codegen::aot::types::TypeMapper;
use compiler::codegen::hir::HirType;

// ────────────────────────────────────────────────────────────────────────────
// A.2: String 表示统一为 i8*（string_as_struct=false）
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_string_maps_to_i8_ptr() {
    let mapper = TypeMapper::new(false);
    let ty = HirType::Named("String".to_string());
    assert_eq!(mapper.map(&ty), "i8*");
}

#[test]
fn test_string_maps_to_struct_when_enabled() {
    // 旧模式：String 映射为 { i8*, i64 }
    let mapper = TypeMapper::new(true);
    let ty = HirType::Named("String".to_string());
    assert_eq!(mapper.map(&ty), "{ i8*, i64 }");
}

// ────────────────────────────────────────────────────────────────────────────
// A.1: 类类型按引用传递（%struct.X*）
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_class_maps_to_struct_ptr() {
    let mapper = TypeMapper::new(false);
    let ty = HirType::Named("MyClass".to_string());
    assert_eq!(mapper.map(&ty), "%struct.MyClass*");
}

#[test]
fn test_class_with_special_chars_maps_correctly() {
    let mapper = TypeMapper::new(false);
    let ty = HirType::Named("My_Class".to_string());
    // sanitizellvm should handle underscores
    assert_eq!(mapper.map(&ty).starts_with("%struct."), true);
}

#[test]
fn test_multiple_class_types_all_ptrs() {
    let mapper = TypeMapper::new(false);
    let ty1 = HirType::Named("Alpha".to_string());
    let ty2 = HirType::Named("Beta".to_string());
    assert_eq!(mapper.map(&ty1), "%struct.Alpha*");
    assert_eq!(mapper.map(&ty2), "%struct.Beta*");
}

// ────────────────────────────────────────────────────────────────────────────
// A.2: 基本类型映射
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_int_types_map_correctly() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("Int".to_string())), "i32");
    assert_eq!(mapper.map(&HirType::Named("Long".to_string())), "i64");
    assert_eq!(mapper.map(&HirType::Named("Short".to_string())), "i16");
    assert_eq!(mapper.map(&HirType::Named("Byte".to_string())), "i8");
    assert_eq!(mapper.map(&HirType::Named("U8".to_string())), "i8");
}

#[test]
fn test_float_types_map_correctly() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("Float".to_string())), "float");
    assert_eq!(mapper.map(&HirType::Named("Double".to_string())), "double");
}

#[test]
fn test_bool_maps_to_i1() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("Boolean".to_string())), "i1");
    assert_eq!(mapper.map(&HirType::Named("Bool".to_string())), "i1");
}

#[test]
fn test_char_maps_to_i16() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("Char".to_string())), "i16");
}

#[test]
fn test_unit_maps_to_empty() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("Unit".to_string())), "");
    assert_eq!(mapper.map(&HirType::Named("Void".to_string())), "");
}

#[test]
fn test_any_and_nothing_map_to_i8_ptr() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("Any".to_string())), "i8*");
    assert_eq!(mapper.map(&HirType::Named("Nothing".to_string())), "i8*");
}

// ────────────────────────────────────────────────────────────────────────────
// A.2-follow-up: String 与 List 都是 i8*，需靠 Aura 类型区分
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_string_and_list_both_i8_ptr() {
    let mapper = TypeMapper::new(false);
    let str_ty = HirType::Named("String".to_string());
    let list_ty = HirType::Named("List".to_string());
    assert_eq!(mapper.map(&str_ty), "i8*");
    assert_eq!(mapper.map(&list_ty), "i8*");
    // 两者 LLVM 类型相同，区分必须靠 Aura 类型系统
}

#[test]
fn test_parameterized_collections_map_to_i8_ptr() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("List<Int>".to_string())), "i8*");
    assert_eq!(
        mapper.map(&HirType::Named("Map<String, Int>".to_string())),
        "i8*"
    );
    assert_eq!(
        mapper.map(&HirType::Named("Set<String>".to_string())),
        "i8*"
    );
    assert_eq!(mapper.map(&HirType::Named("Array<Int>".to_string())), "i8*");
}

#[test]
fn test_runtime_types_map_to_i8_ptr() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("Map".to_string())), "i8*");
    assert_eq!(mapper.map(&HirType::Named("Set".to_string())), "i8*");
    assert_eq!(mapper.map(&HirType::Named("Value".to_string())), "i8*");
    assert_eq!(mapper.map(&HirType::Named("Iterator".to_string())), "i8*");
    assert_eq!(mapper.map(&HirType::Named("Closure".to_string())), "i8*");
}

// ────────────────────────────────────────────────────────────────────────────
// A.3: emit_call 尊重返回类型（函数类型描述）
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_fn_type_simple() {
    let mapper = TypeMapper::new(false);
    let ret = HirType::Named("Int".to_string());
    let params = vec![HirType::Named("Int".to_string())];
    assert_eq!(mapper.fn_type(&ret, &params, false), "i32( i32 )");
}

#[test]
fn test_fn_type_void_return() {
    let mapper = TypeMapper::new(false);
    let ret = HirType::Named("Void".to_string());
    let params = vec![HirType::Named("Int".to_string())];
    assert_eq!(mapper.fn_type(&ret, &params, false), "void( i32 )");
}

#[test]
fn test_fn_type_no_params() {
    let mapper = TypeMapper::new(false);
    let ret = HirType::Named("Int".to_string());
    let params: Vec<HirType> = vec![];
    assert_eq!(mapper.fn_type(&ret, &params, false), "i32( void )");
}

#[test]
fn test_fn_type_vararg() {
    let mapper = TypeMapper::new(false);
    let ret = HirType::Named("Int".to_string());
    let params = vec![HirType::Named("Int".to_string())];
    assert_eq!(mapper.fn_type(&ret, &params, true), "i32( i32, ... )");
}

#[test]
fn test_fn_type_string_params() {
    let mapper = TypeMapper::new(false);
    let ret = HirType::Named("String".to_string());
    let params = vec![HirType::Named("String".to_string())];
    assert_eq!(mapper.fn_type(&ret, &params, false), "i8*( i8* )");
}

// ────────────────────────────────────────────────────────────────────────────
// A.1: 类字段访问按指针语义
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_class_struct_definition() {
    let mapper = TypeMapper::new(false);
    // 用户自定义类型应为指针
    let ty = HirType::Named("Holder".to_string());
    assert_eq!(mapper.map(&ty), "%struct.Holder*");
}

#[test]
fn test_class_as_param_and_return() {
    let mapper = TypeMapper::new(false);
    let ty = HirType::Named("Box".to_string());
    let ret_str = mapper.map(&ty);
    let params = vec![ty];
    let fn_str = mapper.fn_type(&HirType::Named("Void".to_string()), &params, false);
    // 参数应为 %struct.Box*
    assert!(fn_str.contains("%struct.Box*"));
    assert_eq!(ret_str, "%struct.Box*");
}

// ────────────────────────────────────────────────────────────────────────────
// A.4: 函数类型映射（HirType::Function → ptr）
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_function_type_maps_to_ptr() {
    let mapper = TypeMapper::new(false);
    let ty = HirType::Function {
        params: Box::new(vec![HirType::Named("Int".to_string())]),
        return_type: Box::new(HirType::Named("Int".to_string())),
    };
    assert_eq!(mapper.map(&ty), "ptr");
}

#[test]
fn test_pointer_type_maps_to_ptr() {
    let mapper = TypeMapper::new(false);
    let ty = HirType::Pointer(Box::new(HirType::Named("Int".to_string())));
    assert_eq!(mapper.map(&ty), "ptr");
}

#[test]
fn test_unknown_type_maps_to_i8_ptr() {
    let mapper = TypeMapper::new(false);
    let ty = HirType::Unknown;
    assert_eq!(mapper.map(&ty), "i8*");
}

// ────────────────────────────────────────────────────────────────────────────
// A.5: 可空类型映射
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_nullable_scalar_wraps_in_struct() {
    let mapper = TypeMapper::new(false);
    let inner = HirType::Named("Int".to_string());
    let ty = HirType::Nullable(Box::new(inner));
    assert_eq!(mapper.map(&ty), "{ i32, i1 }");
}

#[test]
fn test_nullable_pointer_keeps_inner() {
    let mapper = TypeMapper::new(false);
    let inner = HirType::Named("String".to_string());
    let ty = HirType::Nullable(Box::new(inner));
    assert_eq!(mapper.map(&ty), "i8*");
}

// ────────────────────────────────────────────────────────────────────────────
// A.6: CString / FFI 类型
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ffi_types_map_to_i8_ptr() {
    let mapper = TypeMapper::new(false);
    assert_eq!(mapper.map(&HirType::Named("CString".to_string())), "i8*");
    assert_eq!(mapper.map(&HirType::Named("CStr".to_string())), "i8*");
    assert_eq!(mapper.map(&HirType::Named("Handle".to_string())), "i8*");
}

#[test]
fn test_color_maps_to_struct() {
    let mapper = TypeMapper::new(false);
    assert_eq!(
        mapper.map(&HirType::Named("Color".to_string())),
        "{ i8, i8, i8, i8 }"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// A.7: FFI 函数类型描述（含返回值和参数）
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ffi_fn_type_int_ret_int_params() {
    let mapper = TypeMapper::new(false);
    let ret = HirType::Named("Int".to_string());
    let params = vec![HirType::Named("Int".to_string())];
    let fn_str = mapper.fn_type(&ret, &params, false);
    assert_eq!(fn_str, "i32( i32 )");
}

#[test]
fn test_ffi_fn_type_i8_ptr_ret() {
    let mapper = TypeMapper::new(false);
    let ret = HirType::Named("String".to_string());
    let params: Vec<HirType> = vec![];
    let fn_str = mapper.fn_type(&ret, &params, false);
    assert_eq!(fn_str, "i8*( void )");
}
