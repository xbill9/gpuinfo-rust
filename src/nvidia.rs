//! NVIDIA detail through NVML (`libnvidia-ml.so`, loaded at runtime) and the
//! `/proc/driver/nvidia` files. On hosts without the NVIDIA driver, NVML fails
//! to load and only the vendor-neutral sysfs data is reported.

use std::fmt::Display;

use nvml_wrapper::Device;
use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{
    Clock, ClockId, EccCounter, EncoderType, MemoryError, PcieUtilCounter, PerformanceState,
    RetirementCause, TemperatureSensor, TemperatureThreshold,
};
use nvml_wrapper::enums::device::UsedGpuMemory;
use nvml_wrapper::error::NvmlError;

use crate::report::{Report, bytes};

/// NVLink link slots probed per device (NVML_NVLINK_MAX_LINKS is 18).
const MAX_NVLINKS: u32 = 18;

pub fn init() -> Result<Nvml, NvmlError> {
    Nvml::init()
}

/// Errors that only mean "this GPU or driver does not expose that field".
fn unsupported(e: &NvmlError) -> bool {
    matches!(
        e,
        NvmlError::NotSupported
            | NvmlError::FunctionNotFound
            | NvmlError::FailedToLoadSymbol(_)
            | NvmlError::NotFound
            | NvmlError::InvalidArg
    )
}

/// Adds a field; unsupported fields are omitted, other errors are shown.
fn put<T: Display>(r: &mut Report, key: &str, res: Result<T, NvmlError>) {
    match res {
        Ok(v) => {
            let v = v.to_string();
            if !v.trim().is_empty() {
                r.kv(key, v);
            }
        }
        Err(e) if unsupported(&e) => {}
        Err(e) => r.kv(key, format!("n/a ({e:?})")),
    }
}

fn yes_no(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn mw(milliwatts: u32) -> String {
    format!("{:.2} W", milliwatts as f64 / 1000.0)
}

fn pstate(p: PerformanceState) -> String {
    match p.as_c() {
        n if n < 16 => format!("P{n}"),
        _ => "unknown".into(),
    }
}

fn cuda_version(v: i32) -> String {
    format!("{}.{}", v / 1000, (v % 1000) / 10)
}

pub fn render_host(r: &mut Report, nvml: &Result<Nvml, NvmlError>) {
    r.subsection("NVIDIA");
    match nvml {
        Ok(nvml) => {
            put(r, "Driver Version", nvml.sys_driver_version());
            put(r, "NVML Version", nvml.sys_nvml_version());
            put(
                r,
                "CUDA Driver API Version",
                nvml.sys_cuda_driver_version().map(cuda_version),
            );
            put(r, "NVML Device Count", nvml.device_count());
        }
        Err(e) => r.kv("NVML", format!("unavailable ({e})")),
    }
    if let Ok(v) = std::fs::read_to_string("/proc/driver/nvidia/version") {
        for line in v.lines().filter(|l| !l.trim().is_empty()) {
            r.line(line.trim());
        }
    }
}

/// NVML index of the device at the given PCI domain / bus / device.
pub fn index_for_bdf(nvml: &Nvml, bdf: (u32, u32, u32, u32)) -> Option<u32> {
    let count = nvml.device_count().ok()?;
    (0..count).find(|&i| {
        nvml.device_by_index(i)
            .and_then(|d| d.pci_info())
            .is_ok_and(|p| (p.domain, p.bus, p.device) == (bdf.0, bdf.1, bdf.2))
    })
}

pub fn render_proc_information(r: &mut Report, address: &str) {
    let path = format!("/proc/driver/nvidia/gpus/{address}/information");
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    r.subsection("NVIDIA /proc information");
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            r.kv(k.trim(), v.trim());
        }
    }
}

pub fn render_device(r: &mut Report, nvml: &Nvml, index: u32) {
    let d = match nvml.device_by_index(index) {
        Ok(d) => d,
        Err(e) => {
            r.subsection("NVML");
            r.kv("NVML Device", format!("n/a ({e:?})"));
            return;
        }
    };

    identity(r, &d, index);
    memory(r, &d);
    utilization(r, &d);
    clocks(r, nvml, &d);
    performance(r, &d);
    power(r, &d);
    thermal(r, &d);
    pcie(r, &d);
    nvlink(r, &d);
    processes(r, &d);
    r.end_subsection();
}

fn identity(r: &mut Report, d: &Device, index: u32) {
    r.subsection("NVML Identity");
    r.kv("NVML Index", index);
    put(r, "Product Name", d.name());
    put(r, "Brand", d.brand().map(|b| format!("{b:?}")));
    put(r, "Architecture", d.architecture());
    put(
        r,
        "CUDA Compute Capability",
        d.cuda_compute_capability()
            .map(|c| format!("{}.{}", c.major, c.minor)),
    );
    put(r, "CUDA Cores", d.num_cores());
    put(r, "UUID", d.uuid());
    put(r, "Serial Number", d.serial());
    put(r, "Board Part Number", d.board_part_number());
    put(r, "Board ID", d.board_id().map(|b| format!("{b:#x}")));
    put(r, "Multi-GPU Board", d.is_multi_gpu_board().map(yes_no));
    put(
        r,
        "Minor Number",
        d.minor_number().map(|m| format!("{m} (/dev/nvidia{m})")),
    );
    put(r, "VBIOS Version", d.vbios_version());
    put(r, "InfoROM Image Version", d.info_rom_image_version());
    put(r, "GSP Firmware Version", d.gsp_firmware_version());
    put(
        r,
        "GSP Firmware Mode",
        d.gsp_firmware_mode().map(|m| {
            format!(
                "enabled {}, default {}",
                yes_no(m.enabled),
                yes_no(m.default)
            )
        }),
    );
    put(r, "Bus Type", d.bus_type().map(|b| format!("{b:?}")));
    put(r, "IRQ", d.irq_num());
    put(r, "NUMA Node", d.numa_node_id());
    put(
        r,
        "Virtualization Mode",
        d.virtualization_mode().map(|m| format!("{m:?}")),
    );
    put(
        r,
        "MIG Mode",
        d.mig_mode()
            .map(|m| format!("current {}, pending {}", m.current, m.pending)),
    );
}

fn memory(r: &mut Report, d: &Device) {
    r.subsection("Memory");
    match d.memory_info() {
        Ok(m) => {
            r.kv("Total", bytes(m.total));
            r.kv("Used", bytes(m.used));
            r.kv("Free", bytes(m.free));
            r.kv("Reserved", bytes(m.reserved));
        }
        Err(e) => put::<String>(r, "Memory", Err(e)),
    }
    put(
        r,
        "Memory Bus Width",
        d.memory_bus_width().map(|w| format!("{w} bits")),
    );
    match d.bar1_memory_info() {
        Ok(b) => {
            r.kv("BAR1 Total", bytes(b.total));
            r.kv("BAR1 Used", bytes(b.used));
            r.kv("BAR1 Free", bytes(b.free));
        }
        Err(e) => put::<String>(r, "BAR1", Err(e)),
    }
    put(
        r,
        "ECC Mode",
        d.is_ecc_enabled().map(|e| {
            format!(
                "current {}, pending {}",
                yes_no(e.currently_enabled),
                yes_no(e.pending_enabled)
            )
        }),
    );
    for (counter, cname) in [
        (EccCounter::Volatile, "Volatile"),
        (EccCounter::Aggregate, "Aggregate"),
    ] {
        for (err, ename) in [
            (MemoryError::Corrected, "Corrected"),
            (MemoryError::Uncorrected, "Uncorrected"),
        ] {
            put(
                r,
                &format!("ECC {cname} {ename}"),
                d.total_ecc_errors(err, counter),
            );
        }
    }
    for (cause, label) in [
        (
            RetirementCause::MultipleSingleBitEccErrors,
            "Retired Pages (SBE)",
        ),
        (RetirementCause::DoubleBitEccError, "Retired Pages (DBE)"),
    ] {
        put(r, label, d.retired_pages(cause).map(|p| p.len()));
    }
}

fn utilization(r: &mut Report, d: &Device) {
    r.subsection("Utilization");
    match d.utilization_rates() {
        Ok(u) => {
            r.kv("GPU", format!("{} %", u.gpu));
            r.kv("Memory Controller", format!("{} %", u.memory));
        }
        Err(e) => put::<String>(r, "Utilization", Err(e)),
    }
    put(
        r,
        "Encoder",
        d.encoder_utilization()
            .map(|u| format!("{} % (sampling {} us)", u.utilization, u.sampling_period)),
    );
    put(
        r,
        "Decoder",
        d.decoder_utilization()
            .map(|u| format!("{} % (sampling {} us)", u.utilization, u.sampling_period)),
    );
    put(
        r,
        "Encoder Sessions",
        d.encoder_stats().map(|s| {
            format!(
                "{} (avg {} fps, {} us latency)",
                s.session_count, s.average_fps, s.average_latency
            )
        }),
    );
    put(
        r,
        "Encoder Capacity H.264",
        d.encoder_capacity(EncoderType::H264)
            .map(|c| format!("{c} %")),
    );
    put(
        r,
        "Encoder Capacity HEVC",
        d.encoder_capacity(EncoderType::HEVC)
            .map(|c| format!("{c} %")),
    );
    put(
        r,
        "FBC Sessions",
        d.fbc_stats().map(|s| {
            format!(
                "{} (avg {} fps, {} us latency)",
                s.sessions_count, s.average_fps, s.average_latency
            )
        }),
    );
}

/// Supported graphics clocks for one memory clock.
///
/// nvml-wrapper 0.13 returns a buffer of 128 zeros when a GPU has more than
/// 128 graphics clock steps, so NVML is called directly with a larger buffer.
fn graphics_clocks(nvml: &Nvml, d: &Device, mem_clock: u32) -> Result<Vec<u32>, NvmlError> {
    const MAX_STEPS: usize = 1024;
    let sym = nvml
        .lib()
        .nvmlDeviceGetSupportedGraphicsClocks
        .as_ref()
        .map_err(|_| NvmlError::FunctionNotFound)?;
    let mut clocks = vec![0u32; MAX_STEPS];
    let mut count = MAX_STEPS as u32;
    // SAFETY: the handle is valid while `d` is borrowed, and `clocks` holds
    // `count` elements for NVML to fill.
    let code = unsafe { sym(d.handle(), mem_clock, &mut count, clocks.as_mut_ptr()) };
    if code != 0 {
        return Err(NvmlError::from(code));
    }
    clocks.truncate(count as usize);
    Ok(clocks)
}

fn clocks(r: &mut Report, nvml: &Nvml, d: &Device) {
    r.subsection("Clocks (MHz)");
    for (clock, name) in [
        (Clock::Graphics, "Graphics"),
        (Clock::SM, "SM"),
        (Clock::Memory, "Memory"),
        (Clock::Video, "Video"),
    ] {
        put(r, &format!("{name} Current"), d.clock_info(clock));
        put(r, &format!("{name} Max"), d.max_clock_info(clock));
        put(
            r,
            &format!("{name} Max Customer Boost"),
            d.max_customer_boost_clock(clock),
        );
        put(
            r,
            &format!("{name} Application Target"),
            d.clock(clock, ClockId::TargetAppClock),
        );
        put(
            r,
            &format!("{name} Application Default"),
            d.clock(clock, ClockId::DefaultAppClock),
        );
    }
    put(
        r,
        "Auto Boost",
        d.auto_boosted_clocks_enabled().map(|a| {
            format!(
                "enabled {}, default {}",
                yes_no(a.is_enabled),
                yes_no(a.is_enabled_default)
            )
        }),
    );
    match d.supported_memory_clocks() {
        Ok(mem) => {
            r.kv(
                "Supported Memory Clocks",
                mem.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            for m in mem {
                put(
                    r,
                    &format!("Graphics Clocks @ Mem {m}"),
                    graphics_clocks(nvml, d, m).map(|g| {
                        let (min, max) = (g.iter().min(), g.iter().max());
                        match (min, max) {
                            (Some(lo), Some(hi)) => format!("{} steps, {lo}-{hi}", g.len()),
                            _ => "none".into(),
                        }
                    }),
                );
            }
        }
        Err(e) => put::<String>(r, "Supported Memory Clocks", Err(e)),
    }
}

fn performance(r: &mut Report, d: &Device) {
    r.subsection("Performance & Modes");
    put(r, "Performance State", d.performance_state().map(pstate));
    put(
        r,
        "Throttle Reasons",
        d.current_throttle_reasons().map(|t| {
            if t.is_empty() {
                "none".to_string()
            } else {
                t.iter_names()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        }),
    );
    put(
        r,
        "Compute Mode",
        d.compute_mode().map(|m| format!("{m:?}")),
    );
    put(r, "Persistence Mode", d.is_in_persistent_mode().map(yes_no));
    put(r, "Display Connected", d.is_display_connected().map(yes_no));
    put(r, "Display Active", d.is_display_active().map(yes_no));
    put(r, "Accounting Mode", d.is_accounting_enabled().map(yes_no));
}

fn power(r: &mut Report, d: &Device) {
    r.subsection("Power");
    put(r, "Power Draw", d.power_usage().map(mw));
    put(r, "Power Limit", d.power_management_limit().map(mw));
    put(
        r,
        "Default Power Limit",
        d.power_management_limit_default().map(mw),
    );
    put(r, "Enforced Power Limit", d.enforced_power_limit().map(mw));
    put(
        r,
        "Power Limit Range",
        d.power_management_limit_constraints()
            .map(|c| format!("{} - {}", mw(c.min_limit), mw(c.max_limit))),
    );
    put(
        r,
        "Energy Since Driver Load",
        d.total_energy_consumption()
            .map(|mj| format!("{:.3} kJ", mj as f64 / 1_000_000.0)),
    );
    put(
        r,
        "Power Source",
        d.power_source().map(|s| format!("{s:?}")),
    );
}

fn thermal(r: &mut Report, d: &Device) {
    r.subsection("Thermal & Cooling");
    put(
        r,
        "GPU Temperature",
        d.temperature(TemperatureSensor::Gpu)
            .map(|t| format!("{t} °C")),
    );
    for (t, name) in [
        (TemperatureThreshold::Shutdown, "Shutdown"),
        (TemperatureThreshold::Slowdown, "Slowdown"),
        (TemperatureThreshold::GpuMax, "GPU Max Operating"),
        (TemperatureThreshold::MemoryMax, "Memory Max Operating"),
        (TemperatureThreshold::AcousticMin, "Acoustic Min"),
        (TemperatureThreshold::AcousticCurr, "Acoustic Target"),
        (TemperatureThreshold::AcousticMax, "Acoustic Max"),
    ] {
        put(
            r,
            &format!("{name} Threshold"),
            d.temperature_threshold(t).map(|v| format!("{v} °C")),
        );
    }
    match d.num_fans() {
        Ok(n) => {
            r.kv("Fans", n);
            for i in 0..n {
                put(
                    r,
                    &format!("Fan {i} Speed"),
                    d.fan_speed(i).map(|s| format!("{s} %")),
                );
                put(r, &format!("Fan {i} RPM"), d.fan_speed_rpm(i));
            }
        }
        Err(e) => put::<String>(r, "Fans", Err(e)),
    }
    put(
        r,
        "Fan Speed Range",
        d.min_max_fan_speed()
            .map(|(lo, hi)| format!("{lo} - {hi} %")),
    );
}

fn pcie(r: &mut Report, d: &Device) {
    r.subsection("PCIe (NVML)");
    put(r, "Bus ID", d.pci_info().map(|p| p.bus_id));
    put(r, "Current Generation", d.current_pcie_link_gen());
    put(r, "Max Generation", d.max_pcie_link_gen());
    put(
        r,
        "Current Width",
        d.current_pcie_link_width().map(|w| format!("x{w}")),
    );
    put(
        r,
        "Max Width",
        d.max_pcie_link_width().map(|w| format!("x{w}")),
    );
    put(
        r,
        "Current Speed",
        d.pcie_link_speed().map(|s| format!("{s} MT/s")),
    );
    put(
        r,
        "Max Speed",
        d.max_pcie_link_speed().map(|s| {
            s.as_integer()
                .map_or("unknown".into(), |v| format!("{v} MT/s"))
        }),
    );
    put(
        r,
        "TX Throughput",
        d.pcie_throughput(PcieUtilCounter::Send)
            .map(|k| format!("{k} KB/s")),
    );
    put(
        r,
        "RX Throughput",
        d.pcie_throughput(PcieUtilCounter::Receive)
            .map(|k| format!("{k} KB/s")),
    );
    put(r, "Replay Counter", d.pcie_replay_counter());
}

fn nvlink(r: &mut Report, d: &Device) {
    let mut lines = Vec::new();
    for link in 0..MAX_NVLINKS {
        let l = d.link_wrapper_for(link);
        match l.is_active() {
            Ok(active) => {
                let mut s = format!("active {}", yes_no(active));
                if let Ok(v) = l.version() {
                    s.push_str(&format!(", version {v}"));
                }
                if let Ok(p) = l.remote_pci_info() {
                    s.push_str(&format!(", remote {}", p.bus_id));
                }
                lines.push((link, s));
            }
            Err(_) => break,
        }
    }
    if lines.is_empty() {
        return;
    }
    r.subsection("NVLink");
    for (link, s) in lines {
        r.kv(&format!("Link {link}"), s);
    }
}

fn process_name(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "?".into())
}

fn processes(r: &mut Report, d: &Device) {
    let compute = d.running_compute_processes();
    let graphics = d.running_graphics_processes();
    r.subsection("Processes");
    for (kind, res) in [("compute", compute), ("graphics", graphics)] {
        match res {
            Ok(list) if list.is_empty() => r.kv(&format!("{kind} processes"), 0),
            Ok(list) => {
                r.kv(&format!("{kind} processes"), list.len());
                for p in list {
                    let mem = match p.used_gpu_memory {
                        UsedGpuMemory::Used(b) => bytes(b),
                        UsedGpuMemory::Unavailable => "memory n/a".into(),
                    };
                    r.line(format!(
                        "  pid {:<8} {:<20} {mem}",
                        p.pid,
                        process_name(p.pid)
                    ));
                }
            }
            Err(e) => put::<String>(r, &format!("{kind} processes"), Err(e)),
        }
    }
}

/// PCIe/NVLink topology between every pair of NVIDIA GPUs.
pub fn render_topology(r: &mut Report, nvml: &Nvml) {
    let Ok(count) = nvml.device_count() else {
        return;
    };
    if count < 2 {
        return;
    }
    r.subsection("NVIDIA Topology");
    for a in 0..count {
        for b in (a + 1)..count {
            let (Ok(da), Ok(db)) = (nvml.device_by_index(a), nvml.device_by_index(b)) else {
                continue;
            };
            put(
                r,
                &format!("GPU {a} <-> GPU {b}"),
                da.topology_common_ancestor(db).map(|t| format!("{t:?}")),
            );
        }
    }
}
