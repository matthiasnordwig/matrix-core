//! Total physical memory of the device (bytes): picks the chat-model tier
//! (`localModelPresets.ts`), blocks the image-model presets on small iPhones
//! (`visionModelPresets.ts`), sizes the context estimate
//! (`discovery::context_window`, on iOS the app budget instead) and the longest
//! input of the search model (`embedding/onnx.rs`). `None` where it cannot be
//! read (other Unix). `sysctlbyname("hw.memsize")` on macOS/iOS, `/proc/meminfo`
//! (`MemTotal`) on Linux and Android, `GlobalMemoryStatusEx` (`ullTotalPhys`) on
//! Windows; the meminfo parser is separate so it is unit-testable.

/// Parse the `MemTotal:` line of `/proc/meminfo` (value in kB) into bytes.
pub fn parse_meminfo_total(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let mut parts = line["MemTotal:".len()..].split_whitespace();
    let value: u64 = parts.next()?.parse().ok()?;
    let factor = match parts.next() {
        Some("kB") | Some("KB") | Some("kb") | None => 1024,
        Some("mB") | Some("MB") => 1024 * 1024,
        Some("B") => 1,
        _ => return None,
    };
    value.checked_mul(factor).filter(|&b| b > 0)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn device_memory_bytes() -> Option<u64> {
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `size`/`len` outlive the call; the name is NUL-terminated.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            &mut size as *mut u64 as *mut libc::c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && size > 0).then_some(size)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn device_memory_bytes() -> Option<u64> {
    parse_meminfo_total(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(windows)]
pub fn device_memory_bytes() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: MEMORYSTATUSEX is plain data; the call requires `dwLength` set to its size.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `status` is a valid, initialised MEMORYSTATUSEX that outlives the call.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0 && status.ullTotalPhys > 0).then_some(status.ullTotalPhys)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "ios", target_os = "linux", target_os = "android")))]
pub fn device_memory_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_memtotal_in_kb() {
        let t = "MemTotal:       16318412 kB\nMemFree:         1234 kB\n";
        assert_eq!(parse_meminfo_total(t), Some(16_318_412 * 1024));
    }

    #[test]
    fn rejects_missing_or_garbled_memtotal() {
        assert_eq!(parse_meminfo_total(""), None);
        assert_eq!(parse_meminfo_total("MemFree: 12 kB\n"), None);
        assert_eq!(parse_meminfo_total("MemTotal: abc kB\n"), None);
        assert_eq!(parse_meminfo_total("MemTotal: 0 kB\n"), None);
        assert_eq!(parse_meminfo_total("MemTotal: 5 parsecs\n"), None);
    }

    #[test]
    fn host_reports_some_memory_where_supported() {
        if cfg!(any(windows, target_os = "macos", target_os = "linux")) {
            assert!(device_memory_bytes().unwrap() > 64 * 1024 * 1024);
        }
    }
}
