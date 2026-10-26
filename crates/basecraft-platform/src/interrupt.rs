//! CLI process-lifetime interrupt flag. The handler performs only a lock-free
//! atomic store; job owners perform cancellation and process reaping normally.
use std::sync::atomic::{AtomicBool, Ordering};
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Acquire)
}
#[cfg(target_os = "linux")]
extern "C" fn interrupt(_signal: std::ffi::c_int) {
    INTERRUPTED.store(true, Ordering::Release);
}
#[cfg(target_os = "linux")]
pub fn install() -> std::io::Result<()> {
    unsafe extern "C" {
        fn signal(signal: std::ffi::c_int, handler: usize) -> usize;
    }
    for number in [2, 15] {
        // SAFETY: Linux signal() uses pointer-sized handler values. This is a
        // static C-ABI handler whose only action is an AtomicBool store.
        if unsafe { signal(number, interrupt as *const () as usize) } == usize::MAX {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub fn install() -> std::io::Result<()> {
    Ok(())
}
