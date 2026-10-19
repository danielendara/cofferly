//! Shared test-only helpers.

/// Serializes every reader/writer of `COFFERLY_DATA_DIR` so parallel tests
/// cannot overwrite each other's data dir.
static COFFERLY_DATA_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds `COFFERLY_DATA_DIR_LOCK` without changing the variable, for tests
/// that must not overlap `CofferlyApp::new` (it sweeps `cofferly-*` temp files).
pub(crate) fn data_dir_lock() -> std::sync::MutexGuard<'static, ()> {
    COFFERLY_DATA_DIR_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Points `COFFERLY_DATA_DIR` at a temp dir for the life of the guard and
/// restores the previous value on drop, so this test cannot leak state to
/// others even if an assertion panics.
///
/// Construction holds `COFFERLY_DATA_DIR_LOCK` for the guard lifetime.
pub(crate) struct ScopedDataDir {
    previous: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl ScopedDataDir {
    pub(crate) fn set(path: &std::path::Path) -> Self {
        let lock = data_dir_lock();
        let previous = std::env::var("COFFERLY_DATA_DIR").ok();
        // SAFETY: the mutex serializes every reader/writer of this env var.
        unsafe { std::env::set_var("COFFERLY_DATA_DIR", path) };
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for ScopedDataDir {
    fn drop(&mut self) {
        // SAFETY: see `set` above.
        unsafe {
            match &self.previous {
                Some(previous) => std::env::set_var("COFFERLY_DATA_DIR", previous),
                None => std::env::remove_var("COFFERLY_DATA_DIR"),
            }
        }
    }
}
