//! Aura debugger core engine

use crate::codegen::opcode::BytecodeModule;
use crate::source_map::{FileId, SourceMap};
use crate::vm::{LoadedModule, Value, Vm, VmError, VmOptions};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DebugMode {
    Vm,
    Jit,
    Aot,
}
impl DebugMode {
    pub fn as_str(&self) -> &str {
        match self {
            DebugMode::Vm => "VM",
            DebugMode::Jit => "JIT",
            DebugMode::Aot => "AOT",
        }
    }
}

#[derive(Debug, Clone)]
pub enum BreakpointTarget {
    Line { line: usize },
    Function { name: String },
    JitFunc { func_idx: usize },
    AotSymbol { symbol: String },
}
impl BreakpointTarget {
    pub fn describe(&self) -> String {
        match self {
            BreakpointTarget::Line { line } => format!("line {}", line),
            BreakpointTarget::Function { name } => format!("func {}", name),
            BreakpointTarget::JitFunc { func_idx } => format!("jit func #{}", func_idx),
            BreakpointTarget::AotSymbol { symbol } => format!("symbol {}", symbol),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StopReason {
    Breakpoint { bp_id: usize, description: String },
    Step { mode: StepMode },
    Completion { result: Value },
    Error { message: String, at_func: Option<String> },
    JitCompiled { func_name: String, success: bool },
    AotCompiled { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepMode {
    Off,
    In,
    Over,
    Out,
}
impl StepMode {
    pub fn as_str(&self) -> &str {
        match self {
            StepMode::Off => "off",
            StepMode::In => "in",
            StepMode::Over => "over",
            StepMode::Out => "out",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FunctionDebugInfo {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub func_idx: usize,
    pub is_native: bool,
}

#[derive(Debug, Clone)]
pub struct SourceMapping {
    pub functions: Vec<FunctionDebugInfo>,
    pub line_to_func: HashMap<usize, usize>,
    /// Phase 4: 指令级行号映射（函数索引 → [(instr_idx, line)]）
    pub instr_line_map: HashMap<usize, Vec<(usize, usize)>>,
}

impl SourceMapping {
    pub fn from_module(module: &BytecodeModule) -> Self {
        let mut functions = Vec::new();
        let mut line_to_func: HashMap<usize, usize> = HashMap::new();
        let mut instr_line_map: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for (idx, func) in module.functions.iter().enumerate() {
            functions.push(FunctionDebugInfo {
                name: func.name.clone(),
                start_line: 0,
                end_line: 0,
                func_idx: idx,
                is_native: func.is_native,
            });
            if idx == 0 {
                line_to_func.insert(0, 0);
            }
            // Phase 4: 使用 line_table 建立指令级行号映射
            if let Some(ref lt) = func.line_table {
                instr_line_map.insert(idx, lt.clone());
                for &(instr_idx, line) in lt {
                    if line > 0 {
                        line_to_func.insert(line, idx);
                    }
                }
            }
        }
        SourceMapping {
            functions,
            line_to_func,
            instr_line_map,
        }
    }
    pub fn func_for_line(&self, line: usize) -> Option<&FunctionDebugInfo> {
        self.line_to_func.get(&line).and_then(|idx| self.functions.get(*idx))
    }
    pub fn func_idx_by_name(&self, name: &str) -> Option<usize> {
        self.functions
            .iter()
            .find(|f| f.name == name || f.name.ends_with(&format!(".{}", name)))
            .map(|f| f.func_idx)
    }
    pub fn info_by_name(&self, name: &str) -> Option<&FunctionDebugInfo> {
        self.functions.iter().find(|f| f.name == name || f.name.ends_with(&format!(".{}", name)))
    }
    /// Phase 4: 查找行号对应的函数和指令索引
    pub fn line_to_instr(&self, line: usize) -> Option<(usize, usize)> {
        if let Some(idx) = self.line_to_func.get(&line) {
            if let Some(ref entries) = self.instr_line_map.get(idx) {
                // 找到该行对应的最小指令索引
                let instr = entries.iter().filter(|e| e.1 == line).map(|e| e.0).min();
                if let Some(instr) = instr {
                    return Some((*idx, instr));
                }
            }
            return Some((*idx, 0));
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub id: usize,
    pub target: BreakpointTarget,
    pub resolved_func_idx: Option<usize>,
    pub resolved_instr_idx: Option<usize>,
    pub enabled: bool,
    pub hit_count: u32,
    pub condition: Option<String>,
    pub mode: DebugMode,
}
impl Breakpoint {
    pub fn describe(&self) -> String {
        let mut s = format!("#{}", self.id);
        s.push_str(&format!(" {}", self.target.describe()));
        if let Some(idx) = self.resolved_func_idx {
            s.push_str(&format!(" -> func #{}", idx));
        }
        if let Some(instr) = self.resolved_instr_idx {
            s.push_str(&format!(", instr {}", instr));
        }
        if !self.enabled {
            s.push_str(" [disabled]");
        }
        s
    }
}

#[derive(Debug, Clone, Default)]
pub struct JitDebugInfo {
    pub func_states: Vec<JitFuncState>,
    pub total_compile_count: u32,
    pub fallback_count: u32,
}
#[derive(Debug, Clone)]
pub struct JitFuncState {
    pub func_idx: usize,
    pub func_name: String,
    pub is_compiled: bool,
    pub is_skipped: bool,
    pub skip_reason: Option<String>,
    pub call_count: u64,
}

#[derive(Debug, Clone)]
pub struct AotDebugInfo {
    pub ll_path: Option<PathBuf>,
    pub exe_path: Option<PathBuf>,
    pub dwarf_functions: Vec<DwarfFunctionInfo>,
    pub external_debugger: Option<String>,
    pub supports_breakpoints: bool,
}
#[derive(Debug, Clone)]
pub struct DwarfFunctionInfo {
    pub name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub llvm_name: String,
}

pub struct DebugSession {
    pub mode: DebugMode,
    pub vm: Option<Vm>,
    pub module: BytecodeModule,
    pub source: String,
    pub file_name: String,
    pub source_map: SourceMap,
    pub file_id: FileId,
    pub mapping: SourceMapping,
    pub breakpoints: Vec<Breakpoint>,
    bp_counter: usize,
    pub step_mode: StepMode,
    step_out_depth: usize,
    pub paused: bool,
    pub stop_reason: Option<StopReason>,
    pub step_counter: u64,
    pub jit_info: Option<JitDebugInfo>,
    pub aot_info: Option<AotDebugInfo>,
}

impl DebugSession {
    pub fn new(module: BytecodeModule, source: &str, file_name: &str) -> Self {
        let mut source_map = SourceMap::new();
        let file_id = source_map.add_file(file_name, source);
        let mapping = SourceMapping::from_module(&module);
        let opts = VmOptions::default();
        let vm = Vm::new(&module, opts).expect("VM init failed");
        DebugSession {
            mode: DebugMode::Vm,
            vm: Some(vm),
            module,
            source: source.to_string(),
            file_name: file_name.to_string(),
            source_map,
            file_id,
            mapping,
            breakpoints: Vec::new(),
            bp_counter: 0,
            step_mode: StepMode::Off,
            step_out_depth: 0,
            paused: false,
            stop_reason: None,
            step_counter: 0,
            jit_info: None,
            aot_info: None,
        }
    }
    pub fn new_jit(module: BytecodeModule, source: &str, file_name: &str) -> Self {
        let mut source_map = SourceMap::new();
        let file_id = source_map.add_file(file_name, source);
        let mapping = SourceMapping::from_module(&module);
        let opts = VmOptions {
            jit: true,
            ..Default::default()
        };
        let vm = Vm::new(&module, opts).expect("VM init failed");
        DebugSession {
            mode: DebugMode::Jit,
            vm: Some(vm),
            module,
            source: source.to_string(),
            file_name: file_name.to_string(),
            source_map,
            file_id,
            mapping,
            breakpoints: Vec::new(),
            bp_counter: 0,
            step_mode: StepMode::Off,
            step_out_depth: 0,
            paused: false,
            stop_reason: None,
            step_counter: 0,
            jit_info: Some(JitDebugInfo::default()),
            aot_info: None,
        }
    }
    pub fn initialize(&mut self) -> Result<(), VmError> {
        let vm = self.vm.as_mut().ok_or_else(|| VmError::Load("no VM".to_string()))?;
        vm.debug_setup();
        vm.debug_push_entry()
    }
    pub fn cleanup(&mut self) {
        if let Some(vm) = self.vm.as_mut() {
            vm.debug_cleanup();
        }
    }
    pub fn run(&mut self) -> Result<StopReason, VmError> {
        loop {
            if let Some(reason) = self.check_breakpoints() {
                self.paused = true;
                self.stop_reason = Some(reason.clone());
                if let StopReason::Breakpoint { bp_id, .. } = &reason {
                    if let Some(bp) = self.breakpoints.iter_mut().find(|b| b.id == *bp_id) {
                        bp.hit_count += 1;
                    }
                }
                return Ok(reason);
            }
            self.vm.as_mut().ok_or_else(|| VmError::Load("no VM".to_string()))?.debug_step()?;
            self.step_counter += 1;
            if self.vm.as_ref().ok_or_else(|| VmError::Load("no VM".to_string()))?.is_halt() {
                let result = self
                    .vm
                    .as_ref()
                    .ok_or_else(|| VmError::Load("no VM".to_string()))?
                    .result()
                    .unwrap_or(Value::Null);
                self.paused = true;
                let reason = StopReason::Completion {
                    result: result.clone(),
                };
                self.stop_reason = Some(reason.clone());
                return Ok(reason);
            }
            if let Some(reason) = self.check_step_mode() {
                self.paused = true;
                self.stop_reason = Some(reason.clone());
                return Ok(reason);
            }
        }
    }
    fn check_breakpoints(&mut self) -> Option<StopReason> {
        let vm = self.vm.as_mut()?;
        let frames = vm.frames_mut();
        let top = frames.last()?;
        for bp in &self.breakpoints {
            if !bp.enabled || bp.mode != self.mode {
                continue;
            }
            if let Some(func_idx) = bp.resolved_func_idx {
                if let Some(instr_idx) = bp.resolved_instr_idx {
                    if top.func == func_idx && top.ip == instr_idx {
                        return Some(StopReason::Breakpoint {
                            bp_id: bp.id,
                            description: bp.describe(),
                        });
                    }
                }
            }
        }
        None
    }
    fn check_step_mode(&mut self) -> Option<StopReason> {
        let vm = self.vm.as_ref()?;
        let depth = vm.depth();
        match self.step_mode {
            StepMode::Off => None,
            StepMode::In => {
                self.step_mode = StepMode::Off;
                Some(StopReason::Step {
                    mode: StepMode::In,
                })
            }
            StepMode::Over => {
                if depth <= self.step_out_depth {
                    self.step_mode = StepMode::Off;
                    Some(StopReason::Step {
                        mode: StepMode::Over,
                    })
                } else {
                    None
                }
            }
            StepMode::Out => {
                if depth < self.step_out_depth {
                    self.step_mode = StepMode::Off;
                    Some(StopReason::Step {
                        mode: StepMode::Out,
                    })
                } else {
                    None
                }
            }
        }
    }
    pub fn continue_execution(&mut self) {
        self.step_mode = StepMode::Off;
        self.paused = false;
        self.stop_reason = None;
    }
    pub fn step_in(&mut self) {
        self.step_mode = StepMode::In;
        self.paused = false;
        self.stop_reason = None;
    }
    pub fn step_over(&mut self) {
        self.step_mode = StepMode::Over;
        if let Some(vm) = self.vm.as_ref() {
            self.step_out_depth = vm.depth();
        }
        self.paused = false;
        self.stop_reason = None;
    }
    pub fn step_out(&mut self) {
        self.step_mode = StepMode::Out;
        if let Some(vm) = self.vm.as_ref() {
            self.step_out_depth = vm.depth().max(1);
        }
        self.paused = false;
        self.stop_reason = None;
    }
    pub fn set_breakpoint(&mut self, target: BreakpointTarget) -> Result<usize, String> {
        let (func_idx, instr_idx) = self.resolve_breakpoint(&target)?;
        self.bp_counter += 1;
        let id = self.bp_counter;
        self.breakpoints.push(Breakpoint {
            id,
            target: target.clone(),
            resolved_func_idx: Some(func_idx),
            resolved_instr_idx: Some(instr_idx),
            enabled: true,
            hit_count: 0,
            condition: None,
            mode: self.mode,
        });
        Ok(id)
    }
    fn resolve_breakpoint(&self, target: &BreakpointTarget) -> Result<(usize, usize), String> {
        match target {
            BreakpointTarget::Function { name } => match self.mapping.func_idx_by_name(name) {
                Some(idx) => Ok((idx, 0)),
                None => Err(format!("function '{}' not found", name)),
            },
            BreakpointTarget::Line { line } => {
                // Phase 4: 优先使用指令级行号映射
                if let Some((func_idx, instr_idx)) = self.mapping.line_to_instr(*line) {
                    return Ok((func_idx, instr_idx));
                }
                match self.mapping.func_for_line(*line) {
                    Some(info) => Ok((info.func_idx, 0)),
                    None => {
                        let mut best: Option<(usize, usize)> = None;
                        for func in &self.mapping.functions {
                            if func.start_line <= *line && *line <= func.end_line {
                                best = Some((func.func_idx, func.start_line));
                            } else if *line > func.start_line && func.start_line > 0 {
                                if best.is_none() || func.start_line > best.unwrap().1 {
                                    best = Some((func.func_idx, func.start_line));
                                }
                            }
                        }
                        match best {
                            Some((idx, ..)) => Ok((idx, 0)),
                            None => Err(format!("line {} not in any function", line)),
                        }
                    }
                }
            }
            BreakpointTarget::JitFunc { func_idx } => Ok((*func_idx, 0)),
            BreakpointTarget::AotSymbol { symbol } => {
                Err(format!("AOT mode: use external debugger for '{}'", symbol))
            }
        }
    }
    pub fn delete_breakpoint(&mut self, id: usize) -> bool {
        let len_before = self.breakpoints.len();
        self.breakpoints.retain(|bp| bp.id != id);
        self.breakpoints.len() < len_before
    }
    pub fn list_breakpoints(&self) -> String {
        if self.breakpoints.is_empty() {
            return "  (no breakpoints)".to_string();
        }
        let mut out = String::new();
        for bp in &self.breakpoints {
            let status = if bp.enabled { "*" } else { "o" };
            out.push_str(&format!("  {} {}\n", status, bp.describe()));
        }
        out
    }
    pub fn current_function(&self) -> Option<&str> {
        if let Some(vm) = self.vm.as_ref() {
            if let Some(top) = vm.frames().last() {
                vm.module_ref().funcs.get(top.func).map(|f| f.name.as_str())
            } else {
                None
            }
        } else {
            None
        }
    }
    pub fn current_instr(&self) -> usize {
        if let Some(vm) = self.vm.as_ref() {
            if let Some(top) = vm.frames().last() { top.ip } else { 0 }
        } else {
            0
        }
    }
    pub fn current_func_idx(&self) -> Option<usize> {
        if let Some(vm) = self.vm.as_ref() {
            if let Some(top) = vm.frames().last() { Some(top.func) } else { None }
        } else {
            None
        }
    }
    pub fn current_code_len(&self) -> usize {
        if let Some(vm) = self.vm.as_ref() {
            if let Some(top) = vm.frames().last() {
                vm.module_ref().funcs.get(top.func).map_or(0, |f| f.code.len())
            } else {
                0
            }
        } else {
            0
        }
    }
    pub fn module_ref(&self) -> Option<&LoadedModule> {
        self.vm.as_ref().map(|vm| vm.module_ref())
    }
    pub fn show_locals(&self, depth: Option<usize>) -> String {
        let vm = match self.vm.as_ref() {
            Some(v) => v,
            None => return "  (VM unavailable)".to_string(),
        };
        let frames = vm.frames();
        if frames.is_empty() {
            return "  (no call frames)".to_string();
        }
        let target = depth.unwrap_or(frames.len() - 1);
        if target >= frames.len() {
            return format!("  (frame depth {} out of range)", target + 1);
        }
        let frame = &frames[target];
        let func = &vm.module_ref().funcs[frame.func];
        let param_count = func.param_count as usize;
        let mut out = String::new();
        out.push_str(&format!(
            "  [{}] {} (frame #{} / {})\n",
            target + 1,
            func.name,
            frames.len() - target,
            frames.len()
        ));
        for i in 0..param_count {
            if i < frame.locals.len() {
                out.push_str(&format!(
                    "    {:>12} = {}\n",
                    format!("arg{}", i),
                    format_value(&frame.locals[i])
                ));
            }
        }
        if param_count < frame.locals.len() {
            out.push_str("    -- locals --\n");
            for i in param_count..frame.locals.len() {
                out.push_str(&format!(
                    "    slot {} = {}\n",
                    i,
                    format_value(&frame.locals[i])
                ));
            }
        }
        out
    }
    pub fn show_stack(&self, depth: Option<usize>) -> String {
        let vm = match self.vm.as_ref() {
            Some(v) => v,
            None => return "  (VM unavailable)".to_string(),
        };
        let frames = vm.frames();
        if frames.is_empty() {
            return "  (no call frames)".to_string();
        }
        let target = depth.unwrap_or(frames.len() - 1);
        if target >= frames.len() {
            return format!("  (frame depth {} out of range)", target + 1);
        }
        let frame = &frames[target];
        if frame.stack.is_empty() {
            return "  (stack empty)".to_string();
        }
        let mut out = String::new();
        out.push_str(&format!(
            "  [frame #{}] stack ({})\n",
            target + 1,
            frame.stack.len()
        ));
        for (i, val) in frame.stack.iter().rev().enumerate() {
            out.push_str(&format!("    {:>3}  {}\n", i, format_value(val)));
        }
        out
    }
    pub fn show_backtrace(&self) -> String {
        let vm = match self.vm.as_ref() {
            Some(v) => v,
            None => return "  (VM unavailable)".to_string(),
        };
        let frames = vm.frames();
        if frames.is_empty() {
            return "  (no call frames)".to_string();
        }
        let mut out = String::new();
        for (i, frame) in frames.iter().enumerate() {
            let func = &vm.module_ref().funcs[frame.func];
            let func_name = func.name.as_str();
            let line_info = if let Some(info) = self.mapping.functions.get(frame.func) {
                if info.start_line > 0 { format!(":{}", info.start_line) } else { String::new() }
            } else {
                String::new()
            };
            let frame_display = if i == frames.len() - 1 {
                format!("#{} <- {}", i + 1, func_name)
            } else {
                format!("#{}  {}", i + 1, func_name)
            };
            out.push_str(&format!(
                "  {}{}  (instr {}/{}  depth {})\n",
                frame_display,
                line_info,
                frame.ip,
                func.code.len(),
                frames.len() - i
            ));
        }
        out
    }
    pub fn show_source(&self, center_line: usize, context: usize) -> String {
        let file = self.source_map.file(self.file_id);
        let total_lines = file.line_count();
        let start = center_line.saturating_sub(context).max(1);
        let end = (center_line + context).min(total_lines);
        let gutter = end.to_string().len().max(1);
        let mut out = String::new();
        for line in start..=end {
            let text = file.line(line).unwrap_or("");
            let marker = if line == center_line { ">" } else { " " };
            out.push_str(&format!(
                "  {:>width$} {}| {}\n",
                line,
                marker,
                text,
                width = gutter
            ));
        }
        if center_line == 0 || center_line > total_lines {
            out.push_str(&format!(
                "  (line {} out of range 1-{})\n",
                center_line, total_lines
            ));
        }
        out
    }
    pub fn show_current_instr(&self) -> String {
        let vm = match self.vm.as_ref() {
            Some(v) => v,
            None => return "  (VM unavailable)".to_string(),
        };
        let frames = vm.frames();
        if frames.is_empty() {
            return "  (no call frames)".to_string();
        }
        let frame = frames.last().unwrap();
        let func = &vm.module_ref().funcs[frame.func];
        if frame.ip >= func.code.len() {
            return format!("  {}  ->  (done, ip={})", func.name, frame.ip);
        }
        format!(
            "  {}  [instr {} / {}]  {}",
            func.name,
            frame.ip,
            func.code.len(),
            format_instr(&func.code[frame.ip])
        )
    }
    pub fn show_info(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("  Debugger status:\n"));
        out.push_str(&format!("    Mode: {}\n", self.mode.as_str()));
        if let Some(vm) = self.vm.as_ref() {
            out.push_str(&format!("    Call depth: {}\n", vm.depth()));
            out.push_str(&format!("    Live objects: {}\n", vm.live_objects()));
            out.push_str(&format!("    Executed instrs: {}\n", self.step_counter));
            out.push_str(&format!("    Functions: {}\n", vm.module_ref().funcs.len()));
            out.push_str(&format!(
                "    Constants: {}\n",
                vm.module_ref().consts.len()
            ));
            out.push_str(&format!("    Natives: {}\n", vm.module_ref().natives.len()));
            if let Some(fi) = self.current_func_idx() {
                if let Some(func) = vm.module_ref().funcs.get(fi) {
                    out.push_str(&format!("    Current func: {}\n", func.name));
                    out.push_str(&format!(
                        "    Current instr: {}/{}\n",
                        self.current_instr(),
                        func.code.len()
                    ));
                }
            }
        }
        if self.paused {
            if let Some(ref r) = self.stop_reason {
                out.push_str(&format!("    Stop reason: {:?}\n", r));
            }
        } else {
            out.push_str("    State: running\n");
        }
        out
    }
    pub fn show_functions(&self) -> String {
        let mut out = String::new();
        for info in &self.mapping.functions {
            let nt = if info.is_native { " [native]" } else { "" };
            out.push_str(&format!("  #{}  {}{}\n", info.func_idx, info.name, nt));
        }
        out
    }
    pub fn get_result(&self) -> Value {
        if let Some(vm) = self.vm.as_ref() {
            vm.result().unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    }
    pub fn current_line(&self) -> usize {
        if let Some(fi) = self.current_func_idx() {
            // Phase 4: 优先使用指令级行号映射
            let instr = self.current_instr();
            if let Some(entries) = self.mapping.instr_line_map.get(&fi) {
                if let Some(&(instr_idx, line)) = entries.iter().rev().find(|e| e.0 <= instr) {
                    if line > 0 {
                        return line;
                    }
                }
            }
            if let Some(info) = self.mapping.functions.get(fi) {
                return info.start_line;
            }
        }
        1
    }

    // ═══════════════════════════════════════════════════════════
    // Phase 2: JIT 调试
    // ═══════════════════════════════════════════════════════════

    /// 刷新 JIT 状态信息（从 VM 查询当前 JIT 编译状态）
    pub fn refresh_jit_info(&mut self) {
        if self.mode != DebugMode::Jit {
            return;
        }
        let vm = match self.vm.as_ref() {
            Some(v) => v,
            None => return,
        };
        #[cfg(feature = "jit")]
        let states = vm.jit_state();
        #[cfg(not(feature = "jit"))]
        let states = Vec::new();

        let funcs = &vm.module_ref().funcs;
        let counts = vm.call_counts();
        let mut info = JitDebugInfo::default();
        let mut total_compiled = 0u32;
        let mut total_fallback = 0u32;

        for (idx, (compiled, skipped)) in states.iter().enumerate() {
            let name = funcs.get(idx).map_or("?".to_string(), |f| f.name.clone());
            let call_count = counts.get(idx).copied().unwrap_or(0);
            let is_compiled = *compiled;
            let is_skipped = *skipped;
            let skip_reason =
                if is_skipped { vm.jit_skip_reason(idx).map(|s| s.to_string()) } else { None };

            if is_compiled {
                total_compiled += 1;
            }
            if is_skipped {
                total_fallback += 1;
            }

            info.func_states.push(JitFuncState {
                func_idx: idx,
                func_name: name,
                is_compiled,
                is_skipped,
                skip_reason,
                call_count,
            });
        }
        info.total_compile_count = total_compiled;
        info.fallback_count = total_fallback;
        self.jit_info = Some(info);
    }

    /// 显示 JIT 编译状态摘要
    pub fn show_jit_state(&self) -> String {
        let jit_info = match &self.jit_info {
            Some(info) => info,
            None => return "  (JIT 信息不可用 — 请使用 --mode jit 启动)".to_string(),
        };

        let mut out = String::new();
        out.push_str(&format!(
            "  JIT 编译状态 (共 {} 函数):\n",
            jit_info.func_states.len()
        ));
        out.push_str(&format!(
            "    已编译: {}  |  已跳过(回退VM): {}\n",
            jit_info.total_compile_count, jit_info.fallback_count
        ));
        out.push_str(&format!(
            "  {:>5}  {:<25}  {:>8}  {:>6}  {:>6}\n",
            "#", "函数", "状态", "调用", "编译"
        ));
        out.push_str(&format!(
            "  {:>5}  {:<25}  {:>8}  {:>6}  {:>6}\n",
            "-----", "-------------------------", "--------", "------", "------"
        ));

        for state in &jit_info.func_states {
            let status = if state.is_compiled {
                "JIT ✓"
            } else if state.is_skipped {
                "VM ✗"
            } else {
                "pending"
            };
            let compiled_mark = if state.is_compiled {
                "YES"
            } else if state.is_skipped {
                "NO"
            } else {
                "-"
            };
            out.push_str(&format!(
                "  {:>5}  {:<25}  {:>8}  {:>6}  {:>6}\n",
                state.func_idx,
                if state.func_name.len() > 25 {
                    format!("{}...", &state.func_name[..22])
                } else {
                    state.func_name.clone()
                },
                status,
                state.call_count,
                compiled_mark
            ));
        }
        out
    }

    /// 显示 JIT 回退详情（仅跳过的函数）
    pub fn show_jit_fallbacks(&self) -> String {
        let jit_info = match &self.jit_info {
            Some(info) => info,
            None => return "  (JIT 信息不可用 — 请使用 --mode jit 启动)".to_string(),
        };

        let skipped: Vec<_> = jit_info.func_states.iter().filter(|s| s.is_skipped).collect();
        if skipped.is_empty() {
            return "  (无回退函数 — 所有 JIT 编译成功)".to_string();
        }

        let mut out = String::new();
        out.push_str(&format!("  JIT 回退详情 ({} 个函数):\n", skipped.len()));
        for state in skipped {
            let reason = state.skip_reason.clone().unwrap_or_else(|| "unknown".to_string());
            out.push_str(&format!(
                "    #{}  {}  (调用 {} 次)\n    原因: {}\n",
                state.func_idx, state.func_name, state.call_count, reason
            ));
        }
        out
    }

    /// 显示已 JIT 编译的函数列表
    pub fn show_jit_compiled(&self) -> String {
        let jit_info = match &self.jit_info {
            Some(info) => info,
            None => return "  (JIT 信息不可用 — 请使用 --mode jit 启动)".to_string(),
        };

        let compiled: Vec<_> = jit_info.func_states.iter().filter(|s| s.is_compiled).collect();
        if compiled.is_empty() {
            return "  (无已编译函数 — JIT 尚未触发)".to_string();
        }

        let mut out = String::new();
        out.push_str(&format!("  已 JIT 编译 ({} 个函数):\n", compiled.len()));
        for state in compiled {
            out.push_str(&format!(
                "    #{}  {}  (调用 {} 次)\n",
                state.func_idx, state.func_name, state.call_count
            ));
        }
        out
    }

    // ═══════════════════════════════════════════════════════════
    // Phase 3: AOT 调试
    // ═══════════════════════════════════════════════════════════

    /// Phase 3: AOT 编译 — 生成 .ll + .exe 并提取 DWARF 函数列表
    #[cfg(feature = "llvm")]
    pub fn aot_compile(&mut self) -> Result<String, String> {
        if self.mode != DebugMode::Aot {
            return Err("请先切换到 AOT 模式 (mode aot)".to_string());
        }

        use std::path::Path;

        // 确定输出目录
        let out_dir = Path::new(&self.file_name).parent().unwrap_or(Path::new("."));
        let out_dir = std::path::PathBuf::from(out_dir);

        // 设置 AOT 选项（带 DWARF 调试信息）
        let opts = crate::codegen::aot::AotOptions {
            debug_info: true,
            opt_level: crate::codegen::aot::OptimizationLevel::None,
            ..Default::default()
        };

        // 确定输出文件名
        let stem = std::path::Path::new(&self.file_name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "debug_output".to_string());
        let exe_ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
        let exe_path = out_dir.join(format!("{}{}", stem, exe_ext));

        // 执行 AOT 编译
        let output = match crate::codegen::aot::aot_compile(&self.source, &exe_path, opts) {
            Ok(output) => output,
            Err(e) => return Err(format!("AOT 编译失败: {}", e)),
        };

        // 解析 .ll 文件提取 DWARF 函数信息
        let mut dwarf_functions = Vec::new();
        if let Some(ref ll_path) = output.ll_path {
            if let Ok(ll_text) = std::fs::read_to_string(ll_path) {
                dwarf_functions = Self::parse_dwarf_functions(&ll_text);
            }
        }

        // 更新 AOT 调试信息
        self.aot_info = Some(AotDebugInfo {
            ll_path: output.ll_path.clone(),
            exe_path: output.exe_path.clone(),
            dwarf_functions: dwarf_functions.clone(),
            external_debugger: None,
            supports_breakpoints: !dwarf_functions.is_empty(),
        });

        let summary = format!(
            "  AOT 编译完成:\n    LLVM IR: {:?}\n    可执行文件: {:?}\n    DWARF 函数: {} 个",
            output.ll_path.as_ref().map(|p| p.to_string_lossy().to_string()),
            output.exe_path.as_ref().map(|p| p.to_string_lossy().to_string()),
            dwarf_functions.len()
        );

        Ok(summary)
    }

    /// 从 LLVM IR 文本解析 DWARF 函数信息
    fn parse_dwarf_functions(ll_text: &str) -> Vec<DwarfFunctionInfo> {
        let mut functions = Vec::new();

        for line in ll_text.lines() {
            if !line.contains("!DISubprogram") {
                continue;
            }
            let trimmed = line.trim();

            // 提取函数名: name: "funcName"
            let name = if let Some(name_start) = trimmed.find("name: \"") {
                let s = name_start + 7;
                if let Some(end) = trimmed[s..].find('"') {
                    trimmed[s..s + end].to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            // 提取行号: line: 42
            let start_line: u32 = if let Some(line_start) = trimmed.find("line: ") {
                let s = line_start + 6;
                let end = trimmed[s..].find(|c: char| !c.is_ascii_digit() && c != ',');
                if let Some(end) = end {
                    trimmed[s..s + end].parse().unwrap_or(0)
                } else {
                    trimmed[s..].trim().trim_end_matches(',').parse().unwrap_or(0)
                }
            } else {
                0
            };

            if !name.is_empty() {
                functions.push(DwarfFunctionInfo {
                    name: name.clone(),
                    start_line,
                    end_line: 0,
                    llvm_name: name,
                });
            }
        }

        functions
    }

    /// 显示 AOT DWARF 元数据摘要
    pub fn show_aot_dwarf(&self) -> String {
        let aot_info = match &self.aot_info {
            Some(info) => info,
            None => return "  (AOT 尚未编译 — 请先执行 aot compile)".to_string(),
        };

        let mut out = String::new();
        out.push_str("  AOT DWARF 调试信息:\n");
        if let Some(ref ll) = aot_info.ll_path {
            out.push_str(&format!("    LLVM IR: {}\n", ll.to_string_lossy()));
        }
        if let Some(ref exe) = aot_info.exe_path {
            out.push_str(&format!("    可执行: {}\n", exe.to_string_lossy()));
        }
        out.push_str(&format!(
            "    支持断点: {}\n",
            if aot_info.supports_breakpoints { "是" } else { "否" }
        ));
        out.push_str(&format!(
            "    DWARF 函数 ({} 个):\n",
            aot_info.dwarf_functions.len()
        ));

        for func in &aot_info.dwarf_functions {
            out.push_str(&format!(
                "      {}  (line {})\n",
                func.name, func.start_line
            ));
        }

        out
    }

    /// 显示 AOT 输出路径
    pub fn show_aot_path(&self) -> String {
        let aot_info = match &self.aot_info {
            Some(info) => info,
            None => return "  (AOT 尚未编译 — 请先执行 aot compile)".to_string(),
        };

        let mut out = String::new();
        if let Some(ref ll) = aot_info.ll_path {
            out.push_str(&format!("  .ll 文件: {}\n", ll.to_string_lossy()));
        }
        if let Some(ref exe) = aot_info.exe_path {
            out.push_str(&format!("  可执行文件: {}\n", exe.to_string_lossy()));
        }
        out
    }

    /// Phase 3: 启动外部调试器 (lldb/gdb)
    pub fn aot_launch(&mut self) -> Result<String, String> {
        let exe_path = {
            let aot_info = self
                .aot_info
                .as_ref()
                .ok_or_else(|| "AOT 尚未编译 — 请先执行 aot compile".to_string())?;
            aot_info.exe_path.as_ref().ok_or_else(|| "无可执行文件".to_string())?.clone()
        };

        // 检测可用调试器
        let debuggers = [
            "lldb", "gdb", "windbg",
        ];
        let mut found_debugger: Option<String> = None;

        for dbg in &debuggers {
            let check_cmd = if cfg!(target_os = "windows") {
                format!("where {}", dbg)
            } else {
                format!("which {}", dbg)
            };
            let shell = if cfg!(target_os = "windows") { "cmd" } else { "sh" };
            let flags = if cfg!(target_os = "windows") {
                vec![
                    "/c", &check_cmd,
                ]
            } else {
                vec![
                    "-c", &check_cmd,
                ]
            };
            match std::process::Command::new(shell).args(flags.as_slice()).output() {
                Ok(output) if output.status.success() => {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        found_debugger = Some(dbg.to_string());
                        break;
                    }
                }
                _ => {}
            }
        }

        match found_debugger {
            Some(debugger) => {
                // 记录调试器名称
                if let Some(ref mut info) = self.aot_info {
                    info.external_debugger = Some(debugger.clone());
                }

                let exe_str = exe_path.to_string_lossy().to_string();
                let mut cmd = std::process::Command::new(&debugger);
                if debugger == "lldb" {
                    cmd.args([
                        "--", &exe_str,
                    ]);
                } else {
                    cmd.arg(&exe_str);
                }

                cmd.spawn().map_err(|e| format!("无法启动 {}: {}", debugger, e))?;

                Ok(format!(
                    "  已启动外部调试器: {}\n    目标: {}\n    提示: 在调试器中使用 b <函数名> 设置断点，c 继续执行",
                    debugger, exe_str
                ))
            }
            None => Err(
                "未找到外部调试器 (lldb/gdb/windbg)\n  请安装 LLVM (包含 lldb) 或 GDB 后重试"
                    .to_string(),
            ),
        }
    }

    // ═══════════════════════════════════════════════════════════
    // 辅助
    // ═══════════════════════════════════════════════════════════

    pub fn step_count(&self) -> u64 {
        self.step_counter
    }
    pub fn short_file_name(&self) -> String {
        std::path::Path::new(&self.file_name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| self.file_name.clone())
    }
}

pub fn format_value(val: &Value) -> String {
    match val {
        Value::Int(i) => format!("{}i", i),
        Value::Float(f) => format!("{}f", f),
        Value::Bool(b) => format!("{}", b),
        Value::Str(s) => format!("\"{}\"", s),
        Value::Null => "null".to_string(),
        Value::Ref(h) => format!("@ref[{}]", h),
        Value::Weak(h) => format!("@weak[{}]", h),
        Value::Ptr(p) => format!("@ptr[{:x}]", p),
        Value::List(items) => {
            let s: Vec<String> = items.iter().map(|v| format_value(v)).collect();
            format!("[{}]", s.join(", "))
        }
        Value::Map(map) => {
            let p: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} => {}", format_value(k), format_value(v)))
                .collect();
            format!("{{{}}}", p.join(", "))
        }
    }
}

pub fn format_instr(instr: &crate::vm::Instr) -> String {
    use crate::vm::Instr::*;
    match instr {
        LoadConst(i) => format!("LoadConst({})", i),
        LoadVar(i) => format!("LoadVar({})", i),
        StoreVar(i) => format!("StoreVar({})", i),
        Add => "Add".into(),
        Sub => "Sub".into(),
        Mul => "Mul".into(),
        Div => "Div".into(),
        Rem => "Rem".into(),
        Neg => "Neg".into(),
        Not => "Not".into(),
        And => "And".into(),
        Or => "Or".into(),
        BitAnd => "BitAnd".into(),
        BitOr => "BitOr".into(),
        BitXor => "BitXor".into(),
        Shl => "Shl".into(),
        Shr => "Shr".into(),
        Eq => "Eq".into(),
        Ne => "Ne".into(),
        Lt => "Lt".into(),
        Gt => "Gt".into(),
        Le => "Le".into(),
        Ge => "Ge".into(),
        Jump(t) => format!("Jump({})", t),
        JumpIfTrue(t) => format!("JumpIfTrue({})", t),
        JumpIfFalse(t) => format!("JumpIfFalse({})", t),
        Call(i) => format!("Call({})", i),
        CallNative(i) => format!("CallNative({})", i),
        CallNativeArgs(i, argc) => format!("CallNativeArgs({}, argc={})", i, argc),
        Return => "Return".into(),
        ReturnUnit => "ReturnUnit".into(),
        NewObject(i) => format!("NewObject({})", i),
        NewArray => "NewArray".into(),
        GetField(i) => format!("GetField({})", i),
        SetField(i) => format!("SetField({})", i),
        GetIndex => "GetIndex".into(),
        SetIndex => "SetIndex".into(),
        IncRef => "IncRef".into(),
        DecRef => "DecRef".into(),
        CallC(i) => format!("CallC({})", i),
        CallMethod(i) => format!("CallMethod({})", i),
        CallCtor(i) => format!("CallCtor({})", i),
        InstanceOf(i) => format!("InstanceOf({})", i),
        CheckCast(i) => format!("CheckCast({})", i),
        NewList => "NewList".into(),
        NewMap => "NewMap".into(),
        ListPush => "ListPush".into(),
        ListPop => "ListPop".into(),
        ListLen => "ListLen".into(),
        MapSet => "MapSet".into(),
        MapGet => "MapGet".into(),
        MapLen => "MapLen".into(),
        Yield => "Yield".into(),
        NewCoroutine(i) => format!("NewCoroutine({})", i),
        ResumeCoroutine => "ResumeCoroutine".into(),
        DropRef => "DropRef".into(),
        Retain => "Retain".into(),
        Release => "Release".into(),
        WeakRef => "WeakRef".into(),
        WeakGet => "WeakGet".into(),
        BoxAlloc => "BoxAlloc".into(),
        DeferBegin => "DeferBegin".into(),
        DeferEnd => "DeferEnd".into(),
        Halt => "Halt".into(),
        CString => "CString".into(),
        ReadCStr => "ReadCStr".into(),
        PtrIsNull => "PtrIsNull".into(),
        PtrToInt => "PtrToInt".into(),
        IntToPtr => "IntToPtr".into(),
        MakeCallback(i) => format!("MakeCallback({})", i),
        MakeClosure(i) => format!("MakeClosure({})", i),
        CallClosure => "CallClosure".into(),
        EnumConstruct(i) => format!("EnumConstruct({})", i),
        EnumTag => "EnumTag".into(),
        MakeFnRef(i) => format!("MakeFnRef({})", i),
        CallExport(i) => format!("CallExport({})", i),
        CallExternal(m, s) => format!("CallExternal({}, {})", m, s),
        CallAot(i) => format!("CallAot({})", i),
        PushHandler(ip, slot) => format!("PushHandler({}, slot={})", ip, slot),
        PopHandler => "PopHandler".into(),
    }
}
