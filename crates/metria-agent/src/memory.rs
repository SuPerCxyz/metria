//! Agent 长驻进程的堆内存回收。

/// 尽力把 glibc 已释放的堆页归还操作系统（每轮扫描结束后调用）。
///
/// 非 glibc/Linux 目标为 no-op：malloc_trim 是平台专用优化，不影响功能。
pub fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        let _ = malloc_trim(0);
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn malloc_trim(pad: usize) -> i32;
}
