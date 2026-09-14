//! Hub 后台大任务的临时内存回收。

/// 在任务离开作用域时尽力把 glibc 已释放的堆页归还操作系统。
#[derive(Debug)]
pub struct HeapReleaseGuard;

impl Drop for HeapReleaseGuard {
    fn drop(&mut self) {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        unsafe {
            // Debian Hub runtime 使用 glibc；其他目标不编译这段平台专用调用。
            let _ = malloc_trim(0);
        }
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
unsafe extern "C" {
    fn malloc_trim(pad: usize) -> i32;
}
