//! A graphics card the system shows, but the chat engine cannot use: no graphics driver with
//! Vulkan, so llama.cpp (`gguf-vulkan`, the Windows app) loads only its CPU backend and every local
//! model runs on the processor (`super::gguf_verdict`: "no graphics unit"). The app then tells the
//! user once per start, with a link to the maker's driver page (services `commands/health.rs`
//! `gpu_driver_notice`, the app's line `components/ComputeLine.tsx`); it installs nothing itself.
//!
//! [`notice`] decides (pure, tested): the system lists a physical display device ([`Adapter`],
//! from [`system_adapters`]) while llama.cpp reports no graphics unit. The maker comes from the PCI
//! vendor id of the device's hardware id ([`pci_vendor`], [`vendor`]), not from its name — a card
//! without its driver shows as "Microsoft Basic Display Adapter" and only the id still says whose it
//! is. Virtual adapters (Hyper-V, VMware, VirtualBox, QEMU …) and the display chips of server boards
//! (ASPEED, Matrox) never count: no driver would give them Vulkan. A maker not in the list gets the
//! notice without a link ([`driver_page`] `None`).
//!
//! [`system_adapters`] reads the present display devices through SetupAPI (Windows with
//! `gguf-vulkan`); elsewhere it is empty: Apple runs on Metal, and the Linux build has no
//! llama.cpp at all.

use serde::Serialize;

use super::Gpu;

/// Who made a graphics card, as far as the driver link goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

/// A display device the system lists: its description and the PCI vendor id of its hardware id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    pub name: String,
    pub vendor_id: u16,
}

/// What the window shows: the card (`None` where the system names it only generically, without
/// its driver), its maker and the maker's driver page (`None` for a maker without one listed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriverNotice {
    pub gpu: Option<String>,
    pub vendor: Vendor,
    pub driver_url: Option<&'static str>,
}

/// The vendor id in a PCI hardware id (`PCI\VEN_10DE&DEV_2684&SUBSYS_…`); `None` for any other bus
/// (software display drivers, remote sessions) or a malformed id.
pub fn pci_vendor(hardware_id: &str) -> Option<u16> {
    let id = hardware_id.to_ascii_uppercase();
    let rest = id.strip_prefix("PCI\\")?;
    let ven = rest.split('&').find_map(|part| part.strip_prefix("VEN_"))?;
    (ven.len() == 4).then(|| u16::from_str_radix(ven, 16).ok()).flatten()
}

/// The maker of a PCI display device; `None` for adapters no graphics driver gives Vulkan:
/// virtual machines' adapters and server boards' display chips.
pub fn vendor(vendor_id: u16) -> Option<Vendor> {
    match vendor_id {
        0x10DE => Some(Vendor::Nvidia),
        0x1002 | 0x1022 => Some(Vendor::Amd),
        0x8086 => Some(Vendor::Intel),
        // Microsoft (Hyper-V, Basic Render), VMware, VirtualBox, QEMU/Bochs, virtio, Red Hat (QXL),
        // Parallels, Xen; ASPEED and Matrox (server boards).
        0x1414 | 0x15AD | 0x80EE | 0x1234 | 0x1AF4 | 0x1B36 | 0x1AB8 | 0x5853 | 0x1A03 | 0x102B => None,
        _ => Some(Vendor::Other),
    }
}

/// The maker's official driver page (checked to answer 200).
pub fn driver_page(vendor: Vendor) -> Option<&'static str> {
    match vendor {
        Vendor::Nvidia => Some("https://www.nvidia.com/en-us/drivers/"),
        Vendor::Amd => Some("https://www.amd.com/en/support/download/drivers.html"),
        Vendor::Intel => Some("https://www.intel.com/content/www/us/en/support/detect.html"),
        Vendor::Other => None,
    }
}

/// The notice where the system shows a graphics card but llama.cpp found no graphics unit
/// (`backend_gpus` empty); `None` otherwise. With several cards a dedicated maker's goes first
/// (NVIDIA, AMD), then Intel, then any other.
pub fn notice(backend_gpus: &[Gpu], adapters: &[Adapter]) -> Option<DriverNotice> {
    if !backend_gpus.is_empty() {
        return None;
    }
    let (vendor, adapter) = adapters
        .iter()
        .filter_map(|a| vendor(a.vendor_id).map(|v| (v, a)))
        .min_by_key(|(v, _)| *v)?;
    let generic = adapter.name.trim().is_empty() || adapter.name.starts_with("Microsoft ");
    Some(DriverNotice { gpu: (!generic).then(|| adapter.name.trim().to_string()), vendor, driver_url: driver_page(vendor) })
}

/// The display devices present now (Windows with `gguf-vulkan`, SetupAPI); empty elsewhere.
pub fn system_adapters() -> Vec<Adapter> {
    #[cfg(all(windows, feature = "gguf-vulkan"))]
    {
        windows_adapters::list()
    }
    #[cfg(not(all(windows, feature = "gguf-vulkan")))]
    {
        Vec::new()
    }
}

#[cfg(all(windows, feature = "gguf-vulkan"))]
mod windows_adapters {
    use super::{pci_vendor, Adapter};
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW, DIGCF_PRESENT,
        GUID_DEVCLASS_DISPLAY, SETUP_DI_REGISTRY_PROPERTY, SPDRP_DEVICEDESC, SPDRP_HARDWAREID, SP_DEVINFO_DATA,
    };

    /// A device's string property; for the hardware ids (a list of strings) the first one.
    fn property(set: isize, dev: &SP_DEVINFO_DATA, prop: SETUP_DI_REGISTRY_PROPERTY) -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut needed = 0u32;
        // SAFETY: `set`/`dev` come from SetupDiGetClassDevsW/SetupDiEnumDeviceInfo; the buffer's size is passed in bytes.
        let ok = unsafe {
            SetupDiGetDeviceRegistryPropertyW(set, dev, prop, std::ptr::null_mut(), buf.as_mut_ptr().cast(), (buf.len() * 2) as u32, &mut needed)
        };
        if ok == 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    pub fn list() -> Vec<Adapter> {
        // SAFETY: a null enumerator and window are allowed; the set is destroyed below.
        let set = unsafe { SetupDiGetClassDevsW(&GUID_DEVCLASS_DISPLAY, std::ptr::null(), std::ptr::null_mut(), DIGCF_PRESENT) };
        if set == -1 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for index in 0..64 {
            let mut dev: SP_DEVINFO_DATA = unsafe { std::mem::zeroed() };
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            // SAFETY: `dev.cbSize` is set; a false return ends the list.
            if unsafe { SetupDiEnumDeviceInfo(set, index, &mut dev) } == 0 {
                break;
            }
            let Some(vendor_id) = property(set, &dev, SPDRP_HARDWAREID).as_deref().and_then(pci_vendor) else { continue };
            out.push(Adapter { name: property(set, &dev, SPDRP_DEVICEDESC).unwrap_or_default(), vendor_id });
        }
        // SAFETY: `set` is a valid device information set from SetupDiGetClassDevsW.
        unsafe { SetupDiDestroyDeviceInfoList(set) };
        out
    }
}
