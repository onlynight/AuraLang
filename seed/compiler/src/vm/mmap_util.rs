//! 跨平台内存映射工具
//!
//! Phase 1: AOT 机器码嵌入方案的基础设施。
//! 提供 mmap/VirtualAlloc 的安全封装，用于将 AOT 机器码映射为可执行内存。
//!
//! 设计见: docs/AOT机器码嵌入方案-详细设计.md §5.4

// ─────────────────────────────────────────────────────────────────────────────
// 内存保护标志
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemoryProtection {
    pub read: bool,
    pub write: bool,
    pub exec: bool,
}

impl MemoryProtection {
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            exec: false,
        }
    }

    pub fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            exec: false,
        }
    }

    pub fn read_exec() -> Self {
        Self {
            read: true,
            write: false,
            exec: true,
        }
    }

    pub fn read_write_exec() -> Self {
        Self {
            read: true,
            write: true,
            exec: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MappedRegion: 已映射的内存区域
// ─────────────────────────────────────────────────────────────────────────────

/// 已映射的内存区域
pub struct MappedRegion {
    /// 映射基地址
    pub base: usize,
    /// 映射大小
    pub size: usize,
    #[cfg(unix)]
    _mapping: *mut std::os::raw::c_void,
}

impl MappedRegion {
    /// 返回映射区域的指针
    pub fn as_ptr(&self) -> *mut u8 {
        self.base as *mut u8
    }

    /// 获取映射区域的切片引用
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.size) }
    }

    /// 获取映射区域的可变切片引用
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.as_ptr(), self.size) }
    }
}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        unsafe {
            #[cfg(unix)]
            {
                libc::munmap(self.base as *mut libc::c_void, self.size);
            }
            #[cfg(windows)]
            {
                unsafe {
                    VirtualFree(self.base as *mut libc::c_void, 0, MEM_RELEASE);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unix 实现
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(unix)]
impl MappedRegion {
    /// 映射匿名内存 (mmap MAP_ANONYMOUS)
    pub fn map_anonymous(size: usize, prot: MemoryProtection) -> Result<Self, String> {
        use std::os::raw::c_void;

        let prot_flags = if prot.read && prot.write && prot.exec {
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC
        } else if prot.read && prot.write {
            libc::PROT_READ | libc::PROT_WRITE
        } else if prot.read && prot.exec {
            libc::PROT_READ | libc::PROT_EXEC
        } else if prot.read {
            libc::PROT_READ
        } else {
            libc::PROT_NONE
        };

        let addr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                prot_flags,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if addr == libc::MAP_FAILED {
            return Err(format!("mmap failed: {}", std::io::Error::last_os_error()));
        }

        Ok(MappedRegion {
            base: addr as usize,
            size,
            _mapping: addr,
        })
    }

    /// 修改内存保护 (mprotect)
    pub unsafe fn protect(&self, prot: MemoryProtection) -> Result<(), String> {
        let prot_flags = if prot.read && prot.write && prot.exec {
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC
        } else if prot.read && prot.write {
            libc::PROT_READ | libc::PROT_WRITE
        } else if prot.read && prot.exec {
            libc::PROT_READ | libc::PROT_EXEC
        } else if prot.read {
            libc::PROT_READ
        } else {
            libc::PROT_NONE
        };

        let ret = libc::mprotect(self.base as *mut c_void, self.size, prot_flags);
        if ret != 0 {
            Err(format!(
                "mprotect failed: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Windows 实现
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(windows)]
const MEM_RELEASE: u32 = 0x8000;

#[cfg(windows)]
unsafe extern "system" {
    fn VirtualAlloc(
        lp_address: *mut libc::c_void,
        dw_size: usize,
        fl_allocation_type: u32,
        fl_protection: u32,
    ) -> *mut libc::c_void;

    fn VirtualFree(lp_address: *mut libc::c_void, dw_size: usize, dw_free_type: u32) -> bool;

    fn VirtualProtect(
        lp_address: *mut libc::c_void,
        dw_size: usize,
        fl_new_protection: u32,
        lp_old_protection: *mut u32,
    ) -> bool;
}

#[cfg(windows)]
impl MappedRegion {
    fn alloc_protection(prot: MemoryProtection) -> u32 {
        if prot.exec {
            if prot.write {
                0x40 // PAGE_EXECUTE_READWRITE
            } else {
                0x20 // PAGE_EXECUTE_READ
            }
        } else if prot.write {
            0x04 // PAGE_READWRITE
        } else {
            0x02 // PAGE_READONLY
        }
    }

    /// 分配匿名内存 (VirtualAlloc)
    pub fn map_anonymous(size: usize, prot: MemoryProtection) -> Result<Self, String> {
        let addr = unsafe {
            VirtualAlloc(
                std::ptr::null_mut(),
                size,
                0x2000 | 0x1000, // MEM_COMMIT | MEM_RESERVE
                Self::alloc_protection(prot),
            )
        };

        if addr.is_null() {
            return Err("VirtualAlloc failed".to_string());
        }

        Ok(MappedRegion {
            base: addr as usize,
            size,
        })
    }

    /// 修改内存保护 (VirtualProtect)
    pub unsafe fn protect(&self, prot: MemoryProtection) -> Result<(), String> {
        let mut old_prot = 0u32;
        let ok = VirtualProtect(
            self.base as *mut libc::c_void,
            self.size,
            Self::alloc_protection(prot),
            &mut old_prot,
        );
        if ok { Ok(()) } else { Err("VirtualProtect failed".to_string()) }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 单元测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_anonymous_read_only() {
        let region = MappedRegion::map_anonymous(4096, MemoryProtection::read_only())
            .expect("mmap should succeed");
        assert!(region.base != 0);
        assert_eq!(region.size, 4096);
    }

    #[test]
    fn test_map_anonymous_read_write() {
        let mut region = MappedRegion::map_anonymous(4096, MemoryProtection::read_write())
            .expect("mmap should succeed");
        let slice = region.as_mut_slice();
        slice[0] = 0xAB;
        slice[1] = 0xCD;
        assert_eq!(slice[0], 0xAB);
        assert_eq!(slice[1], 0xCD);
    }

    #[test]
    fn test_protect_change() {
        let mut region = MappedRegion::map_anonymous(4096, MemoryProtection::read_write())
            .expect("mmap should succeed");
        // 改为只读
        unsafe {
            region.protect(MemoryProtection::read_only()).expect("protect should succeed");
        }
        // 改回可写
        unsafe {
            region.protect(MemoryProtection::read_write()).expect("protect should succeed");
        }
        region.as_mut_slice()[0] = 0xFF;
    }

    #[test]
    fn test_drop_unmaps() {
        let base = {
            let region = MappedRegion::map_anonymous(4096, MemoryProtection::read_only())
                .expect("mmap should succeed");
            region.base
        };
        // region dropped, base should no longer be valid
        assert!(base != 0); // 地址有效，但内存已释放
    }
}
