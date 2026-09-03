//! 目标平台三元组（Target Triple）
//!
//! 对应 技术方案 §9.5.1 TargetTriple。
//!
//! LLVM 三元组格式：`<arch>-<vendor>-<os>-<abi>`，例如：
//! - `x86_64-pc-windows-msvc`
//! - `aarch64-unknown-linux-gnu`
//! - `armv7-unknown-linux-gnueabihf`
//!
//! 本模块提供类型化的三元组抽象，用于配置 LLVM 后端的目标平台。

/// CPU 架构
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86_64,
    X86,
    Aarch64,
    Armv7,
    Riscv64,
    Loongarch64,
}

impl Architecture {
    pub fn as_str(&self) -> &'static str {
        match self {
            Architecture::X86_64 => "x86_64",
            Architecture::X86 => "x86",
            Architecture::Aarch64 => "aarch64",
            Architecture::Armv7 => "armv7",
            Architecture::Riscv64 => "riscv64",
            Architecture::Loongarch64 => "loongarch64",
        }
    }

    /// 从字符串解析
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "x86_64" | "amd64" => Some(Architecture::X86_64),
            "x86" | "i686" | "i386" => Some(Architecture::X86),
            "aarch64" | "arm64" => Some(Architecture::Aarch64),
            "armv7" | "arm" => Some(Architecture::Armv7),
            "riscv64" => Some(Architecture::Riscv64),
            "loongarch64" => Some(Architecture::Loongarch64),
            _ => None,
        }
    }
}

/// 厂商
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Pc,
    Apple,
    Unknown,
}

impl Vendor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Vendor::Pc => "pc",
            Vendor::Apple => "apple",
            Vendor::Unknown => "unknown",
        }
    }
}

/// 操作系统
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingSystem {
    Windows,
    Linux,
    MacOS,
    FreeBsd,
}

impl OperatingSystem {
    pub fn as_str(&self) -> &'static str {
        match self {
            OperatingSystem::Windows => "windows",
            OperatingSystem::Linux => "linux",
            OperatingSystem::MacOS => "darwin",
            OperatingSystem::FreeBsd => "freebsd",
        }
    }

    pub fn as_triple_os(&self) -> &'static str {
        match self {
            OperatingSystem::Windows => "windows",
            OperatingSystem::Linux => "linux",
            OperatingSystem::MacOS => "darwin",
            OperatingSystem::FreeBsd => "freebsd",
        }
    }
}

/// ABI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abi {
    Msvc,
    Gnu,
    Gnueabihf,
    Macabih,
    None,
}

impl Abi {
    pub fn as_str(&self) -> &'static str {
        match self {
            Abi::Msvc => "msvc",
            Abi::Gnu => "gnu",
            Abi::Gnueabihf => "gnueabihf",
            Abi::Macabih => "macabih",
            Abi::None => "",
        }
    }
}

/// 完整的目标三元组
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetTriple {
    pub arch: Architecture,
    pub vendor: Vendor,
    pub os: OperatingSystem,
    pub abi: Abi,
}

impl TargetTriple {
    /// 构造三元组
    pub fn new(
        arch: Architecture,
        vendor: Vendor,
        os: OperatingSystem,
        abi: Abi,
    ) -> Self {
        Self {
            arch,
            vendor,
            os,
            abi,
        }
    }

    /// 转为 LLVM 三元组字符串
    pub fn to_string(&self) -> String {
        let abi_str = self.abi.as_str();
        if abi_str.is_empty() {
            format!("{}-{}-{}", self.arch.as_str(), self.vendor.as_str(), self.os.as_str())
        } else {
            format!(
                "{}-{}-{}-{}",
                self.arch.as_str(),
                self.vendor.as_str(),
                self.os.as_str(),
                abi_str
            )
        }
    }

    /// 从字符串解析（LLVM 格式）
    pub fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() < 3 {
            return None;
        }
        let arch = Architecture::from_str(parts[0])?;
        let vendor = match parts[1] {
            "pc" => Vendor::Pc,
            "apple" => Vendor::Apple,
            _ => Vendor::Unknown,
        };
        let os = match parts[2] {
            "windows" => OperatingSystem::Windows,
            "linux" => OperatingSystem::Linux,
            "darwin" => OperatingSystem::MacOS,
            "freebsd" => OperatingSystem::FreeBsd,
            _ => return None,
        };
        let abi = if parts.len() >= 4 {
            match parts[3] {
                "msvc" => Abi::Msvc,
                "gnu" => Abi::Gnu,
                "gnueabihf" => Abi::Gnueabihf,
                "macabih" => Abi::Macabih,
                _ => Abi::None,
            }
        } else {
            Abi::None
        };
        Some(Self {
            arch,
            vendor,
            os,
            abi,
        })
    }

    /// 默认主机（根据编译目标推断）
    pub fn default() -> Self {
        let arch = if cfg!(target_arch = "aarch64") {
            Architecture::Aarch64
        } else {
            Architecture::X86_64
        };
        let (vendor, os, abi) = if cfg!(target_os = "windows") {
            (Vendor::Pc, OperatingSystem::Windows, Abi::Msvc)
        } else if cfg!(target_os = "macos") {
            (Vendor::Apple, OperatingSystem::MacOS, Abi::None)
        } else {
            (Vendor::Unknown, OperatingSystem::Linux, Abi::Gnu)
        };
        Self { arch, vendor, os, abi }
    }

    /// Windows x86_64 MSVC
    pub fn windows_x86_64() -> Self {
        Self {
            arch: Architecture::X86_64,
            vendor: Vendor::Pc,
            os: OperatingSystem::Windows,
            abi: Abi::Msvc,
        }
    }

    /// Linux aarch64（树莓派 4/5）
    pub fn linux_aarch64() -> Self {
        Self {
            arch: Architecture::Aarch64,
            vendor: Vendor::Unknown,
            os: OperatingSystem::Linux,
            abi: Abi::Gnu,
        }
    }

    /// Linux armv7（树莓派 3 / Zero 2）
    pub fn linux_armv7() -> Self {
        Self {
            arch: Architecture::Armv7,
            vendor: Vendor::Unknown,
            os: OperatingSystem::Linux,
            abi: Abi::Gnueabihf,
        }
    }

    /// macOS aarch64
    pub fn macos_aarch64() -> Self {
        Self {
            arch: Architecture::Aarch64,
            vendor: Vendor::Apple,
            os: OperatingSystem::MacOS,
            abi: Abi::None,
        }
    }

    /// 获取目标文件扩展名
    pub fn object_ext(&self) -> &'static str {
        if self.os == OperatingSystem::Windows {
            "obj"
        } else {
            "o"
        }
    }

    /// 获取可执行文件扩展名
    pub fn exe_ext(&self) -> &'static str {
        if self.os == OperatingSystem::Windows {
            "exe"
        } else {
            ""
        }
    }
}

impl std::fmt::Display for TargetTriple {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

// AOT 专用的别名（为保持语义清晰）
pub type AOTargetTriple = TargetTriple;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_x86_64() {
        let t = TargetTriple::windows_x86_64();
        assert_eq!(t.to_string(), "x86_64-pc-windows-msvc");
    }

    #[test]
    fn test_linux_aarch64() {
        let t = TargetTriple::linux_aarch64();
        assert_eq!(t.to_string(), "aarch64-unknown-linux-gnu");
    }

    #[test]
    fn test_linux_armv7() {
        let t = TargetTriple::linux_armv7();
        assert_eq!(t.to_string(), "armv7-unknown-linux-gnueabihf");
    }

    #[test]
    fn test_from_str() {
        let t = TargetTriple::from_str("x86_64-pc-windows-msvc").unwrap();
        assert_eq!(t.arch, Architecture::X86_64);
        assert_eq!(t.os, OperatingSystem::Windows);
        assert_eq!(t.abi, Abi::Msvc);
    }

    #[test]
    fn test_from_str_linux() {
        let t = TargetTriple::from_str("aarch64-unknown-linux-gnu").unwrap();
        assert_eq!(t.arch, Architecture::Aarch64);
        assert_eq!(t.os, OperatingSystem::Linux);
        assert_eq!(t.abi, Abi::Gnu);
    }

    #[test]
    fn test_from_str_invalid() {
        assert!(TargetTriple::from_str("invalid-target").is_none());
    }
}
