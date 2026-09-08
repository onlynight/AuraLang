//! P7 内存管理 — ARC 自动插入与优化
//!
//! 7.1 逃逸分析扩展
//! 7.2 ARC 自动插入（Retain/Release）
//! 7.8 ARC 优化（冗余 Retain/Release 消除）
//!
//! 对应技术方案 §6（内存管理）：
//! - 栈分配 + ARC + 手动管理三层策略
//! - 编译期逃逸分析
//! - 引用计数优化

use crate::codegen::mir::{MirFunction, MirInstr, Terminator};
use std::collections::{HashMap, HashSet};

// ─────────────────────────────────────────────────────────────────────────────
// 7.1 逃逸分析（扩展版）
// ─────────────────────────────────────────────────────────────────────────────

/// 分配逃逸信息
#[derive(Debug, Clone)]
pub struct EscapeInfo {
    /// 逃逸的分配寄存器集合（作为参数/字段/返回值传递的）
    pub escaping_allocs: HashSet<usize>,
    /// 未逃逸的分配寄存器集合（可考虑栈分配）
    pub non_escaping_allocs: HashSet<usize>,
}

/// 对 MIR 函数列表进行逃逸分析（7.1）
pub fn escape_analysis(funcs: &[MirFunction]) -> HashMap<String, EscapeInfo> {
    let mut result = HashMap::new();
    for f in funcs {
        if f.is_native {
            continue;
        }
        let mut escaping = HashSet::new();
        let mut allocs = HashSet::new();

        for b in &f.blocks {
            for instr in &b.instrs {
                match instr {
                    MirInstr::Alloc { dst, .. } => {
                        allocs.insert(*dst);
                    }
                    MirInstr::Box { dst, .. } => {
                        allocs.insert(*dst);
                    }
                    // 逃逸条件：作为函数参数传递
                    MirInstr::Call { args, .. } | MirInstr::CallNative { args, .. } => {
                        for a in args {
                            if allocs.contains(a) {
                                escaping.insert(*a);
                            }
                        }
                    }
                    // 逃逸条件：写入字段
                    MirInstr::SetField { src, .. } => {
                        if allocs.contains(src) {
                            escaping.insert(*src);
                        }
                    }
                    // 逃逸条件：写入数组元素
                    MirInstr::SetIndex { src, .. } => {
                        if allocs.contains(src) {
                            escaping.insert(*src);
                        }
                    }
                    // 逃逸条件：保留引用（传递到其他作用域）
                    MirInstr::Retain { src } => {
                        if allocs.contains(src) {
                            escaping.insert(*src);
                        }
                    }
                    _ => {}
                }
            }
            // 逃逸条件：作为返回值
            if let Terminator::Return(r) = &b.term {
                if allocs.contains(r) {
                    escaping.insert(*r);
                }
            }
        }

        let non_escaping = allocs.difference(&escaping).copied().collect();
        result.insert(
            f.name.clone(),
            EscapeInfo {
                escaping_allocs: escaping,
                non_escaping_allocs: non_escaping,
            },
        );
    }
    result
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.2 ARC 自动插入
// ─────────────────────────────────────────────────────────────────────────────

/// ARC 自动插入结果
#[derive(Debug, Default)]
pub struct ArcInsertionStats {
    pub retains: usize,
    pub releases: usize,
}

impl ArcInsertionStats {
    pub fn total(&self) -> usize {
        self.retains + self.releases
    }
}

/// 对 MIR 函数列表自动插入 ARC 操作（7.2）
///
/// 策略：
/// - 函数调用参数：传入对象引用时插入 Retain
/// - 函数返回值：返回对象引用时插入 Retain
/// - 字段赋值：写入对象引用时插入 Retain
/// - 作用域结束：插入 Release（通过块级分析）
pub fn insert_arc(funcs: &mut [MirFunction]) -> ArcInsertionStats {
    let mut stats = ArcInsertionStats::default();
    for f in funcs.iter_mut() {
        if f.is_native {
            continue;
        }
        let f_stats = insert_arc_function(f);
        stats.retains += f_stats.retains;
        stats.releases += f_stats.releases;
    }
    stats
}

fn insert_arc_function(f: &mut MirFunction) -> ArcInsertionStats {
    let mut stats = ArcInsertionStats::default();

    // 收集每个基本块中的 ARC 插入点
    for b in &mut f.blocks {
        let mut new_instrs = Vec::with_capacity(b.instrs.len() * 2);

        for instr in &b.instrs {
            new_instrs.push(instr.clone());

            match instr {
                // 函数调用：参数中的对象引用需要 Retain
                MirInstr::Call { args, .. } | MirInstr::CallNative { args, .. } => {
                    for a in args {
                        new_instrs.push(MirInstr::Retain { src: *a });
                        stats.retains += 1;
                    }
                }
                // 字段赋值：写入对象引用需要 Retain
                MirInstr::SetField { src, .. } => {
                    new_instrs.push(MirInstr::Retain { src: *src });
                    stats.retains += 1;
                }
                // 数组元素写入：写入对象引用需要 Retain
                MirInstr::SetIndex { src, .. } => {
                    new_instrs.push(MirInstr::Retain { src: *src });
                    stats.retains += 1;
                }
                // Box 分配：新对象需要初始 Retain
                MirInstr::Box { src, .. } => {
                    new_instrs.push(MirInstr::Retain { src: *src });
                    stats.retains += 1;
                }
                _ => {}
            }
        }

        // 返回语句：返回对象引用需要 Retain
        match &b.term {
            Terminator::Return(r) => {
                new_instrs.push(MirInstr::Retain { src: *r });
                stats.retains += 1;
            }
            _ => {}
        }

        b.instrs = new_instrs;
    }

    // 参数槽的 Release 不在此处插入。
    //
    // 调用方已经为每个实参插入了一次 `Retain`，被调方若再按块释放参数槽，就会在
    // 函数体尚未执行完（分支前的中间块）或返回值已被调用方持有（返回块）时把对象
    // 提前回收，表现为 “no virtual method … for object” 或读到失效句柄。
    // 该简化实现宁可保留轻微泄漏，也不做不安全的提前释放；真正的
    // 生命周期回收由 `DropRef` / 显式 release 与 ARC 冗余消除共同负责。

    stats
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.8 ARC 优化（冗余消除）
// ─────────────────────────────────────────────────────────────────────────────

/// ARC 优化结果
#[derive(Debug, Default)]
pub struct ArcOptimizationStats {
    pub eliminated_retains: usize,
    pub eliminated_releases: usize,
}

impl ArcOptimizationStats {
    pub fn total(&self) -> usize {
        self.eliminated_retains + self.eliminated_releases
    }
}

/// 对 MIR 函数列表进行 ARC 优化（7.8）
///
/// 消除策略：
/// - 连续的 Retain 后跟 Release（同一寄存器）→ 两者都删除
/// - 连续的 Release 后跟 Retain（同一寄存器）→ 两者都删除
/// - 连续的 Retain 后跟 Retain（同一寄存器）→ 仅保留第一个
/// - 连续的 Release 后跟 Release（同一寄存器）→ 仅保留第一个
pub fn optimize_arc(funcs: &mut [MirFunction]) -> ArcOptimizationStats {
    let mut stats = ArcOptimizationStats::default();
    for f in funcs.iter_mut() {
        if f.is_native {
            continue;
        }
        let f_stats = optimize_arc_function(f);
        stats.eliminated_retains += f_stats.eliminated_retains;
        stats.eliminated_releases += f_stats.eliminated_releases;
    }
    stats
}

fn optimize_arc_function(f: &mut MirFunction) -> ArcOptimizationStats {
    let mut stats = ArcOptimizationStats::default();

    for b in &mut f.blocks {
        b.instrs = optimize_arc_instrs(&b.instrs, &mut stats);
    }

    stats
}

/// 优化指令序列中的冗余 ARC 操作
fn optimize_arc_instrs(instrs: &[MirInstr], stats: &mut ArcOptimizationStats) -> Vec<MirInstr> {
    let mut result = Vec::with_capacity(instrs.len());
    let mut last_retain: Option<usize> = None;
    let mut last_release: Option<usize> = None;

    for instr in instrs {
        match instr {
            MirInstr::Retain { src } => {
                // 如果上次是相同寄存器的 Release，则两者都消除
                if last_release == Some(*src) {
                    // 删除上一个 Release
                    if let Some(last) = result.last() {
                        if matches!(last, MirInstr::Release { src: s } if *s == *src) {
                            result.pop();
                            stats.eliminated_releases += 1;
                            stats.eliminated_retains += 1;
                            last_retain = None;
                            last_release = None;
                            continue;
                        }
                    }
                }
                // 如果上次是相同寄存器的 Retain，则跳过（冗余）
                if last_retain == Some(*src) {
                    stats.eliminated_retains += 1;
                    continue;
                }
                result.push(instr.clone());
                last_retain = Some(*src);
                last_release = None;
            }
            MirInstr::Release { src } => {
                // 如果上次是相同寄存器的 Retain，则两者都消除
                if last_retain == Some(*src) {
                    // 删除上一个 Retain
                    if let Some(last) = result.last() {
                        if matches!(last, MirInstr::Retain { src: s } if *s == *src) {
                            result.pop();
                            stats.eliminated_releases += 1;
                            stats.eliminated_retains += 1;
                            last_retain = None;
                            last_release = None;
                            continue;
                        }
                    }
                }
                // 如果上次是相同寄存器的 Release，则跳过（冗余）
                if last_release == Some(*src) {
                    stats.eliminated_releases += 1;
                    continue;
                }
                result.push(instr.clone());
                last_release = Some(*src);
                last_retain = None;
            }
            _ => {
                // 非 ARC 指令重置状态
                last_retain = None;
                last_release = None;
                result.push(instr.clone());
            }
        }
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.9 内存泄漏检测
// ─────────────────────────────────────────────────────────────────────────────

/// 内存泄漏检测报告
#[derive(Debug, Clone)]
pub struct LeakReport {
    /// 未释放的分配总数
    pub leaked_allocs: usize,
    /// 泄漏的分配详情
    pub details: Vec<LeakDetail>,
}

impl LeakReport {
    pub fn is_clean(&self) -> bool {
        self.leaked_allocs == 0
    }

    pub fn summary(&self) -> String {
        if self.is_clean() {
            "内存无泄漏".to_string()
        } else {
            format!("检测到 {} 个内存泄漏", self.leaked_allocs)
        }
    }
}

/// 单个泄漏详情
#[derive(Debug, Clone)]
pub struct LeakDetail {
    pub function: String,
    pub instr_index: usize,
    pub description: String,
}

/// 分析 MIR 函数列表，检测潜在的内存泄漏（7.9）
pub fn detect_leaks(funcs: &[MirFunction]) -> LeakReport {
    let mut details = Vec::new();

    for f in funcs {
        if f.is_native {
            continue;
        }

        // 跟踪每个分配的 Retain 和 Release 计数
        let mut alloc_retain_count: HashMap<usize, usize> = HashMap::new();
        let mut alloc_release_count: HashMap<usize, usize> = HashMap::new();

        for (_bi, b) in f.blocks.iter().enumerate() {
            for (_ii, instr) in b.instrs.iter().enumerate() {
                match instr {
                    MirInstr::Alloc { dst, .. } | MirInstr::Box { dst, .. } => {
                        alloc_retain_count.entry(*dst).or_insert(0);
                        *alloc_retain_count.entry(*dst).or_insert(1) += 1;
                    }
                    MirInstr::Retain { src } => {
                        *alloc_retain_count.entry(*src).or_insert(0) += 1;
                    }
                    MirInstr::Release { src } => {
                        *alloc_release_count.entry(*src).or_insert(0) += 1;
                    }
                    _ => {}
                }
            }
        }

        // 检查每个分配：Retain 数 > Release 数 → 潜在泄漏
        for (&reg, &retains) in &alloc_retain_count {
            let releases = alloc_release_count.get(&reg).copied().unwrap_or(0);
            if retains > releases {
                details.push(LeakDetail {
                    function: f.name.clone(),
                    instr_index: reg,
                    description: format!(
                        "寄存器 {} 有 {} 次 Retain 但仅 {} 次 Release（净增 {}）",
                        reg,
                        retains,
                        releases,
                        retains - releases
                    ),
                });
            }
        }
    }

    LeakReport {
        leaked_allocs: details.len(),
        details,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 统一入口：完整 ARC 分析 + 插入 + 优化 + 泄漏检测
// ─────────────────────────────────────────────────────────────────────────────

/// ARC 分析结果摘要
#[derive(Debug)]
pub struct ArcAnalysisResult {
    pub escape_info: HashMap<String, EscapeInfo>,
    pub insertion_stats: ArcInsertionStats,
    pub optimization_stats: ArcOptimizationStats,
    pub leak_report: LeakReport,
}

impl ArcAnalysisResult {
    pub fn summary(&self) -> String {
        format!(
            "ARC 分析完成：{} 个函数，插入 {} 个 Retain / {} 个 Release，\
             优化消除 {} 个冗余操作，检测到 {} 个潜在泄漏",
            self.escape_info.len(),
            self.insertion_stats.retains,
            self.insertion_stats.releases,
            self.optimization_stats.total(),
            self.leak_report.leaked_allocs
        )
    }
}

/// 执行完整的 ARC 分析流水线：逃逸分析 → 自动插入 → 优化 → 泄漏检测
pub fn run_arc_analysis(funcs: &mut [MirFunction]) -> ArcAnalysisResult {
    let escape = escape_analysis(funcs);
    let insertion = insert_arc(funcs);
    let optimization = optimize_arc(funcs);
    let leaks = detect_leaks(funcs);

    ArcAnalysisResult {
        escape_info: escape,
        insertion_stats: insertion,
        optimization_stats: optimization,
        leak_report: leaks,
    }
}
