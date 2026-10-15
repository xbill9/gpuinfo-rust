//! Vendor-neutral GPU discovery from Linux sysfs (`/sys/bus/pci`, `/sys/class/drm`).
//!
//! Works for NVIDIA, AMD and Intel GPUs without any vendor library: PCI
//! identity, driver, PCIe link, BARs, AER counters, DRM nodes, display
//! connectors, hwmon sensors, and the amdgpu / i915 / xe specific files.

use std::fs;
use std::path::{Path, PathBuf};

use crate::pciids;
use crate::report::{Report, bytes};

const PCI_DEVICES: &str = "/sys/bus/pci/devices";

pub struct PciGpu {
    pub address: String,
    pub path: PathBuf,
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsys_vendor_id: u16,
    pub subsys_device_id: u16,
    pub class: u32,
    pub driver: Option<String>,
}

impl PciGpu {
    pub fn vendor_name(&self) -> String {
        pciids::vendor(self.vendor_id).unwrap_or_else(|| format!("{:04x}", self.vendor_id))
    }

    pub fn device_name(&self) -> String {
        pciids::device(self.vendor_id, self.device_id)
            .unwrap_or_else(|| format!("{:04x}", self.device_id))
    }

    /// PCI domain, bus, device and function parsed from the address.
    pub fn bdf(&self) -> Option<(u32, u32, u32, u32)> {
        parse_bdf(&self.address)
    }
}

pub fn parse_bdf(addr: &str) -> Option<(u32, u32, u32, u32)> {
    let (domain, rest) = addr.rsplit_once(':')?;
    let (domain, bus) = domain.rsplit_once(':')?;
    let (dev, func) = rest.split_once('.')?;
    Some((
        u32::from_str_radix(domain, 16).ok()?,
        u32::from_str_radix(bus, 16).ok()?,
        u32::from_str_radix(dev, 16).ok()?,
        u32::from_str_radix(func, 16).ok()?,
    ))
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_hex(path: impl AsRef<Path>) -> Option<u32> {
    read(path).and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
}

fn link_name(path: impl AsRef<Path>) -> Option<String> {
    fs::read_link(path)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
}

fn sorted_entries(dir: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Display controllers (PCI class 0x03) and processing accelerators (0x12).
pub fn discover() -> Vec<PciGpu> {
    sorted_entries(PCI_DEVICES)
        .into_iter()
        .filter_map(|path| {
            let class = read_hex(path.join("class"))?;
            if !matches!(class >> 16, 0x03 | 0x12) {
                return None;
            }
            Some(PciGpu {
                address: path.file_name()?.to_string_lossy().into_owned(),
                vendor_id: read_hex(path.join("vendor"))? as u16,
                device_id: read_hex(path.join("device"))? as u16,
                subsys_vendor_id: read_hex(path.join("subsystem_vendor")).unwrap_or(0) as u16,
                subsys_device_id: read_hex(path.join("subsystem_device")).unwrap_or(0) as u16,
                class,
                driver: link_name(path.join("driver")),
                path,
            })
        })
        .collect()
}

pub fn render(r: &mut Report, gpu: &PciGpu) {
    let p = &gpu.path;

    r.subsection("PCI (sysfs)");
    r.kv("PCI Address", &gpu.address);
    r.kv(
        "Vendor",
        format!("{} [{:04x}]", gpu.vendor_name(), gpu.vendor_id),
    );
    r.kv(
        "Device",
        format!("{} [{:04x}]", gpu.device_name(), gpu.device_id),
    );
    let sub_vendor = pciids::vendor(gpu.subsys_vendor_id)
        .unwrap_or_else(|| format!("{:04x}", gpu.subsys_vendor_id));
    r.kv(
        "Subsystem Vendor",
        format!("{sub_vendor} [{:04x}]", gpu.subsys_vendor_id),
    );
    r.opt_kv(
        "Subsystem Device",
        pciids::subsystem(
            gpu.vendor_id,
            gpu.device_id,
            gpu.subsys_vendor_id,
            gpu.subsys_device_id,
        )
        .map(|n| format!("{n} [{:04x}]", gpu.subsys_device_id))
        .or(Some(format!("[{:04x}]", gpu.subsys_device_id))),
    );
    let class_name = pciids::class((gpu.class >> 16) as u8, (gpu.class >> 8) as u8)
        .unwrap_or_else(|| "unknown".into());
    r.kv("Class", format!("{class_name} [{:06x}]", gpu.class));
    r.opt_kv("Revision", read(p.join("revision")));
    r.kv("Name Source", pciids::source());

    r.subsection("Driver");
    match &gpu.driver {
        Some(drv) => {
            r.kv("Kernel Driver", drv);
            r.opt_kv(
                "Driver Module Version",
                read(format!("/sys/module/{drv}/version")),
            );
            r.opt_kv(
                "Driver srcversion",
                read(format!("/sys/module/{drv}/srcversion")),
            );
        }
        None => r.kv("Kernel Driver", "none bound"),
    }
    r.opt_kv("Modalias", read(p.join("modalias")));

    r.subsection("PCIe Link");
    r.opt_kv("Current Link Speed", read(p.join("current_link_speed")));
    r.opt_kv(
        "Current Link Width",
        read(p.join("current_link_width")).map(|w| format!("x{w}")),
    );
    r.opt_kv("Max Link Speed", read(p.join("max_link_speed")));
    r.opt_kv(
        "Max Link Width",
        read(p.join("max_link_width")).map(|w| format!("x{w}")),
    );
    for (file, label) in [
        ("aer_dev_correctable", "AER Correctable Errors"),
        ("aer_dev_nonfatal", "AER Non-fatal Errors"),
        ("aer_dev_fatal", "AER Fatal Errors"),
    ] {
        let total = read(p.join(file)).and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("TOTAL_"))
                .and_then(|l| l.split_whitespace().last().map(str::to_string))
        });
        r.opt_kv(label, total);
    }

    r.subsection("Platform");
    r.opt_kv("NUMA Node", read(p.join("numa_node")));
    r.opt_kv("Local CPUs", read(p.join("local_cpulist")));
    r.kv(
        "IOMMU Group",
        link_name(p.join("iommu_group")).unwrap_or_else(|| "none".into()),
    );
    r.opt_kv("IRQ", read(p.join("irq")));
    r.opt_kv(
        "Boot VGA",
        read(p.join("boot_vga")).map(|v| if v == "1" { "yes" } else { "no" }),
    );
    r.opt_kv("PCI Power State", read(p.join("power_state")));
    r.opt_kv("Runtime PM Status", read(p.join("power/runtime_status")));
    r.opt_kv("Runtime PM Control", read(p.join("power/control")));
    r.opt_kv("D3cold Allowed", read(p.join("d3cold_allowed")));
    r.opt_kv("Reset Methods", read(p.join("reset_method")));

    render_bars(r, p);
    render_drm(r, p);
    render_amdgpu(r, p);
    render_hwmon(r, p);
    r.end_subsection();
}

fn render_bars(r: &mut Report, p: &Path) {
    const IORESOURCE_IO: u64 = 0x100;
    const IORESOURCE_PREFETCH: u64 = 0x2000;
    const IORESOURCE_MEM_64: u64 = 0x10_0000;
    const ROM_INDEX: usize = 6;

    let Some(text) = read(p.join("resource")) else {
        return;
    };
    r.subsection("BARs");
    for (i, line) in text.lines().enumerate() {
        let nums: Vec<u64> = line
            .split_whitespace()
            .filter_map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .collect();
        let [start, end, flags] = nums[..] else {
            continue;
        };
        if start == 0 && end == 0 {
            continue;
        }
        let mut kind = vec![if flags & IORESOURCE_IO != 0 {
            "I/O"
        } else {
            "memory"
        }];
        if flags & IORESOURCE_MEM_64 != 0 {
            kind.push("64-bit");
        }
        if flags & IORESOURCE_PREFETCH != 0 {
            kind.push("prefetchable");
        }
        if p.join(format!("resource{i}_resize")).exists() {
            kind.push("resizable");
        }
        let label = if i == ROM_INDEX {
            "Expansion ROM".to_string()
        } else {
            format!("BAR{i}")
        };
        r.kv(
            &label,
            format!(
                "{start:#x} size {} [{}]",
                bytes(end - start + 1),
                kind.join(", ")
            ),
        );
    }
}

fn render_drm(r: &mut Report, p: &Path) {
    let nodes: Vec<PathBuf> = sorted_entries(p.join("drm"));
    if nodes.is_empty() {
        return;
    }
    r.subsection("DRM");
    let names: Vec<String> = nodes
        .iter()
        .filter_map(|n| n.file_name().map(|f| f.to_string_lossy().into_owned()))
        .collect();
    r.kv("DRM Nodes", names.join(", "));

    for card in names.iter().filter(|n| n.starts_with("card")) {
        let card_dir = p.join("drm").join(card);
        // Intel i915 GT frequency controls live on the card node.
        for (file, label) in [
            ("gt_cur_freq_mhz", "GT Current Freq (MHz)"),
            ("gt_act_freq_mhz", "GT Actual Freq (MHz)"),
            ("gt_min_freq_mhz", "GT Min Freq (MHz)"),
            ("gt_max_freq_mhz", "GT Max Freq (MHz)"),
            ("gt_boost_freq_mhz", "GT Boost Freq (MHz)"),
            ("gt_RP0_freq_mhz", "GT RP0 Freq (MHz)"),
            ("gt_RPn_freq_mhz", "GT RPn Freq (MHz)"),
        ] {
            r.opt_kv(label, read(card_dir.join(file)));
        }

        let prefix = format!("{card}-");
        for conn in sorted_entries("/sys/class/drm") {
            let Some(name) = conn.file_name().map(|f| f.to_string_lossy().into_owned()) else {
                continue;
            };
            let Some(port) = name.strip_prefix(&prefix) else {
                continue;
            };
            let status = read(conn.join("status")).unwrap_or_else(|| "unknown".into());
            let mut desc = status.clone();
            if let Some(enabled) = read(conn.join("enabled")) {
                desc.push_str(&format!(", {enabled}"));
            }
            if status == "connected" {
                if let Some(mode) =
                    read(conn.join("modes")).and_then(|m| m.lines().next().map(str::to_string))
                {
                    desc.push_str(&format!(", preferred mode {mode}"));
                }
                if let Some(monitor) = fs::read(conn.join("edid"))
                    .ok()
                    .and_then(|e| edid_monitor(&e))
                {
                    desc.push_str(&format!(", monitor {monitor}"));
                }
            }
            r.kv(&format!("Connector {port}"), desc);
        }
    }
}

/// Monitor name and manufacturer from a 128-byte EDID base block.
fn edid_monitor(edid: &[u8]) -> Option<String> {
    const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];
    if edid.len() < 128 || edid[..8] != HEADER {
        return None;
    }
    let id = u16::from_be_bytes([edid[8], edid[9]]);
    let letter = |shift: u16| (b'@' + ((id >> shift) & 0x1f) as u8) as char;
    let manufacturer: String = [letter(10), letter(5), letter(0)].iter().collect();

    let name = (0..4).find_map(|i| {
        let d = &edid[54 + i * 18..54 + (i + 1) * 18];
        (d[0] == 0 && d[1] == 0 && d[3] == 0xfc).then(|| {
            String::from_utf8_lossy(&d[5..])
                .split('\n')
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        })
    });
    Some(match name {
        Some(n) if !n.is_empty() => format!("{n} ({manufacturer})"),
        _ => manufacturer,
    })
}

fn render_amdgpu(r: &mut Report, p: &Path) {
    if !p.join("mem_info_vram_total").exists() && !p.join("gpu_busy_percent").exists() {
        return;
    }
    r.subsection("amdgpu");
    for (file, label) in [
        ("product_name", "Product Name"),
        ("product_number", "Product Number"),
        ("serial_number", "Serial Number"),
        ("unique_id", "Unique ID"),
        ("vbios_version", "VBIOS Version"),
        ("power_dpm_state", "DPM State"),
        ("power_dpm_force_performance_level", "Performance Level"),
        ("gpu_busy_percent", "GPU Busy (%)"),
        ("mem_busy_percent", "Memory Busy (%)"),
        ("mem_info_vram_vendor", "VRAM Vendor"),
    ] {
        r.opt_kv(label, read(p.join(file)));
    }
    for (file, label) in [
        ("mem_info_vram_total", "VRAM Total"),
        ("mem_info_vram_used", "VRAM Used"),
        ("mem_info_vis_vram_total", "Visible VRAM Total"),
        ("mem_info_vis_vram_used", "Visible VRAM Used"),
        ("mem_info_gtt_total", "GTT Total"),
        ("mem_info_gtt_used", "GTT Used"),
    ] {
        r.opt_kv(
            label,
            read(p.join(file)).and_then(|s| s.parse().ok()).map(bytes),
        );
    }
    for (file, label) in [
        ("pp_dpm_sclk", "Shader Clock Levels"),
        ("pp_dpm_mclk", "Memory Clock Levels"),
        ("pp_dpm_fclk", "Fabric Clock Levels"),
        ("pp_dpm_socclk", "SoC Clock Levels"),
        ("pp_dpm_pcie", "PCIe DPM Levels"),
    ] {
        if let Some(levels) = read(p.join(file)) {
            r.kv(label, levels.lines().collect::<Vec<_>>().join(" | "));
        }
    }
}

fn render_hwmon(r: &mut Report, p: &Path) {
    for hw in sorted_entries(p.join("hwmon")) {
        let name = read(hw.join("name")).unwrap_or_else(|| "hwmon".into());
        r.subsection(&format!("Sensors: {name}"));
        let mut files: Vec<String> = sorted_entries(&hw)
            .iter()
            .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        files.sort();
        for file in files {
            let (scale, unit) = if file.starts_with("temp") && file.ends_with("_input")
                || file.ends_with("_crit")
                || file.ends_with("_emergency")
            {
                (1000.0, "°C")
            } else if file.starts_with("power")
                && (file.ends_with("_average")
                    || file.ends_with("_input")
                    || file.ends_with("_cap")
                    || file.ends_with("_cap_max"))
            {
                (1_000_000.0, "W")
            } else if file.starts_with("in") && file.ends_with("_input") {
                (1.0, "mV")
            } else if file.starts_with("curr") && file.ends_with("_input") {
                (1.0, "mA")
            } else if file.starts_with("fan") && file.ends_with("_input") {
                (1.0, "RPM")
            } else if file.starts_with("freq") && file.ends_with("_input") {
                (1_000_000.0, "MHz")
            } else if file.starts_with("energy") && file.ends_with("_input") {
                (1_000_000.0, "J")
            } else {
                continue;
            };
            let Some(raw) = read(hw.join(&file)).and_then(|s| s.parse::<f64>().ok()) else {
                continue;
            };
            let base = file.split('_').next().unwrap_or(&file);
            let label = read(hw.join(format!("{base}_label")))
                .map(|l| format!("{file} ({l})"))
                .unwrap_or(file.clone());
            r.kv(&label, format!("{:.1} {unit}", raw / scale));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pci_address() {
        assert_eq!(parse_bdf("0000:01:00.0"), Some((0, 1, 0, 0)));
        assert_eq!(parse_bdf("00000000:41:1f.3"), Some((0, 0x41, 0x1f, 3)));
        assert_eq!(parse_bdf("garbage"), None);
    }

    #[test]
    fn decodes_edid_manufacturer_and_name() {
        let mut edid = vec![0u8; 128];
        edid[..8].copy_from_slice(&[0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]);
        // "DEL" = 4,5,12 -> 00100 00101 01100
        edid[8] = 0b0001_0000;
        edid[9] = 0b1010_1100;
        let d = &mut edid[54..72];
        d[3] = 0xfc;
        d[5..5 + 6].copy_from_slice(b"U2723\n");
        assert_eq!(edid_monitor(&edid).as_deref(), Some("U2723 (DEL)"));
    }
}
