; ---- Windows CRT (@native(SYS_*) platform implementations) ----
declare i32 @_write(i32, i8*, i32)
declare i32 @_read(i32, i8*, i32)
declare i32 @_open(i8*, i32, i32)
declare i32 @_close(i32)
declare i64 @_lseeki64(i32, i64, i32)
declare i32 @_fstat(i32, i8*)
declare i32 @_stat64(i8*, i8*)
declare i32 @_access(i8*, i32)
declare i32 @_unlink(i8*)
declare i32 @_mkdir(i8*)
declare i32 @_rmdir(i8*)
declare i32 @_getpid()
; `Process.run(cmd)` 的 Windows 实现（发射器内置降级）
declare i32 @system(i8*)
declare i32 @MoveFileA(i8*, i8*)

; UCRT 不导出 `_rename`（只有 `rename`，但它与本模块内 `@native fun rename`
; 生成的包装器同名 → 会自递归）。这里用 kernel32 的 MoveFileA 提供定义，
; 返回值语义与 C `rename` 对齐：成功 0，失败 -1。
define i32 @_rename(i8* %oldp, i8* %newp) {
entry:
  %r = call i32 @MoveFileA(i8* %oldp, i8* %newp)
  %ok = icmp ne i32 %r, 0
  %rv = select i1 %ok, i32 0, i32 -1
  ret i32 %rv
}
