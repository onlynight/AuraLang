//! Aura VM Hotspot JIT Compiler (Cranelift)
use std::collections::HashMap;
use crate::codegen::opcode::Const;
use crate::vm::value::Value;
use crate::vm::{DecodedFunction, Instr};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JitValue { pub tag: i64, pub payload: i64 }

pub const TAG_INT: i64 = 0;
pub const TAG_FLOAT: i64 = 1;
pub const TAG_BOOL: i64 = 2;
pub const TAG_NULL: i64 = 3;

impl JitValue {
    pub fn null() -> Self { JitValue { tag: TAG_NULL, payload: 0 } }
    pub fn from_value(v: &Value) -> Self {
        match v {
            Value::Int(i) => JitValue { tag: TAG_INT, payload: *i },
            Value::Float(f) => JitValue { tag: TAG_FLOAT, payload: f.to_bits() as i64 },
            Value::Bool(b) => JitValue { tag: TAG_BOOL, payload: *b as i64 },
            _ => JitValue::null(),
        }
    }
    pub fn to_value(self) -> Value {
        match self.tag {
            TAG_INT => Value::Int(self.payload),
            TAG_FLOAT => Value::Float(f64::from_bits(self.payload as u64)),
            TAG_BOOL => Value::Bool(self.payload != 0),
            _ => Value::Null,
        }
    }
}

pub type JitEntry = unsafe extern "C" fn(*const JitValue, *mut JitValue, usize, *const ());

pub struct JitState {
    compiled: HashMap<usize, JitEntry>,
    skipped: HashMap<usize, ()>,
    dispatch_table: Vec<Option<JitEntry>>,
}

impl Default for JitState {
    fn default() -> Self {
        JitState { compiled: HashMap::new(), skipped: HashMap::new(), dispatch_table: Vec::new() }
    }
}

impl JitState {
    pub fn ensure_capacity(&mut self, len: usize) {
        while self.dispatch_table.len() < len { self.dispatch_table.push(None); }
    }
    pub fn new() -> Self { JitState::default() }
    pub fn is_compiled(&self, idx: usize) -> bool { self.compiled.contains_key(&idx) }
    pub fn is_skipped(&self, idx: usize) -> bool { self.skipped.contains_key(&idx) }
    pub fn insert(&mut self, idx: usize, entry: JitEntry) {
        while self.dispatch_table.len() <= idx { self.dispatch_table.push(None); }
        self.dispatch_table[idx] = Some(entry);
        self.compiled.insert(idx, entry);
    }
    pub fn skip(&mut self, idx: usize) { self.skipped.insert(idx, ()); }
    fn dispatch_table_ptr(&self) -> *const () { self.dispatch_table.as_ptr() as *const () }
    pub fn call(&self, idx: usize, args: &[JitValue]) -> Option<JitValue> {
        let entry = *self.compiled.get(&idx)?;
        let mut out = JitValue::null();
        unsafe { entry(args.as_ptr(), &mut out, args.len(), self.dispatch_table_ptr()) };
        Some(out)
    }
    pub unsafe fn invoke(&self, idx: usize, args: *const JitValue, out: *mut JitValue, argc: usize) {
        if let Some(entry) = self.compiled.get(&idx) {
            unsafe { entry(args, out, argc, self.dispatch_table_ptr()) };
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aura_jit_dispatch(
    dispatch_table: *const (), callee_idx: usize, args: *const JitValue, out: *mut JitValue, argc: usize,
) {
    let table = dispatch_table as *const Option<JitEntry>;
    let entry = unsafe { *table.add(callee_idx) };
    match entry {
        None => return,
        Some(f) => unsafe { f(args, out, argc, dispatch_table) },
    }
}

pub fn is_jit_compilable(idx: usize, _f: &DecodedFunction, consts: &[Const], funcs: &[DecodedFunction]) -> bool {
    is_jit_compilable_inner(idx, consts, funcs, &mut std::collections::HashSet::new())
}

fn is_jit_compilable_inner(
    idx: usize, consts: &[Const], funcs: &[DecodedFunction], in_progress: &mut std::collections::HashSet<usize>,
) -> bool {
    if idx >= funcs.len() { return false; }
    if !in_progress.insert(idx) { return true; }
    let f = &funcs[idx];
    for instr in &f.code {
        match instr {
            Instr::LoadConst(ci) => match consts.get(*ci as usize) {
                Some(Const::Int(_)) => {} _ => return false,
            },
            Instr::LoadVar(_) | Instr::StoreVar(_) | Instr::Add | Instr::Sub | Instr::Mul
            | Instr::Div | Instr::Rem | Instr::Neg | Instr::Eq | Instr::Ne | Instr::Lt
            | Instr::Gt | Instr::Le | Instr::Ge | Instr::Jump(_) | Instr::JumpIfTrue(_)
            | Instr::JumpIfFalse(_) | Instr::Return => {}
            Instr::Call(ci) => {
                if !is_jit_compilable_inner(*ci as usize, consts, funcs, in_progress) { return false; }
            }
            _ => return false,
        }
    }
    true
}

pub fn compile_function(
    idx: usize, f: &DecodedFunction, consts: &[Const], funcs: &[DecodedFunction],
) -> Option<JitEntry> {
    if !is_jit_compilable(idx, f, consts, funcs) { return None; }
    
    // Run optimization passes before Cranelift compilation
    let opt_result = crate::vm::jit_opt::optimize_function_with_deps(f, consts, funcs);
    
    // Combine original and extra constants
    let mut all_consts = consts.to_vec();
    all_consts.extend(opt_result.extra_consts);
    
    cranelift_backend::jit_compile_cranelift(&opt_result.func, &all_consts, funcs)
}

#[cfg(feature = "jit")]
mod cranelift_backend {
    use super::{DecodedFunction, Instr, JitEntry, TAG_BOOL, TAG_INT};
    use crate::codegen::opcode::Const;
    use std::collections::HashMap;
    use cranelift::codegen::ir::{
        condcodes::IntCC, immediates::Offset32, types, AbiParam, InstBuilder, MemFlags,
        Signature, StackSlotData, StackSlotKind, TrapCode, Value as IrValue,
    };
    use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
    use cranelift::codegen::ir::Block;
    use cranelift_jit::{JITBuilder, JITModule};
    use cranelift_module::{default_libcall_names, Linkage, Module};

    const VALUE_BYTES: i64 = 16;
    const MAX_STACK: i64 = 256;

    pub(super) fn jit_compile_cranelift(
        f: &DecodedFunction, consts: &[Const], funcs: &[DecodedFunction],
    ) -> Option<JitEntry> {
        // Cranelift optimization flags:
        // - opt_level: "speed" for maximum performance
        // - enable_verifier: false to speed up compilation (no runtime impact)
        let flags = &[
            ("opt_level", "speed"),
            ("enable_verifier", "false"),
        ];
        let builder = JITBuilder::with_flags(flags, default_libcall_names()).ok()?;
        let mut module = JITModule::new(builder);
        let tc = module.target_config();
        let ptr_ty = tc.pointer_type();
        let call_conv = tc.default_call_conv;

        let sig = Signature {
            params: vec![AbiParam::new(ptr_ty), AbiParam::new(ptr_ty),
                AbiParam::new(types::I64), AbiParam::new(ptr_ty)],
            returns: vec![], call_conv,
        };

        let func_id = module.declare_function("aura_jit_entry", Linkage::Export, &sig).ok()?;
        let mut ctx = module.make_context();
        ctx.func.signature = sig;

        let targets = compute_targets(f);
        let mut blocks: HashMap<usize, Block> = HashMap::new();
        let mut pred_total: HashMap<usize, usize> = HashMap::new();
        for (idx, instr) in f.code.iter().enumerate() {
            match instr {
                Instr::Jump(t) => { *pred_total.entry(*t).or_insert(0) += 1; }
                Instr::JumpIfTrue(t) | Instr::JumpIfFalse(t) => {
                    *pred_total.entry(*t).or_insert(0) += 1;
                    *pred_total.entry(idx + 1).or_insert(0) += 1;
                }
                _ => {}
            }
        }
        let mut pred_declared: HashMap<usize, usize> = HashMap::new();
        let mut sealed: std::collections::HashSet<usize> = std::collections::HashSet::new();

        {
            let mut fbctx = FunctionBuilderContext::new();
            let mut fb = FunctionBuilder::new(&mut ctx.func, &mut fbctx);
            let entry = fb.create_block();
            fb.append_block_params_for_function_params(entry);
            blocks.insert(0, entry);
            for &t in targets.iter().skip(1) {
                if t < f.code.len() { blocks.insert(t, fb.create_block()); }
            }
            fb.switch_to_block(entry);

            let jit_entry_sig = Signature {
                params: vec![AbiParam::new(ptr_ty), AbiParam::new(ptr_ty),
                    AbiParam::new(types::I64), AbiParam::new(ptr_ty)],
                returns: vec![], call_conv,
            };
            let jit_entry_sig_ref = fb.import_signature(jit_entry_sig);

            if pred_total.get(&0).copied().unwrap_or(0) == 0 {
                sealed.insert(0); fb.seal_block(entry);
            }

            let tag_vars: Vec<Variable> = (0..f.locals as usize)
                .map(|i| Variable::from_bits(100 + i as u32 * 2)).collect();
            let payload_vars: Vec<Variable> = (0..f.locals as usize)
                .map(|i| Variable::from_bits(100 + i as u32 * 2 + 1)).collect();
            for v in tag_vars.iter().chain(payload_vars.iter()) {
                fb.declare_var(*v, types::I64);
                let zero = fb.ins().iconst(types::I64, 0);
                fb.def_var(*v, zero);
            }

            let stack_slot = fb.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot, (VALUE_BYTES * MAX_STACK) as u32, 4));
            let sp = Variable::from_bits(3);
            fb.declare_var(sp, types::I64);
            let zero_sp = fb.ins().iconst(types::I64, 0);
            fb.def_var(sp, zero_sp);

            let args_ptr = Variable::from_bits(0);
            let out_ptr = Variable::from_bits(1);
            let dispatch_table = Variable::from_bits(4);
            fb.declare_var(args_ptr, ptr_ty);
            fb.def_var(args_ptr, fb.block_params(entry)[0]);
            fb.declare_var(out_ptr, ptr_ty);
            fb.def_var(out_ptr, fb.block_params(entry)[1]);
            fb.declare_var(dispatch_table, ptr_ty);
            fb.def_var(dispatch_table, fb.block_params(entry)[3]);

            for i in 0..f.param_count as usize {
                let off = (i as i64) * VALUE_BYTES;
                let base = fb.block_params(entry)[0];
                let addr = fb.ins().iadd_imm(base, off);
                let tag = fb.ins().load(types::I64, MemFlags::new(), addr, Offset32::new(0));
                let payload = fb.ins().load(types::I64, MemFlags::new(), addr, Offset32::new(8));
                if let Some(tv) = tag_vars.get(i) { fb.def_var(*tv, tag); }
                if let Some(pv) = payload_vars.get(i) { fb.def_var(*pv, payload); }
            }

            let mut current = entry;
            let mut terminated = false;
            for (idx, instr) in f.code.iter().enumerate() {
                if let Some(&b) = blocks.get(&idx) {
                    if b != current {
                        if !terminated {
                            fb.ins().jump(b, &[]);
                            seal_if_complete(&mut fb, idx, &pred_total, &mut pred_declared, &mut sealed, &blocks);
                        }
                        fb.switch_to_block(b);
                        current = b;
                        terminated = false;
                    }
                } else if terminated {
                    let b = fb.create_block();
                    fb.switch_to_block(b);
                    current = b;
                    terminated = false;
                }

                let term = emit_instr(&mut fb, instr, consts, funcs, &tag_vars, &payload_vars,
                    &blocks, idx, &stack_slot, sp, args_ptr, out_ptr, ptr_ty, jit_entry_sig_ref, dispatch_table);
                terminated = terminated || term;

                if term {
                    match instr {
                        Instr::Jump(t) => {
                            seal_if_complete(&mut fb, *t, &pred_total, &mut pred_declared, &mut sealed, &blocks);
                        }
                        Instr::JumpIfTrue(t) | Instr::JumpIfFalse(t) => {
                            seal_if_complete(&mut fb, *t, &pred_total, &mut pred_declared, &mut sealed, &blocks);
                            seal_if_complete(&mut fb, idx + 1, &pred_total, &mut pred_declared, &mut sealed, &blocks);
                        }
                        _ => {}
                    }
                }
            }
            if !terminated { fb.ins().return_(&[]); }
            fb.seal_all_blocks();
            fb.finalize();
        }

        if module.define_function(func_id, &mut ctx).is_err() { return None; }
        module.clear_context(&mut ctx);
        if module.finalize_definitions().is_err() { return None; }
        let code = module.get_finalized_function(func_id);
        std::mem::forget(module);
        Some(unsafe { std::mem::transmute::<*const u8, JitEntry>(code) })
    }

    fn compute_targets(f: &DecodedFunction) -> Vec<usize> {
        let mut set = std::collections::BTreeSet::new();
        set.insert(0);
        for (idx, instr) in f.code.iter().enumerate() {
            match instr {
                Instr::Jump(t) => { set.insert(*t); set.insert(idx + 1); }
                Instr::JumpIfTrue(t) | Instr::JumpIfFalse(t) => { set.insert(*t); set.insert(idx + 1); }
                Instr::Return => { set.insert(idx + 1); }
                _ => {}
            }
        }
        set.into_iter().collect()
    }

    fn seal_if_complete(
        fb: &mut FunctionBuilder, t: usize,
        pred_total: &HashMap<usize, usize>, pred_declared: &mut HashMap<usize, usize>,
        sealed: &mut std::collections::HashSet<usize>, blocks: &HashMap<usize, Block>,
    ) {
        let d = pred_declared.entry(t).or_insert(0);
        *d += 1;
        if *d == pred_total.get(&t).copied().unwrap_or(0) && sealed.insert(t) {
            if let Some(&b) = blocks.get(&t) { fb.seal_block(b); }
        }
    }

    fn emit_instr(
        fb: &mut FunctionBuilder, instr: &Instr, consts: &[Const], funcs: &[DecodedFunction],
        tag_vars: &[Variable], payload_vars: &[Variable], blocks: &HashMap<usize, Block>,
        idx: usize, stack_slot: &cranelift::codegen::ir::StackSlot, sp: Variable,
        _args_ptr: Variable, out_ptr: Variable, ptr_ty: types::Type,
        jit_entry_sig_ref: cranelift::codegen::ir::SigRef, dispatch_table: Variable,
    ) -> bool {
        let i64_ty = types::I64;
        let zero32 = Offset32::new(0);
        let eight32 = Offset32::new(8);

        let mut push = |fb: &mut FunctionBuilder, tag: IrValue, payload: IrValue| {
            let cur = fb.use_var(sp);
            let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
            let slot_off = fb.ins().imul_imm(cur, VALUE_BYTES);
            let addr = fb.ins().iadd(base, slot_off);
            fb.ins().store(MemFlags::new(), tag, addr, zero32);
            fb.ins().store(MemFlags::new(), payload, addr, eight32);
            let new_sp = fb.ins().iadd_imm(cur, 1);
            fb.def_var(sp, new_sp);
        };

        let mut pop = |fb: &mut FunctionBuilder| -> (IrValue, IrValue) {
            let cur = fb.use_var(sp);
            let new_sp = fb.ins().iadd_imm(cur, -1);
            fb.def_var(sp, new_sp);
            let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
            let slot_off = fb.ins().imul_imm(new_sp, VALUE_BYTES);
            let addr = fb.ins().iadd(base, slot_off);
            let tag = fb.ins().load(i64_ty, MemFlags::new(), addr, zero32);
            let payload = fb.ins().load(i64_ty, MemFlags::new(), addr, eight32);
            (tag, payload)
        };

        match instr {
            Instr::LoadConst(ci) => {
                if let Some(Const::Int(v)) = consts.get(*ci as usize) {
                    let tag = fb.ins().iconst(i64_ty, TAG_INT);
                    let payload = fb.ins().iconst(i64_ty, *v);
                    push(fb, tag, payload);
                } else { fb.ins().trap(TrapCode::unwrap_user(2)); }
                false
            }
            Instr::LoadVar(s) => {
                let tag = fb.use_var(tag_vars[*s as usize]);
                let payload = fb.use_var(payload_vars[*s as usize]);
                push(fb, tag, payload);
                false
            }
            Instr::StoreVar(s) => {
                let (tag, payload) = pop(fb);
                fb.def_var(tag_vars[*s as usize], tag);
                fb.def_var(payload_vars[*s as usize], payload);
                false
            }
            Instr::Add => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().iadd(x, y)),
            Instr::Sub => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().isub(x, y)),
            Instr::Mul => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().imul(x, y)),
            Instr::Div => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().sdiv(x, y)),
            Instr::Rem => bin_int(fb, &mut pop, &mut push, |fb, x, y| fb.ins().srem(x, y)),
            Instr::Neg => {
                let (_t, p) = pop(fb);
                let r = fb.ins().ineg(p);
                let tag = fb.ins().iconst(i64_ty, TAG_INT);
                push(fb, tag, r);
                false
            }
            Instr::Eq => cmp_int(fb, &mut pop, &mut push, IntCC::Equal),
            Instr::Ne => cmp_int(fb, &mut pop, &mut push, IntCC::NotEqual),
            Instr::Lt => cmp_int(fb, &mut pop, &mut push, IntCC::SignedLessThan),
            Instr::Gt => cmp_int(fb, &mut pop, &mut push, IntCC::SignedGreaterThan),
            Instr::Le => cmp_int(fb, &mut pop, &mut push, IntCC::SignedLessThanOrEqual),
            Instr::Ge => cmp_int(fb, &mut pop, &mut push, IntCC::SignedGreaterThanOrEqual),
            Instr::Jump(target) => {
                if let Some(&b) = blocks.get(target) { fb.ins().jump(b, &[]); }
                true
            }
            Instr::JumpIfTrue(target) => {
                let (_t, p) = pop(fb);
                let cond = fb.ins().icmp_imm(IntCC::NotEqual, p, 0);
                match (blocks.get(target), blocks.get(&(idx + 1))) {
                    (Some(&tb), Some(&nb)) => { fb.ins().brif(cond, tb, &[], nb, &[]); }
                    (Some(&tb), None) => { fb.ins().jump(tb, &[]); }
                    _ => { fb.ins().trap(TrapCode::unwrap_user(1)); }
                }
                true
            }
            Instr::JumpIfFalse(target) => {
                let (_t, p) = pop(fb);
                let cond = fb.ins().icmp_imm(IntCC::NotEqual, p, 0);
                match (blocks.get(target), blocks.get(&(idx + 1))) {
                    (Some(&tb), Some(&nb)) => { fb.ins().brif(cond, nb, &[], tb, &[]); }
                    (Some(&tb), None) => { fb.ins().jump(tb, &[]); }
                    _ => { fb.ins().trap(TrapCode::unwrap_user(1)); }
                }
                true
            }
            Instr::Return => {
                let (tag, payload) = pop(fb);
                let out = fb.use_var(out_ptr);
                fb.ins().store(MemFlags::new(), tag, out, zero32);
                fb.ins().store(MemFlags::new(), payload, out, eight32);
                fb.ins().return_(&[]);
                true
            }
            Instr::Call(callee_idx) => {
                let callee_idx = *callee_idx as usize;
                let callee = &funcs[callee_idx];
                let param_count = callee.param_count as i64;

                let cur_sp = fb.use_var(sp);
                let args_sp = fb.ins().iadd_imm(cur_sp, -param_count);

                let base = fb.ins().stack_addr(ptr_ty, *stack_slot, 0);
                let args_off = fb.ins().imul_imm(args_sp, VALUE_BYTES);
                let args_ptr_val = fb.ins().iadd(base, args_off);

                let out_off = fb.ins().imul_imm(cur_sp, VALUE_BYTES);
                let out_ptr_val = fb.ins().iadd(base, out_off);

                let dt = fb.use_var(dispatch_table);
                let entry_off = fb.ins().iconst(types::I64, callee_idx as i64 * 8);
                let entry_addr = fb.ins().iadd(dt, entry_off);
                let entry_ptr = fb.ins().load(ptr_ty, MemFlags::new(), entry_addr, zero32);

                let argc_val = fb.ins().iconst(types::I64, param_count);
                fb.ins().call_indirect(jit_entry_sig_ref, entry_ptr, &[args_ptr_val, out_ptr_val, argc_val, dt]);

                fb.def_var(sp, args_sp);

                let ret_tag = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, zero32);
                let ret_payload = fb.ins().load(i64_ty, MemFlags::new(), out_ptr_val, eight32);
                push(fb, ret_tag, ret_payload);

                false
            }
            _ => { fb.ins().trap(TrapCode::unwrap_user(2)); true }
        }
    }

    fn bin_int<F>(
        fb: &mut FunctionBuilder, pop: &mut dyn FnMut(&mut FunctionBuilder) -> (IrValue, IrValue),
        push: &mut dyn FnMut(&mut FunctionBuilder, IrValue, IrValue), f: F,
    ) -> bool where F: FnOnce(&mut FunctionBuilder, IrValue, IrValue) -> IrValue {
        let (_bt, b) = pop(fb);
        let (_at, a) = pop(fb);
        let r = f(fb, a, b);
        let tag = fb.ins().iconst(types::I64, TAG_INT);
        push(fb, tag, r);
        false
    }

    fn cmp_int(
        fb: &mut FunctionBuilder, pop: &mut dyn FnMut(&mut FunctionBuilder) -> (IrValue, IrValue),
        push: &mut dyn FnMut(&mut FunctionBuilder, IrValue, IrValue), cc: IntCC,
    ) -> bool {
        let (_bt, b) = pop(fb);
        let (_at, a) = pop(fb);
        let c = fb.ins().icmp(cc, a, b);
        let r = fb.ins().uextend(types::I64, c);
        let tag = fb.ins().iconst(types::I64, TAG_BOOL);
        push(fb, tag, r);
        false
    }
}

#[cfg(not(feature = "jit"))]
fn jit_compile_cranelift(
    _f: &DecodedFunction, _consts: &[Const], _funcs: &[DecodedFunction],
) -> Option<JitEntry> { None }