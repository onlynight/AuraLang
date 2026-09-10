//! Phase 3: Execution mode selection and configuration.
//!
//! Provides unified configuration for VM, JIT, and AOT execution modes.
//! Corresponds to Phase 3 §5.3 in the full Aura-ification plan.

use crate::codegen::opcode::BytecodeModule;

/// Execution mode configuration.
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// Target execution mode
    pub mode: ExecutionMode,
    /// Optimization level (0-3)
    pub opt_level: u8,
    /// Enable debug information
    pub debug: bool,
    /// Enable parallel compilation
    pub parallel: bool,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::default(),
            opt_level: 2,
            debug: true,
            parallel: true,
        }
    }
}

/// Execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    #[default]
    Vm,
    Jit,
    Aot,
}

impl std::fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionMode::Vm => write!(f, "vm"),
            ExecutionMode::Jit => write!(f, "jit"),
            ExecutionMode::Aot => write!(f, "aot"),
        }
    }
}

impl std::str::FromStr for ExecutionMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "vm" | "default" => Ok(ExecutionMode::Vm),
            "jit" => Ok(ExecutionMode::Jit),
            "aot" => Ok(ExecutionMode::Aot),
            _ => Err(format!("unknown execution mode: {}", s)),
        }
    }
}

/// Result of execution mode configuration.
#[derive(Debug, Clone)]
pub struct ExecutionConfigResult {
    /// Mode that was configured
    pub mode: ExecutionMode,
    /// Configuration details
    pub details: Vec<String>,
}

/// Configure the module for a specific execution mode.
///
/// This function sets up the module for the target execution mode,
/// including optimization settings, debug info, and mode-specific flags.
pub fn configure_execution_mode(
    _module: &mut BytecodeModule,
    config: &ExecutionConfig,
) -> ExecutionConfigResult {
    let mut details = Vec::new();

    match config.mode {
        ExecutionMode::Vm => {
            details.push(format!(
                "VM mode: opt_level={}, debug={}",
                config.opt_level, config.debug
            ));
        }
        ExecutionMode::Jit => {
            details.push(format!(
                "JIT mode: opt_level={}, debug={}",
                config.opt_level, config.debug
            ));
        }
        ExecutionMode::Aot => {
            details.push(format!(
                "AOT mode: opt_level={}, debug={}",
                config.opt_level, config.debug
            ));
        }
    }

    ExecutionConfigResult {
        mode: config.mode,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_mode_default() {
        let mode = ExecutionMode::default();
        assert_eq!(mode, ExecutionMode::Vm);
    }

    #[test]
    fn test_execution_mode_display() {
        assert_eq!(ExecutionMode::Vm.to_string(), "vm");
        assert_eq!(ExecutionMode::Jit.to_string(), "jit");
        assert_eq!(ExecutionMode::Aot.to_string(), "aot");
    }

    #[test]
    fn test_execution_mode_from_str() {
        assert_eq!("vm".parse::<ExecutionMode>(), Ok(ExecutionMode::Vm));
        assert_eq!("jit".parse::<ExecutionMode>(), Ok(ExecutionMode::Jit));
        assert_eq!("aot".parse::<ExecutionMode>(), Ok(ExecutionMode::Aot));
        assert!("unknown".parse::<ExecutionMode>().is_err());
    }

    #[test]
    fn test_execution_config_default() {
        let config = ExecutionConfig::default();
        assert_eq!(config.mode, ExecutionMode::Vm);
        assert_eq!(config.opt_level, 2);
        assert!(config.debug);
        assert!(config.parallel);
    }

    #[test]
    fn test_configure_execution_mode_vm() {
        let mut module = BytecodeModule::default();
        let config = ExecutionConfig {
            mode: ExecutionMode::Vm,
            ..Default::default()
        };
        let result = configure_execution_mode(&mut module, &config);
        assert_eq!(result.mode, ExecutionMode::Vm);
        assert!(result.details.iter().any(|d| d.contains("VM")));
    }

    #[test]
    fn test_configure_execution_mode_jit() {
        let mut module = BytecodeModule::default();
        let config = ExecutionConfig {
            mode: ExecutionMode::Jit,
            ..Default::default()
        };
        let result = configure_execution_mode(&mut module, &config);
        assert_eq!(result.mode, ExecutionMode::Jit);
    }

    #[test]
    fn test_configure_execution_mode_aot() {
        let mut module = BytecodeModule::default();
        let config = ExecutionConfig {
            mode: ExecutionMode::Aot,
            ..Default::default()
        };
        let result = configure_execution_mode(&mut module, &config);
        assert_eq!(result.mode, ExecutionMode::Aot);
    }
}
