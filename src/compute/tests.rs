use super::*;

fn gpu(name: &str, integrated: bool) -> Gpu {
    Gpu { name: name.into(), integrated, memory_total: 8 << 30, driver: "1.0".into() }
}

#[test]
fn rule_without_a_test() {
    assert_eq!(gguf_verdict(true, &[], None), Verdict::Use(Unit::Gpu, "Metal"));
    assert!(matches!(gguf_verdict(false, &[], Some(Fit::All)), Verdict::Use(Unit::Cpu, _)));
    let card = [gpu("Radeon RX 7900 XTX", false)];
    assert!(matches!(gguf_verdict(false, &card, Some(Fit::All)), Verdict::Use(Unit::Gpu, _)));
    assert!(matches!(gguf_verdict(false, &card, None), Verdict::Use(Unit::Gpu, _)));
    assert!(matches!(gguf_verdict(false, &card, Some(Fit::Nothing)), Verdict::Use(Unit::Cpu, _)));
}

#[test]
fn unclear_cases_get_a_test() {
    let card = [gpu("Radeon RX 7900 XTX", false)];
    assert!(matches!(gguf_verdict(false, &card, Some(Fit::Part)), Verdict::Probe(_)));
    let igpu = [gpu("Radeon(TM) Graphics", true)];
    assert!(matches!(gguf_verdict(false, &igpu, Some(Fit::All)), Verdict::Probe(_)));
    // An integrated unit beside a card: llama.cpp uses only the card.
    let both = [gpu("Radeon(TM) Graphics", true), gpu("Radeon RX 7900 XTX", false)];
    assert!(matches!(gguf_verdict(false, &both, Some(Fit::All)), Verdict::Use(Unit::Gpu, _)));
}

#[test]
fn judge_errs_towards_the_cpu() {
    let t = |ms| Some(Timing { ms });
    assert_eq!(judge(&Probe { gpu: None, cpu: t(100.0), agree: true }).0, Unit::Cpu);
    assert_eq!(judge(&Probe { gpu: t(10.0), cpu: t(100.0), agree: false }).0, Unit::Cpu);
    assert_eq!(judge(&Probe { gpu: t(80.0), cpu: t(100.0), agree: true }).0, Unit::Cpu);
    assert_eq!(judge(&Probe { gpu: t(50.0), cpu: t(100.0), agree: true }).0, Unit::Gpu);
    assert_eq!(judge(&Probe { gpu: t(50.0), cpu: None, agree: true }).0, Unit::Gpu);
}

#[test]
fn cpu_stops_once_clearly_slower() {
    // 64 of 256 tokens in 400 ms → about 1600 ms projected; graphics took 500 ms.
    assert!(cpu_clearly_slower(400.0, 64, 256, 500.0));
    assert!(!cpu_clearly_slower(300.0, 64, 256, 500.0));
    assert!(!cpu_clearly_slower(0.0, 0, 256, 500.0));
}

#[test]
fn key_changes_with_device_driver_model_and_build() {
    let a = probe_key("chat", "/m/Qwen3-4B.gguf", 100, &[gpu("iGPU", true)], "0.2.7");
    assert_eq!(a, probe_key("chat", r"C:\m\Qwen3-4B.gguf", 100, &[gpu("iGPU", true)], "0.2.7"));
    let mut newer = gpu("iGPU", true);
    newer.driver = "2.0".into();
    assert_ne!(a, probe_key("chat", "/m/Qwen3-4B.gguf", 100, &[newer], "0.2.7"));
    assert_ne!(a, probe_key("chat", "/m/Qwen3-4B.gguf", 101, &[gpu("iGPU", true)], "0.2.7"));
    assert_ne!(a, probe_key("chat", "/m/Qwen3-4B.gguf", 100, &[gpu("iGPU", true)], "0.2.8"));
}

#[test]
fn dedicated_memory_is_the_largest_card_and_ignores_integrated_units() {
    let mut big = gpu("Radeon RX 7900 XTX", false);
    big.memory_total = 24 << 30;
    let mut igpu = gpu("Radeon Graphics", true);
    igpu.memory_total = 64 << 30;
    assert_eq!(dedicated_memory(&[]), None);
    assert_eq!(dedicated_memory(&[igpu.clone()]), None);
    assert_eq!(dedicated_memory(&[gpu("GeForce", false), big, igpu]), Some(24 << 30));
}

mod driver_notice {
    use super::gpu;
    use crate::compute::driver::*;

    fn adapter(name: &str, hardware_id: &str) -> Adapter {
        Adapter { name: name.into(), vendor_id: pci_vendor(hardware_id).expect("PCI id") }
    }

    #[test]
    fn vendor_comes_from_the_pci_id() {
        assert_eq!(pci_vendor(r"PCI\VEN_10DE&DEV_2684&SUBSYS_16F310DE&REV_A1"), Some(0x10DE));
        assert_eq!(pci_vendor(r"pci\ven_1002&dev_744c"), Some(0x1002));
        assert_eq!(pci_vendor(r"ROOT\BasicDisplay"), None);
        assert_eq!(pci_vendor(r"PCI\VEN_10D&DEV_1"), None);
        assert_eq!(pci_vendor(r"PCI\CC_0300"), None);
        assert_eq!(vendor(0x10DE), Some(Vendor::Nvidia));
        assert_eq!(vendor(0x1002), Some(Vendor::Amd));
        assert_eq!(vendor(0x8086), Some(Vendor::Intel));
        assert_eq!(vendor(0x5143), Some(Vendor::Other));
        for virtual_or_server in [0x1414, 0x15AD, 0x80EE, 0x1234, 0x1AF4, 0x1B36, 0x1A03, 0x102B] {
            assert_eq!(vendor(virtual_or_server), None, "{virtual_or_server:04x}");
        }
    }

    #[test]
    fn known_makers_get_their_driver_page_others_none() {
        for v in [Vendor::Nvidia, Vendor::Amd, Vendor::Intel] {
            assert!(driver_page(v).is_some_and(|u| u.starts_with("https://www.")), "{v:?}");
        }
        assert_eq!(driver_page(Vendor::Other), None);
    }

    #[test]
    fn a_card_without_vulkan_gets_the_notice() {
        let card = [adapter("NVIDIA GeForce RTX 4070", r"PCI\VEN_10DE&DEV_2786")];
        let n = notice(&[], &card).expect("notice");
        assert_eq!(n.gpu.as_deref(), Some("NVIDIA GeForce RTX 4070"));
        assert_eq!(n.vendor, Vendor::Nvidia);
        assert_eq!(n.driver_url, driver_page(Vendor::Nvidia));
        // The card already works through Vulkan: nothing to say.
        assert_eq!(notice(&[gpu("NVIDIA GeForce RTX 4070", false)], &card), None);
    }

    #[test]
    fn a_card_without_any_driver_is_named_by_its_maker() {
        let basic = [adapter("Microsoft Basic Display Adapter", r"PCI\VEN_1002&DEV_744C")];
        let n = notice(&[], &basic).expect("notice");
        assert_eq!((n.gpu, n.vendor), (None, Vendor::Amd));
    }

    #[test]
    fn no_card_virtual_or_server_chips_give_no_notice() {
        assert_eq!(notice(&[], &[]), None);
        let vm = [adapter("Microsoft Hyper-V Video", r"PCI\VEN_1414&DEV_5353"), adapter("ASPEED Graphics Family", r"PCI\VEN_1A03&DEV_2000")];
        assert_eq!(notice(&[], &vm), None);
    }

    #[test]
    fn dedicated_maker_first_unknown_maker_without_link() {
        let both = [adapter("Intel(R) UHD Graphics 770", r"PCI\VEN_8086&DEV_4680"), adapter("AMD Radeon RX 7900 XTX", r"PCI\VEN_1002&DEV_744C")];
        assert_eq!(notice(&[], &both).map(|n| n.vendor), Some(Vendor::Amd));
        let other = [adapter("Some Graphics", r"PCI\VEN_5143&DEV_0001")];
        let n = notice(&[], &other).expect("notice");
        assert_eq!((n.vendor, n.driver_url), (Vendor::Other, None));
    }
}
