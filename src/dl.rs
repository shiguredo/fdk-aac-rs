//! プラットフォーム固有の動的ライブラリローダー

use std::ffi::{CString, c_void};
use std::path::Path;

/// 動的ライブラリのハンドル
pub(crate) struct DynLib {
    handle: *mut c_void,
}

impl std::fmt::Debug for DynLib {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynLib").finish_non_exhaustive()
    }
}

unsafe impl Send for DynLib {}
unsafe impl Sync for DynLib {}

impl DynLib {
    /// シンボルを取得して指定の型として返す
    ///
    /// # Safety
    ///
    /// `T` はシンボルが指す関数の正しい型でなければならない
    pub(crate) unsafe fn get<T>(&self, symbol: &[u8]) -> Result<T, String> {
        assert_eq!(
            std::mem::size_of::<T>(),
            std::mem::size_of::<*mut c_void>(),
            "T must be pointer-sized"
        );
        let c_symbol =
            CString::new(symbol).map_err(|_| "symbol name contains null byte".to_string())?;
        let ptr = unsafe { self.sym(&c_symbol)? };
        Ok(unsafe { std::mem::transmute_copy(&ptr) })
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::ffi::{CStr, c_char, c_int};
    use std::os::unix::ffi::OsStrExt;

    // Linux では RTLD_LOCAL = 0 なので RTLD_LAZY のみで十分
    const DLOPEN_FLAGS: c_int = 1; // RTLD_LAZY

    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }

    impl DynLib {
        pub(crate) fn open(path: &Path) -> Result<Self, String> {
            let c_path = CString::new(path.as_os_str().as_bytes())
                .map_err(|_| "path contains null byte".to_string())?;
            unsafe {
                dlerror();
                let handle = dlopen(c_path.as_ptr(), DLOPEN_FLAGS);
                if handle.is_null() {
                    let err = dlerror();
                    let msg = if err.is_null() {
                        format!("failed to load library: {}", path.display())
                    } else {
                        CStr::from_ptr(err).to_string_lossy().into_owned()
                    };
                    return Err(msg);
                }
                Ok(DynLib { handle })
            }
        }

        pub(crate) unsafe fn sym(&self, symbol: &CString) -> Result<*mut c_void, String> {
            unsafe {
                dlerror();
                let ptr = dlsym(self.handle, symbol.as_ptr());
                if ptr.is_null() {
                    let err = dlerror();
                    let msg = if err.is_null() {
                        format!("symbol '{}' resolved to null", symbol.to_string_lossy())
                    } else {
                        CStr::from_ptr(err).to_string_lossy().into_owned()
                    };
                    return Err(msg);
                }
                Ok(ptr)
            }
        }
    }

    impl Drop for DynLib {
        fn drop(&mut self) {
            unsafe {
                dlclose(self.handle);
            }
        }
    }
}
