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
