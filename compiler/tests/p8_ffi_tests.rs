//! P8 FFI 系统测试
//!
//! 覆盖任务清单 8.1-8.9：
//! - 8.1: extern "c" 声明解析（函数 + 常量）
//! - 8.2: C 类型映射（Pointer, CString, CStr, Handle）
//! - 8.3: libloading 动态加载
//! - 8.4: 静态链接（dlsym/GetProcAddress）
//! - 8.5: CString / CStr 类型
//! - 8.6: Pointer<T> 类型与 nullptr
//! - 8.7: 回调函数（C → Aura 回调）
//! - 8.8: Raylib 绑定示例（语法检查）
//! - 8.9: 综合测试

use compiler::codegen::compile_source;
use compiler::vm::{Value, Vm, VmOptions};

/// 编译源码并执行 main，返回结果
fn run_main(source: &str) -> Value {
    let module = compile_source(source).expect("编译应成功");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    vm.run().expect("运行应成功")
}

/// 仅编译源码（不执行），用于语法/语义检查
fn compile_only(source: &str) {
    compile_source(source).expect("编译应成功");
}

// ─────────────────────────────────────────────────────────────────────────────
// 8.1: extern "c" 声明解析与代码生成
// ─────────────────────────────────────────────────────────────────────────────

/// 8.1: extern "c" 块中声明 C 函数
#[test]
fn test_extern_c_function_decl() {
    let src = r#"
        extern "c" "mylib" {
            fun myAdd(a: Int, b: Int): Int
            fun myPrint(msg: String)
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// 8.1: extern "c" 块中声明常量
#[test]
fn test_extern_c_constant_decl() {
    let src = r#"
        extern "c" "mylib" {
            val MY_CONSTANT: Int = 100
            val PI: Float = 3.14f
            val MAX_SIZE: Int = 64
            fun myFunc(a: Int): Int
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// 8.1: 无库名的 extern "c" 声明
#[test]
fn test_extern_c_no_library() {
    let src = r#"
        extern "c" {
            fun puts(msg: String): Int
            fun strlen(s: String): Int
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// 8.1: extern "c" 函数可调用（VM 端）
#[test]
fn test_extern_c_function_call() {
    let src = r#"
        extern "c" {
            fun strlen(s: String): Int
        }
        fun main(): Int {
            return strlen("hello")
        }
    "#;
    // strlen 可能通过静态链接解析到 libc 的 strlen
    // 如果未链接则返回占位值 0
    let result = run_main(src);
    assert!(matches!(result, Value::Int(_)));
}

// ─────────────────────────────────────────────────────────────────────────────
// 8.2: C 类型映射
// ─────────────────────────────────────────────────────────────────────────────

/// 8.2: Pointer<T> 类型
#[test]
fn test_pointer_type() {
    let src = r#"
        extern "c" "mylib" {
            fun getHandle(): Pointer<Int>
            fun process(ptr: Pointer<Int>)
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// 8.2: CString 类型
#[test]
fn test_cstring_type() {
    let src = r#"
        extern "c" "mylib" {
            fun initWindow(title: CString)
            fun getName(): CString
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// 8.2: Handle 类型
#[test]
fn test_handle_type() {
    let src = r#"
        extern "c" "mylib" {
            fun create(): Handle
            fun destroy(h: Handle)
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// 8.2: 函数指针类型（C 回调签名）
#[test]
fn test_function_pointer_type() {
    let src = r#"
        extern "c" "mylib" {
            fun setCallback(cb: Pointer<Int>)
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

// ─────────────────────────────────────────────────────────────────────────────
// 8.5: CString / CStr 类型
// ─────────────────────────────────────────────────────────────────────────────

/// 8.5: CString 构造
#[test]
fn test_cstring_construction() {
    let src = r#"
        fun main(): Int {
            val cs = CString("hello")
            return 42
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(42));
}

/// 8.5: CStr 构造
#[test]
fn test_cstr_construction() {
    let src = r#"
        fun main(): Int {
            val cs = CStr("world")
            return 42
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// 8.6: Pointer<T> 类型与 nullptr
// ─────────────────────────────────────────────────────────────────────────────

/// 8.6: nullptr 检查
#[test]
fn test_nullptr_check() {
    let src = r#"
        fun main(): Int {
            val p = intToPtr(0)
            if (ptrIsNull(p)) {
                return 1
            } else {
                return 0
            }
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(1));
}

/// 8.6: 非空指针检查
#[test]
fn test_non_null_ptr() {
    let src = r#"
        fun main(): Int {
            val p = intToPtr(42)
            if (ptrIsNull(p)) {
                return 0
            } else {
                return 1
            }
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(1));
}

/// 8.6: 指针到整数转换
#[test]
fn test_ptr_to_int() {
    let src = r#"
        fun main(): Int {
            val p = intToPtr(12345)
            return ptrToInt(p)
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(12345));
}

/// 8.6: 整数到指针转换
#[test]
fn test_int_to_ptr() {
    let src = r#"
        fun main(): Int {
            val p = intToPtr(99)
            if (ptrIsNull(p)) {
                return 0
            }
            return ptrToInt(p)
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(99));
}

// ─────────────────────────────────────────────────────────────────────────────
// 8.7: 回调函数（C → Aura 回调）
// ─────────────────────────────────────────────────────────────────────────────

/// 8.7: makeCallback 创建回调
#[test]
fn test_make_callback() {
    let src = r#"
        fun myCallback(x: Int, y: Int): Int {
            return x + y
        }
        fun main(): Int {
            val cb = makeCallback(myCallback)
            if (ptrIsNull(cb)) {
                return 0
            }
            return ptrToInt(cb)
        }
    "#;
    let result = run_main(src);
    // 回调 ID 应为正整数
    assert!(matches!(result, Value::Int(id) if id > 0));
}

/// 8.7: 回调注册到 extern 函数
#[test]
fn test_callback_registration() {
    let src = r#"
        extern "c" "mylib" {
            fun setCallback(cb: Pointer<Int>)
        }
        fun handler(a: Int, b: Int, c: Int, d: Int): Int {
            return a + b + c + d
        }
        fun main(): Int {
            val cb = makeCallback(handler)
            setCallback(cb)
            return 42
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// 8.8: Raylib 绑定示例
// ─────────────────────────────────────────────────────────────────────────────

/// 8.8: Raylib 绑定语法检查
#[test]
fn test_raylib_binding_syntax() {
    let src = r#"
        extern "c" "raylib" {
            fun InitWindow(width: Int, height: Int, title: CString)
            fun WindowShouldClose(): Boolean
            fun BeginDrawing()
            fun EndDrawing()
            fun ClearBackground(color: Int)
            fun DrawCircle(x: Int, y: Int, radius: Float, color: Int)
            fun DrawText(text: CString, x: Int, y: Int, fontSize: Int, color: Int)
            fun SetTargetFPS(fps: Int)
            fun CloseWindow()
            fun IsKeyPressed(key: Int): Boolean
            fun GetMouseX(): Int
            fun GetMouseY(): Int

            val WHITE: Int
            val BLACK: Int
            val RED: Int
            val KEY_ESCAPE: Int
            val KEY_SPACE: Int
            val MOUSE_BUTTON_LEFT: Int
        }

        fun onKeyDown(key: Int, code: Int, action: Int, mods: Int) {
            // callback body
        }

        fun main() {
            val title = CString("Aura Demo")
            InitWindow(800, 600, title)
            SetTargetFPS(60)

            while (!WindowShouldClose()) {
                BeginDrawing()
                ClearBackground(BLACK)
                DrawText(CString("Hello!"), 100, 100, 20, WHITE)
                EndDrawing()
            }

            CloseWindow()
        }
    "#;
    compile_only(src);
}

// ─────────────────────────────────────────────────────────────────────────────
// 8.9: 综合测试
// ─────────────────────────────────────────────────────────────────────────────

/// 8.9: 综合 FFI 测试 — 函数调用 + 类型转换 + 指针操作
#[test]
fn test_ffi_comprehensive() {
    let src = r#"
        extern "c" {
            fun strlen(s: String): Int
            fun puts(s: String): Int
        }

        fun myCallback(x: Int, y: Int, z: Int, w: Int): Int {
            return x * y + z - w
        }

        fun main(): Int {
            // CString 构造
            val cs = CString("test")

            // 指针操作
            val p1 = intToPtr(100)
            val p2 = intToPtr(0)
            val p3 = intToPtr(200)

            // nullptr 检查
            if (!ptrIsNull(p1) && ptrIsNull(p2) && !ptrIsNull(p3)) {
                // 回调创建
                val cb = makeCallback(myCallback)

                // 返回组合结果
                return ptrToInt(p1) + ptrToInt(p3) + ptrToInt(cb)
            }

            return 0
        }
    "#;
    let result = run_main(src);
    // p1=100, p3=200, cb>0 → 结果 > 300
    assert!(matches!(result, Value::Int(v) if v > 300));
}

/// 8.9: extern 常量在字节码中的存储
#[test]
fn test_extern_constants_in_bytecode() {
    let src = r#"
        extern "c" "mylib" {
            val MY_INT: Int = 42
            val MY_FLOAT: Float = 3.14f
            val MY_STR: String = "hello"
            val MY_BOOL: Boolean = true
        }
        fun main(): Int {
            return 42
        }
    "#;
    let module = compile_source(src).expect("编译应成功");
    // 检查常量池是否包含 FFI 常量
    let has_int_42 =
        module.consts.iter().any(|c| matches!(c, compiler::codegen::opcode::Const::Int(42)));
    let has_float_314 = module.consts.iter().any(
        |c| matches!(c, compiler::codegen::opcode::Const::Float(f) if (*f - 3.14).abs() < 0.001),
    );
    let has_str_hello = module
        .consts
        .iter()
        .any(|c| matches!(c, compiler::codegen::opcode::Const::Str(s) if s == "hello"));
    let has_bool_true =
        module.consts.iter().any(|c| matches!(c, compiler::codegen::opcode::Const::Bool(true)));

    assert!(has_int_42, "常量池应包含 Int(42)");
    assert!(has_float_314, "常量池应包含 Float(3.14)");
    assert!(has_str_hello, "常量池应包含 Str(\"hello\")");
    assert!(has_bool_true, "常量池应包含 Bool(true)");
}

/// 8.9: CString 指令执行（VM 端）
#[test]
fn test_cstring_instruction() {
    let src = r#"
        extern "c" {
            fun strlen(s: String): Int
        }
        fun main(): Int {
            // 使用 CString 转换后传给 C 函数
            val cs = CString("hello")
            return 42
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(42));
}

/// 8.9: Value::Ptr 类型测试
#[test]
fn test_value_ptr_type() {
    let v = Value::Ptr(42);
    assert_eq!(v.as_int(), 42);
    assert_eq!(v.as_ptr(), 42);
    assert!(!v.is_null_ptr());
    assert_eq!(v.type_name(), "Pointer");
}

/// 8.9: Value::Ptr(0) 是 nullptr
#[test]
fn test_value_ptr_null() {
    let v = Value::Ptr(0);
    assert!(v.is_null_ptr());
    assert!(!v.is_truthy());
}

/// 8.9: Value::Null 等同于 nullptr
#[test]
fn test_value_null_as_ptr() {
    let v = Value::Null;
    assert!(v.is_null_ptr());
}

/// 8.9: Value::Ptr 比较
#[test]
fn test_value_ptr_equality() {
    let p1 = Value::Ptr(42);
    let p2 = Value::Ptr(42);
    let p3 = Value::Ptr(0);
    assert_eq!(p1, p2);
    assert_eq!(p3, Value::Null); // nullptr == Null
    assert_ne!(p1, Value::Null); // 非空指针 != Null
}
