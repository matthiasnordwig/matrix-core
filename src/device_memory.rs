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
//! `local_context.rs`): macOS `host_statistics64` free + file-backed + purgeable pages ([`vm_available`],
//! what Activity Monitor shows as total minus "Memory Used"), iOS `os_proc_available_memory` (the
//! app's own limit), Linux and Android `/proc/meminfo` `MemAvailable`, Windows `ullAvailPhys`. Outside
//! iOS this includes the file cache the system drops when a program needs memory — also the pages of a
//! model file read before, so callers count a model's weights whole whether it is cached or not (a
//! cold and a warm load get the same context). Both are `None` only where nothing can be read (other
//! Unix — the test names every system that must answer). The parsers are separate so they are
//! unit-testable.
//!
//! [`gpu_dedicated_use`] (Windows): dedicated graphics memory in use now on the adapter that uses
//! most, by all programs (`\GPU Adapter Memory(*)\Dedicated Usage`) and by this process
//! (`\GPU Process Memory(pid_<pid>_<adapter>)\Dedicated Usage`), read in one sample. Needed because
//! Vulkan's memory budget on Windows (AMD, measured) does not shrink when another program holds the card:
//! with 7.2 GiB taken by a second model llama.cpp still reported the card as free. [`foreign_gpu_use`]
//! (pure) turns it into what other programs hold and the backend misses — never this process's own
//! memory, also not while the driver still books a model just freed. Elsewhere `None` (Metal and
//! Linux Vulkan budgets see the others; Apple shares main memory).


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

/// macOS: bytes a program could take now, from page counts of `host_statistics64` — free pages
/// (`free_count` includes the speculative read-ahead pages, which are file-backed and counted there,
/// so they are taken out once), file-backed pages (the file cache, active or not: clean pages the
/// system drops under pressure) and purgeable pages. Not inactive pages: anonymous ones among them
/// leave only by compression or swap, and active file cache is missing from them. Measured on a
/// 24 GB Mac with a cold 12.3 GiB model file: free + inactive gave 11.6 GiB (the model got the
/// minimum context), this 14.4 GiB.
pub fn vm_available(free: u64, speculative: u64, file_backed: u64, purgeable: u64, page_size: u64) -> Option<u64> {
    free.saturating_sub(speculative).checked_add(file_backed)?.checked_add(purgeable)?.checked_mul(page_size).filter(|&b| b > 0)
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
    vm_available(pages(stats.free_count), pages(stats.speculative_count), pages(stats.external_page_count), pages(stats.purgeable_count), page as u64)
}

#[cfg(target_os = "ios")]
unsafe extern "C" {
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

/// Dedicated graphics memory in use on one adapter: by every program, and by this process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuUse {
    pub all: u64,
    pub own: u64,
}

/// What other programs hold on a card that a backend reporting `reported_free` of `total` bytes
/// counts as free: the backend's free beyond what the system leaves free (`total − all`), at most
/// what the system books for other programs (`all − own`). The cap keeps this process's memory
/// out of it: a model just freed leaves the backend's count at once, the system counter (process
/// and adapter alike) a moment later or once the driver gives back its pool.
pub fn foreign_gpu_use(reported_free: u64, total: u64, used: GpuUse) -> u64 {
    let missed = reported_free.saturating_sub(total.saturating_sub(used.all));
    missed.min(used.all.saturating_sub(used.own))
}

/// [`GpuUse`] from the instances of the two counters (name, bytes): the adapter that uses most,
/// and the instances of process `pid` on it (`pid_<pid>_<adapter instance>`). `None` without an
/// adapter.
pub fn pick_gpu_use(adapters: &[(String, u64)], processes: &[(String, u64)], pid: u32) -> Option<GpuUse> {
    let (adapter, all) = adapters.iter().max_by_key(|(_, b)| *b)?;
    let prefix = format!("pid_{pid}_");
    let own = processes
        .iter()
        .filter(|(name, _)| name.strip_prefix(&prefix) == Some(adapter.as_str()))
        .map(|(_, b)| *b)
        .sum::<u64>();
    Some(GpuUse { all: *all, own: own.min(*all) })
}

/// Dedicated graphics memory in use now, by all programs and by this process (Windows; see the
/// module comment).
#[cfg(windows)]
pub fn gpu_dedicated_use() -> Option<GpuUse> {
    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_LARGE, PDH_HCOUNTER, PDH_MORE_DATA,
    };
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let adapter_path = wide("\\GPU Adapter Memory(*)\\Dedicated Usage");
    let process_path = wide("\\GPU Process Memory(*)\\Dedicated Usage");
    let mut query = std::ptr::null_mut();
    // SAFETY: out-pointers to locals; the query is closed below on every path.
    if unsafe { windows_sys::Win32::System::Performance::PdhOpenQueryW(std::ptr::null(), 0, &mut query) } != 0 {
        return None;
    }
    // Every instance of one counter as (name, bytes), from the sample collected last.
    let read = |counter: PDH_HCOUNTER| -> Option<Vec<(String, u64)>> {
        let (mut size, mut count) = (0u32, 0u32);
        // SAFETY: size query with a null buffer, as documented.
        if unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut size, &mut count, std::ptr::null_mut()) } != PDH_MORE_DATA {
            return None;
        }
        let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = vec![Default::default(); (size as usize).div_ceil(item)];
        // SAFETY: `buf` holds at least `size` bytes, aligned for the items.
        if unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_LARGE, &mut size, &mut count, buf.as_mut_ptr()) } != 0 {
            return None;
        }
        let items = buf.iter().take(count as usize).map(|i| {
            // SAFETY: `szName` points to a NUL-terminated name inside `buf` (PDH puts the names
            // after the items); PDH_FMT_LARGE fills `largeValue` of every item it returns.
            let name = unsafe {
                let len = (0..).take_while(|&n| *i.szName.add(n) != 0).count();
                String::from_utf16_lossy(std::slice::from_raw_parts(i.szName, len))
            };
            (name, u64::try_from(unsafe { i.FmtValue.Anonymous.largeValue }).unwrap_or(0))
        });
        Some(items.collect())
    };
    let used = (|| {
        let (mut adapter, mut process) = (std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: `query` is open, the paths NUL-terminated UTF-16, the counters locals.
        if unsafe { PdhAddEnglishCounterW(query, adapter_path.as_ptr(), 0, &mut adapter) } != 0 {
            return None;
        }
        // Without the process counter (older Windows) the own share stays unknown: 0.
        // SAFETY: as above.
        let with_process = unsafe { PdhAddEnglishCounterW(query, process_path.as_ptr(), 0, &mut process) } == 0;
        // SAFETY: as above; one sample for both counters.
        if unsafe { PdhCollectQueryData(query) } != 0 {
            return None;
        }
        let processes = if with_process { read(process).unwrap_or_default() } else { Vec::new() };
        pick_gpu_use(&read(adapter)?, &processes, std::process::id())
    })();
    // SAFETY: closes the query opened above.
    unsafe { PdhCloseQuery(query) };
    used
}

#[cfg(not(windows))]
pub fn gpu_dedicated_use() -> Option<GpuUse> {
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
        // free 10 (2 of them speculative, counted among the 30 file-backed) + 30 + 4 purgeable.
        assert_eq!(vm_available(10, 2, 30, 4, 16384), Some(42 * 16384));
        assert_eq!(vm_available(0, 0, 0, 0, 16384), None);
        assert_eq!(vm_available(u64::MAX, 0, 1, 0, 1), None);
    }

    /// The case from the 24 GB Mac (page counts of 16 KiB): a cold model file of 12.3 GiB next to
    /// 13 GiB of other file cache. Free + inactive alone left less than the weights; the file
    /// cache the system drops makes room, as it does once the model is cached (then its own pages
    /// are the file-backed ones).
    #[test]
    fn file_cache_counts_as_available_so_a_cold_model_fits() {
        let cold = vm_available(100_807, 1_789, 840_818, 3_524, 16384).unwrap();
        assert_eq!(cold >> 30, 14);
        assert!(cold > (12.3 * (1u64 << 30) as f64) as u64 + (1 << 30));
        // Free + inactive + speculative + purgeable of the same moment: below weights + buffers.
        let old = (99_018u64 + 653_671 + 1_789 + 3_524) * 16384;
        assert!(old < (12.3 * (1u64 << 30) as f64) as u64 + (1 << 30) + (384 << 20));
    }

    #[test]
    fn graphics_memory_in_use_is_known_on_windows_only() {
        let used = gpu_dedicated_use();
        if cfg!(windows) {
            // The counter exists on every Windows 10/11 with a display driver (the build PC has one).
            let u = used.unwrap();
            assert!(u.own <= u.all);
        } else {
            assert_eq!(used, None, "Metal and Linux Vulkan budgets already count other programs");
        }
    }

    const GIB: u64 = 1 << 30;

    /// The PC's card (24 GiB, Vulkan counts only this process's memory as used).
    #[test]
    fn foreign_use_is_what_other_programs_hold() {
        let total = 24 * GIB;
        // A second model of 7 GiB in another program, nothing of ours: Vulkan says all free.
        assert_eq!(foreign_gpu_use(total, total, GpuUse { all: 8 * GIB, own: 0 }), 8 * GIB);
        // Our model of 21 GiB and 1 GiB of desktop: Vulkan already counts ours; only the desktop is foreign.
        assert_eq!(foreign_gpu_use(3 * GIB, total, GpuUse { all: 22 * GIB, own: 21 * GIB }), GIB);
        // Nothing foreign: never below 0.
        assert_eq!(foreign_gpu_use(4 * GIB, total, GpuUse { all: 20 * GIB, own: 20 * GIB }), 0);
    }

    /// Regression (model switch MoE → 27B, PC): right after our 21 GiB model was freed Vulkan reports
    /// the card free while the system counter still books it to this process. Our own memory must
    /// not count as another program's, or the next model lands on the CPU.
    #[test]
    fn own_memory_just_freed_is_never_foreign() {
        let total = 24 * GIB;
        let stale = GpuUse { all: 22 * GIB, own: 21 * GIB };
        assert_eq!(foreign_gpu_use(total, total, stale), GIB);
        // Without the cap (backend free − system free) it would have been 22 GiB.
        assert_eq!(total - (total - stale.all), 22 * GIB);
    }

    #[test]
    fn own_use_is_read_on_the_adapter_that_uses_most() {
        let card = "luid_0x00000000_0x0000D1B4_phys_0".to_string();
        let igpu = "luid_0x00000000_0x0000E2C1_phys_0".to_string();
        let adapters = vec![(igpu.clone(), GIB / 4), (card.clone(), 22 * GIB)];
        let processes = vec![
            (format!("pid_4711_{card}"), 21 * GIB),
            (format!("pid_4711_{igpu}"), GIB / 8),
            (format!("pid_99_{card}"), GIB),
            // Another pid that starts with ours.
            (format!("pid_47110_{card}"), GIB / 2),
        ];
        assert_eq!(pick_gpu_use(&adapters, &processes, 4711), Some(GpuUse { all: 22 * GIB, own: 21 * GIB }));
        // Unknown own share (no process counter): 0, the old reading.
        assert_eq!(pick_gpu_use(&adapters, &[], 4711), Some(GpuUse { all: 22 * GIB, own: 0 }));
        assert_eq!(pick_gpu_use(&[], &processes, 4711), None);
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
