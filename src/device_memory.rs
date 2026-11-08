//! Physical memory of the device (bytes), total and free now.
//!
//! [`device_memory_bytes`] (total) picks the chat-model tier (`localModelPresets.ts`), blocks the
//! image-model presets on small iPhones (`visionModelPresets.ts`), sizes the "fits this device"
//! check (`discovery::context_window`, on iOS the app budget instead) and the longest input of the
//! search model (`embedding/onnx.rs`): `sysctlbyname("hw.memsize")` on macOS/iOS, `/proc/meminfo`
//! (`MemTotal`) on Linux and Android, `GlobalMemoryStatusEx` (`ullTotalPhys`) on Windows.
//!
//! [`available_bytes`] (free now, what a model loading now could take without pushing other
//! programs out) sizes the context of a local chat model at every load (services
//! `local_context.rs`): macOS `host_statistics64` free + inactive + speculative + purgeable pages
//! (inactive pages are file cache the system drops first, [`vm_available`]), iOS
//! `os_proc_available_memory` (the app's own limit), Linux and Android `/proc/meminfo`
//! `MemAvailable`, Windows `ullAvailPhys`. Both are `None` only where nothing can be read (other
//! Unix — the test names every system that must answer). The parsers are separate so they are
//! unit-testable.
//!
//! [`file_resident_bytes`]: how much of a file is in the page cache now (`mmap` + `mincore`, macOS,
//! iOS, Linux, Android) — a memory-mapped model whose pages are already cached needs no new memory
//! for them. Windows: `None` (callers count the whole file).

/// Parse the `MemTotal:` line of `/proc/meminfo` (value in kB) into bytes.
pub fn parse_meminfo_total(text: &str) -> Option<u64> {
    parse_meminfo(text, "MemTotal:")
}

/// Parse the `MemAvailable:` line of `/proc/meminfo` (value in kB) into bytes.
pub fn parse_meminfo_available(text: &str) -> Option<u64> {
    parse_meminfo(text, "MemAvailable:")
}

fn parse_meminfo(text: &str, key: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with(key))?;
    let mut parts = line[key.len()..].split_whitespace();
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

/// macOS: bytes free now from page counts of `host_statistics64` — free, inactive (file cache
/// dropped first), speculative (read ahead) and purgeable pages.
pub fn vm_available(free: u64, inactive: u64, speculative: u64, purgeable: u64, page_size: u64) -> Option<u64> {
    free.checked_add(inactive)?.checked_add(speculative)?.checked_add(purgeable)?.checked_mul(page_size).filter(|&b| b > 0)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // `mach_host_self`: libc points to the mach2 crate; the call is the same.
pub fn available_bytes() -> Option<u64> {
    // SAFETY: `stats`/`count` outlive the call; `count` is the size of `stats` in `integer_t`s.
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let rc = unsafe {
        libc::host_statistics64(libc::mach_host_self(), libc::HOST_VM_INFO64, &mut stats as *mut _ as libc::host_info64_t, &mut count)
    };
    // SAFETY: plain libSystem query without arguments.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if rc != libc::KERN_SUCCESS || page <= 0 {
        return None;
    }
    let pages = |n: u32| u64::from(n);
    vm_available(pages(stats.free_count), pages(stats.inactive_count), pages(stats.speculative_count), pages(stats.purgeable_count), page as u64)
}

#[cfg(target_os = "ios")]
extern "C" {
    fn os_proc_available_memory() -> usize;
}

#[cfg(target_os = "ios")]
pub fn available_bytes() -> Option<u64> {
    // SAFETY: plain libSystem query without arguments.
    let b = unsafe { os_proc_available_memory() } as u64;
    (b > 0).then_some(b)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn available_bytes() -> Option<u64> {
    parse_meminfo_available(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(windows)]
pub fn available_bytes() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: as in `device_memory_bytes`.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: `status` is a valid, initialised MEMORYSTATUSEX that outlives the call.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0 && status.ullAvailPhys > 0).then_some(status.ullAvailPhys)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "ios", target_os = "linux", target_os = "android")))]
pub fn available_bytes() -> Option<u64> {
    None
}

/// Bytes of the file at `path` that sit in the page cache now; `None` where the system cannot say.
#[cfg(unix)]
pub fn file_resident_bytes(path: &str) -> Option<u64> {
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open(path).ok()?;
    let len = usize::try_from(file.metadata().ok()?.len()).ok()?;
    if len == 0 {
        return Some(0);
    }
    // SAFETY: plain query without arguments.
    let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok().filter(|&p| p > 0)?;
    // SAFETY: a read-only shared mapping of a file we hold open; unmapped below, never dereferenced.
    let addr = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ, libc::MAP_SHARED, file.as_raw_fd(), 0) };
    if addr == libc::MAP_FAILED {
        return None;
    }
    let mut pages = vec![0u8; len.div_ceil(page)];
    // SAFETY: `addr`/`len` is the mapping above; `pages` has one byte per page of it.
    let rc = unsafe { libc::mincore(addr, len, pages.as_mut_ptr() as _) };
    // SAFETY: unmaps exactly the mapping made above.
    unsafe { libc::munmap(addr, len) };
    (rc == 0).then(|| (pages.iter().filter(|&&p| p & 1 != 0).count() * page).min(len) as u64)
}

#[cfg(not(unix))]
pub fn file_resident_bytes(_path: &str) -> Option<u64> {
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
    fn parses_memavailable_and_not_memfree() {
        let t = "MemTotal: 16318412 kB\nMemFree: 1234 kB\nMemAvailable: 8000000 kB\n";
        assert_eq!(parse_meminfo_available(t), Some(8_000_000 * 1024));
        // An old kernel without MemAvailable: unknown, never MemFree in its place.
        assert_eq!(parse_meminfo_available("MemTotal: 1 kB\nMemFree: 1234 kB\n"), None);
    }

    #[test]
    fn vm_pages_add_up_and_overflow_is_unknown() {
        assert_eq!(vm_available(1, 2, 3, 4, 16384), Some(10 * 16384));
        assert_eq!(vm_available(0, 0, 0, 0, 16384), None);
        assert_eq!(vm_available(u64::MAX, 1, 0, 0, 1), None);
    }

    #[test]
    fn a_file_just_read_is_in_the_page_cache() {
        let path = std::env::temp_dir().join(format!("matrix-resident-{}", std::process::id()));
        std::fs::write(&path, vec![7u8; 1 << 20]).unwrap();
        let _ = std::fs::read(&path).unwrap();
        let resident = file_resident_bytes(path.to_str().unwrap());
        std::fs::remove_file(&path).unwrap();
        if cfg!(unix) {
            // Just written and read: in the cache, never more than the file.
            assert!(resident.unwrap() > 0 && resident.unwrap() <= 1 << 20, "{resident:?}");
        } else {
            assert_eq!(resident, None, "Windows: unknown, callers count the whole file");
        }
        assert_eq!(file_resident_bytes("/no/such/file"), None);
    }

    /// Every system the app ships on must answer both queries (no silent "unknown").
    #[test]
    fn host_reports_total_and_free_memory_where_supported() {
        if cfg!(any(windows, target_os = "macos", target_os = "linux")) {
            let total = device_memory_bytes().unwrap();
            assert!(total > 64 * 1024 * 1024);
            let free = available_bytes().unwrap();
            assert!(free > 0 && free <= total, "free {free} of {total}");
        }
    }
}
