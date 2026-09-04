use aura_compiler::vm::{Vm, VmOptions};
use aura_compiler::codegen::opcode::{Const, Instr};
use aura_compiler::codegen::CompileOptions;

fn make_sum_vm() -> Vm {
    let mut v = Vm::new(VmOptions::default());
    let sum_code = vec![
        Instr::LoadConst(0), Instr::StoreVar(1),
        Instr::LoadConst(1), Instr::StoreVar(2),
        Instr::LoadConst(2), Instr::StoreVar(3),
        Instr::LoadConst(3), Instr::StoreVar(4),
        Instr::LoadVar(3), Instr::LoadVar(2), Instr::Lt,
        Instr::JumpIfFalse(20),
        Instr::LoadVar(1), Instr::LoadVar(3), Instr::Add, Instr::StoreVar(1),
        Instr::LoadVar(3), Instr::LoadConst(4), Instr::Add, Instr::StoreVar(3),
        Instr::Jump(4),
        Instr::Trap,
        Instr::LoadVar(1), Instr::Return,
    ];
    v.run_bytecode(&sum_code, &[Const::Int(0), Const::Int(0), Const::Int(0), Const::Int(60000), Const::Int(1)]);
    v
}

fn main() {
    println!("=== 完整 JIT 优化基准测试 ===\n");
    
    // fib(25) test
    println!("--- fib(25) ---");
    let fib_code = vec![
        Instr::LoadConst(0), Instr::StoreVar(1),
        Instr::LoadVar(1), Instr::LoadConst(1), Instr::Lt, Instr::JumpIfFalse(20),
        Instr::LoadVar(1), Instr::Return,
        Instr::LoadVar(1), Instr::LoadConst(2), Instr::Sub, Instr::LoadVar(0), Instr::Call(0),
        Instr::LoadVar(1), Instr::LoadConst(1), Instr::Sub, Instr::LoadVar(0), Instr::Call(0),
        Instr::Add, Instr::StoreVar(2),
        Instr::LoadVar(2), Instr::Return,
    ];
    
    let mut vm_vm = Vm::new(VmOptions::default());
    vm_vm.force_jit_disabled();
    let start = std::time::Instant::now();
    vm_vm.run_bytecode(&fib_code, &[Const::Int(25)]);
    let vm_time = start.elapsed().as_secs_f64() * 1000.0;
    
    let mut vm_jit = Vm::new(VmOptions { jit: true, ..Default::default() });
    let start = std::time::Instant::now();
    vm_jit.run_bytecode(&fib_code, &[Const::Int(25)]);
    let jit_time = start.elapsed().as_secs_f64() * 1000.0;
    
    println!("  VM (解释器): {:.4} ms/op", vm_time);
    println!("  JIT (优化后): {:.4} ms/op (加速 {:.1}x)", jit_time, vm_time / jit_time);
    println!();
    
    // sum(60000) test
    println!("--- sum(60000) ---");
    let mut vm_vm2 = Vm::new(VmOptions::default());
    vm_vm2.force_jit_disabled();
    let start = std::time::Instant::now();
    vm_vm2.run_bytecode(&sum_code, &[Const::Int(0), Const::Int(0), Const::Int(0), Const::Int(60000), Const::Int(1)]);
    let vm_time2 = start.elapsed().as_secs_f64() * 1000.0;
    
    let mut vm_jit2 = Vm::new(VmOptions { jit: true, ..Default::default() });
    let start = std::time::Instant::now();
    vm_jit2.run_bytecode(&sum_code, &[Const::Int(0), Const::Int(0), Const::Int(0), Const::Int(60000), Const::Int(1)]);
    let jit_time2 = start.elapsed().as_secs_f64() * 1000.0;
    
    println!("  VM (解释器): {:.4} ms/op", vm_time2);
    println!("  JIT (优化后): {:.4} ms/op (加速 {:.1}x)", jit_time2, vm_time2 / jit_time2);
}