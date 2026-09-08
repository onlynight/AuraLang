// demo_rustffi/utils.rs — Rust 绑定文件
//
// 展示同一 Aura AOT DLL 可被 Rust 直接调用，无需额外编译步骤。
// Rust 通过 extern "C" 直接消费 aura_c_* 导出（与 C FFI 相同的 C ABI）。
//
// 编译命令（可选，展示 Rust 兼容性）：
//   rustc utils.rs -L target/build/libs/utils/ -l utils
//
// 或者在 Cargo 项目中：
//   1. 创建 build.rs，在 build script 中 add_library 指向 utils.dll
//   2. 在本文件中使用 #[link(name = "utils", kind = "dylib")]
//
// 运行验证：
//   $ rustc utils.rs -L ../../target/build/libs/utils/ -l utils
//   $ ./utils
//   Rust calls aura_c_add(3, 4) = 7
//   Rust calls aura_c_factorial(5) = 120

/// 链接到 Aura AOT 生成的 utils 动态库
#[link(name = "utils", kind = "dylib")]
extern "C" {
    pub fn aura_c_add(a: i32, b: i32) -> i32;
    pub fn aura_c_multiply(a: i32, b: i32) -> i32;
    pub fn aura_c_factorial(n: i32) -> i32;
    pub fn aura_c_power(base: i32, exp: i32) -> i32;
}

fn main() {
    // 调用 Aura AOT 编译的 add 函数
    let sum = unsafe { aura_c_add(3, 4) };
    println!("Rust calls aura_c_add(3, 4) = {}", sum);

    // 调用 Aura AOT 编译的 multiply 函数
    let prod = unsafe { aura_c_multiply(3, 4) };
    println!("Rust calls aura_c_multiply(3, 4) = {}", prod);

    // 调用 Aura AOT 编译的 factorial 函数
    let fact = unsafe { aura_c_factorial(5) };
    println!("Rust calls aura_c_factorial(5) = {}", fact);

    // 调用 Aura AOT 编译的 power 函数
    let pw = unsafe { aura_c_power(2, 10) };
    println!("Rust calls aura_c_power(2, 10) = {}", pw);
}
