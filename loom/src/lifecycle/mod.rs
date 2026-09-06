//! [Phase L2] 生命周期阶段：clean → resolve → compile → test → package → verify → install → deploy

pub mod phases;

/// 生命周期阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Clean,
    Resolve,
    Compile,
    Test,
    Package,
    Verify,
    Install,
    Deploy,
}

impl Phase {
    /// 标准生命周期顺序
    pub fn standard_order() -> Vec<Phase> {
        vec![
            Phase::Clean,
            Phase::Resolve,
            Phase::Compile,
            Phase::Test,
            Phase::Package,
            Phase::Verify,
            Phase::Install,
            Phase::Deploy,
        ]
    }
}
