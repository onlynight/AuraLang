//! 优化 Pipeline 配置
//!
//! 对应 技术方案 §9.4 优化 Pipeline。
//!
//! LLVM 通过 NewPM（New Pass Manager）提供优化 pass。
//! 我们通过指定优化级别，让 `llc` 或 `clang` 调用对应的 pass 集合。
//!
//! - `-O0`：无优化（快速编译，便于调试）
//! - `-O1`：基础优化
//! - `-O2`：标准优化（默认）
//! - `-O3`：激进优化
//! - `-Os`：体积优化
//! - `-Oz`：极致体积优化

/// 优化级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptimizationLevel {
    /// 无优化，最快编译，便于调试
    None,
    /// 基础优化
    Balanced,
    /// 标准优化（默认）
    #[default]
    Aggressive,
    /// 激进优化
    Extreme,
    /// 体积优化
    Size,
    /// 极致体积优化
    SizeExtreme,
}

impl OptimizationLevel {
    /// 转为 LLVM 命令行参数（`-O<N>`）
    pub fn as_llvm_flag(&self) -> &'static str {
        match self {
            OptimizationLevel::None => "-O0",
            OptimizationLevel::Balanced => "-O1",
            OptimizationLevel::Aggressive => "-O2",
            OptimizationLevel::Extreme => "-O3",
            OptimizationLevel::Size => "-Os",
            OptimizationLevel::SizeExtreme => "-Oz",
        }
    }

    /// 从字符串解析
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "0" | "O0" | "none" | "None" => Some(OptimizationLevel::None),
            "1" | "O1" | "balanced" | "Balanced" => Some(OptimizationLevel::Balanced),
            "2" | "O2" | "aggressive" | "Aggressive" | "default" => {
                Some(OptimizationLevel::Aggressive)
            }
            "3" | "O3" | "extreme" | "Extreme" => Some(OptimizationLevel::Extreme),
            "s" | "Os" | "size" | "Size" => Some(OptimizationLevel::Size),
            "z" | "Oz" | "size_extreme" | "SizeExtreme" => Some(OptimizationLevel::SizeExtreme),
            _ => None,
        }
    }

    /// 此级别下是否启用调试友好的 pass
    pub fn debug_friendly(&self) -> bool {
        *self == OptimizationLevel::None || *self == OptimizationLevel::Balanced
    }
}

impl std::fmt::Display for OptimizationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_llvm_flag())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        assert_eq!(OptimizationLevel::default(), OptimizationLevel::Aggressive);
    }

    #[test]
    fn test_flag() {
        assert_eq!(OptimizationLevel::None.as_llvm_flag(), "-O0");
        assert_eq!(OptimizationLevel::Balanced.as_llvm_flag(), "-O1");
        assert_eq!(OptimizationLevel::Aggressive.as_llvm_flag(), "-O2");
        assert_eq!(OptimizationLevel::Extreme.as_llvm_flag(), "-O3");
        assert_eq!(OptimizationLevel::Size.as_llvm_flag(), "-Os");
        assert_eq!(OptimizationLevel::SizeExtreme.as_llvm_flag(), "-Oz");
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            OptimizationLevel::from_str("2"),
            Some(OptimizationLevel::Aggressive)
        );
        assert_eq!(
            OptimizationLevel::from_str("O3"),
            Some(OptimizationLevel::Extreme)
        );
        assert_eq!(
            OptimizationLevel::from_str("Os"),
            Some(OptimizationLevel::Size)
        );
        assert_eq!(
            OptimizationLevel::from_str("Oz"),
            Some(OptimizationLevel::SizeExtreme)
        );
        assert_eq!(OptimizationLevel::from_str("foo"), None);
    }
}
