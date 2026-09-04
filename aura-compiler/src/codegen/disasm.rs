//! 字节码反汇编器（对应 P4.13）
//!
//! 将 `.auc` 中的字节码解码为人类可读的汇编文本，用于调试与验证
//! （"`.auc` 文件可反汇编回读"里程碑）。

use crate::codegen::opcode::{BytecodeModule, OpCode};

/// 反汇编整个模块为文本
pub fn disassemble(module: &BytecodeModule) -> String {
    let mut out = String::new();
    out.push_str("; Aura bytecode (AURA v1)\n");
    out.push_str(&format!("; entry = {}\n", module.entry));

    // 常量池
    out.push_str("\n; === consts ===\n");
    for (i, c) in module.consts.iter().enumerate() {
        out.push_str(&format!("  C{:>4}  {}\n", i, const_display(c)));
    }

    // 原生函数
    if !module.natives.is_empty() {
        out.push_str("\n; === natives ===\n");
        for (i, n) in module.natives.iter().enumerate() {
            out.push_str(&format!(
                "  N{:>4}  {} ({} param(s))\n",
                i, n.name, n.param_count
            ));
        }
    }

    // 函数
    out.push_str("\n; === functions ===\n");
    for (i, f) in module.functions.iter().enumerate() {
        if f.is_native {
            out.push_str(&format!("\n; --- {} (native) ---\n", f.name));
            continue;
        }
        out.push_str(&format!(
            "\n; --- {} (func {}) : {} param(s), {} local(s) ---\n",
            f.name, i, f.param_count, f.locals
        ));
        out.push_str(&disassemble_function(module, f));
    }

    out
}

fn disassemble_function(
    module: &BytecodeModule,
    f: &crate::codegen::opcode::BytecodeFunction,
) -> String {
    let mut out = String::new();
    let code = &f.code;
    let mut pos = 0usize;
    while pos < code.len() {
        let start = pos;
        let byte = code[pos];
        let op = match OpCode::from_byte(byte) {
            Some(o) => o,
            None => {
                out.push_str(&format!("{:>6}  ??? (byte {})\n", start, byte));
                pos += 1;
                continue;
            }
        };
        let opsize = OpCode::operand_size(byte);
        let operand_bytes = if pos + 1 + opsize <= code.len() {
            &code[pos + 1..pos + 1 + opsize]
        } else {
            &[][..]
        };

        let mut line = format!(
            "{:>6}  {}",
            start,
            opcode_display(module, &op, operand_bytes)
        );

        // 处理操作数偏移（让 Jump 显示目标地址更易读）
        if let OpCode::Jump(o) | OpCode::JumpIfTrue(o) | OpCode::JumpIfFalse(o) = op {
            line.push_str(&format!("   ; -> {}", o));
        }

        out.push_str(&line);
        out.push('\n');

        pos += 1 + opsize;
    }
    out
}

fn opcode_display(module: &BytecodeModule, op: &OpCode, operand: &[u8]) -> String {
    let read_u16 = || -> u16 {
        if operand.len() >= 2 {
            u16::from_le_bytes([operand[0], operand[1]])
        } else {
            0
        }
    };
    let read_i32 = || -> i32 {
        if operand.len() >= 4 {
            i32::from_le_bytes([operand[0], operand[1], operand[2], operand[3]])
        } else {
            0
        }
    };

    match op {
        // 注意：op 由 from_byte 还原，操作数携带占位 0，真实操作数须从 operand 字节读取
        OpCode::LoadConst(_) => {
            let i = read_u16();
            let v = module
                .consts
                .get(i as usize)
                .map(const_display)
                .unwrap_or_else(|| "?".into());
            format!("LOAD_CONST {}   ; {}", i, v)
        }
        OpCode::LoadVar(_) => format!("LOAD_VAR {}", read_u16()),
        OpCode::StoreVar(_) => format!("STORE_VAR {}", read_u16()),
        OpCode::Call(_) => {
            let i = read_u16();
            let name = module
                .functions
                .get(i as usize)
                .map(|f| f.name.as_str())
                .unwrap_or("?");
            format!("CALL {}   ; {}", i, name)
        }
        OpCode::CallNative(_) => {
            let i = read_u16();
            let name = module
                .natives
                .get(i as usize)
                .map(|n| n.name.as_str())
                .unwrap_or("?");
            format!("CALL_NATIVE {}   ; {}", i, name)
        }
        OpCode::NewObject(_) => format!("NEW_OBJECT {}", read_u16()),
        OpCode::GetField(_) => format!("GET_FIELD {}", read_u16()),
        OpCode::SetField(_) => format!("SET_FIELD {}", read_u16()),
        OpCode::Jump(_) => format!("JUMP {}", read_i32()),
        OpCode::JumpIfTrue(_) => format!("JUMP_IF_TRUE {}", read_i32()),
        OpCode::JumpIfFalse(_) => format!("JUMP_IF_FALSE {}", read_i32()),
        OpCode::CallC(_) => format!("CALL_C {}", read_u16()),
        other => other.to_string(),
    }
}

fn const_display(c: &crate::codegen::opcode::Const) -> String {
    match c {
        crate::codegen::opcode::Const::Int(i) => format!("int {}", i),
        crate::codegen::opcode::Const::Float(fl) => format!("float {}", fl),
        crate::codegen::opcode::Const::Str(s) => format!("str {:?}", s),
        crate::codegen::opcode::Const::Bool(b) => format!("bool {}", b),
        crate::codegen::opcode::Const::Null => "null".into(),
    }
}
