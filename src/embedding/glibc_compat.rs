//! glibc ≥ 2.38 compatibility symbols for the statically linked ONNX Runtime
//! (Linux only, feature `onnx`): every Linux binary that links core with
//! ONNX (web server, examples, wheels) gets them through this module.
//!
//! The prebuilt ONNX Runtime 1.28 that `ort` downloads was compiled against
//! glibc 2.38+, where C23 headers redirect `strtol`/`strtoll`/`strtoull` to
//! `__isoc23_*`. The release target (manylinux_2_34 / AlmaLinux 9, glibc 2.34)
//! has no such symbols, so the link fails. They differ from the classic
//! functions only in also accepting a `0b` binary prefix (base 0 or 2), which
//! ONNX Runtime does not rely on — forwarding to the classic functions is
//! exact for every other input. On a newer glibc these definitions simply
//! shadow the library's. The glibc/libstdc++ gate in `scripts/release-web.sh`
//! still checks the final binary for any other too-new symbol.

use std::os::raw::{c_char, c_int, c_long, c_longlong, c_ulonglong};

unsafe extern "C" {
    fn strtol(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_long;
    fn strtoll(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_longlong;
    fn strtoull(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_ulonglong;
}

/// # Safety
/// Same contract as C `strtol`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __isoc23_strtol(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_long {
    unsafe { strtol(s, end, base) }
}

/// # Safety
/// Same contract as C `strtoll`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __isoc23_strtoll(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_longlong {
    unsafe { strtoll(s, end, base) }
}

/// # Safety
/// Same contract as C `strtoull`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __isoc23_strtoull(s: *const c_char, end: *mut *mut c_char, base: c_int) -> c_ulonglong {
    unsafe { strtoull(s, end, base) }
}
