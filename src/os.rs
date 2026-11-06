//! The few places where Unix and other platforms (Windows) differ.
//!
//! On Unix, paths and arguments are arbitrary bytes and are kept as such.
//! Elsewhere they are converted through (lossy) UTF-8.

use std::ffi::OsStr;
use std::path::PathBuf;

#[cfg(unix)]
pub fn bytes_of(s: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(not(unix))]
pub fn bytes_of(s: &OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
pub fn path_of(b: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(b))
}

#[cfg(not(unix))]
pub fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(b).into_owned())
}

#[derive(Clone, Copy)]
pub enum Access {
    Exists,
    Read,
    /// Searchable directory.
    Search,
}

/// `access(2)`: checks permissions for the real user, as the loader would.
#[cfg(unix)]
pub fn access(path: &[u8], mode: Access) -> bool {
    extern "C" {
        fn access(path: *const std::ffi::c_char, mode: std::ffi::c_int) -> std::ffi::c_int;
    }
    let mode = match mode {
        Access::Exists => 0,
        Access::Read => 4,
        Access::Search => 1,
    };
    let Ok(path) = std::ffi::CString::new(path) else { return false };
    // SAFETY: `path` is a valid NUL-terminated string for the duration of the call.
    unsafe { access(path.as_ptr(), mode) == 0 }
}

#[cfg(not(unix))]
pub fn access(path: &[u8], mode: Access) -> bool {
    let path = path_of(path);
    match mode {
        Access::Exists => path.exists(),
        Access::Read => std::fs::File::open(&path).is_ok(),
        Access::Search => path.is_dir(),
    }
}
