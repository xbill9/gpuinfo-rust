//! PCI vendor / device / subsystem / class names.
//!
//! The system `pci.ids` file is preferred because distributions update it more
//! often than the copy compiled into the `pci-ids` crate; the crate is the
//! fallback for hosts (and containers) that ship no database.

use std::collections::HashMap;
use std::sync::LazyLock;

const SYSTEM_PATHS: [&str; 3] = [
    "/usr/share/misc/pci.ids",
    "/usr/share/hwdata/pci.ids",
    "/usr/share/pci.ids",
];

#[derive(Default)]
struct Db {
    vendors: HashMap<u16, String>,
    devices: HashMap<(u16, u16), String>,
    subsystems: HashMap<(u16, u16, u16, u16), String>,
}

static SYSTEM_DB: LazyLock<Option<Db>> = LazyLock::new(|| {
    SYSTEM_PATHS
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .map(|text| parse(&text))
});

fn parse(text: &str) -> Db {
    let mut db = Db::default();
    let mut vendor: Option<u16> = None;
    let mut device: Option<u16> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        // The class list follows the vendor list and uses a different layout.
        if line.starts_with("C ") {
            break;
        }
        if let Some(rest) = line.strip_prefix("\t\t") {
            let mut parts = rest.splitn(3, ' ');
            let (Some(sv), Some(sd), Some(name)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            if let (Some(v), Some(d), Ok(sv), Ok(sd)) = (
                vendor,
                device,
                u16::from_str_radix(sv, 16),
                u16::from_str_radix(sd, 16),
            ) {
                db.subsystems
                    .insert((v, d, sv, sd), name.trim().to_string());
            }
        } else if let Some(rest) = line.strip_prefix('\t') {
            let Some((id, name)) = rest.split_once(' ') else {
                continue;
            };
            if let (Some(v), Ok(d)) = (vendor, u16::from_str_radix(id, 16)) {
                device = Some(d);
                db.devices.insert((v, d), name.trim().to_string());
            }
        } else if let Some((id, name)) = line.split_once(' ')
            && let Ok(v) = u16::from_str_radix(id, 16)
        {
            vendor = Some(v);
            device = None;
            db.vendors.insert(v, name.trim().to_string());
        }
    }
    db
}

pub fn vendor(v: u16) -> Option<String> {
    if let Some(name) = SYSTEM_DB.as_ref().and_then(|db| db.vendors.get(&v)) {
        return Some(name.clone());
    }
    pci_ids::Vendors::iter()
        .find(|x| x.id() == v)
        .map(|x| x.name().to_string())
}

pub fn device(v: u16, d: u16) -> Option<String> {
    if let Some(name) = SYSTEM_DB.as_ref().and_then(|db| db.devices.get(&(v, d))) {
        return Some(name.clone());
    }
    pci_ids::Device::from_vid_pid(v, d).map(|x| x.name().to_string())
}

pub fn subsystem(v: u16, d: u16, sv: u16, sd: u16) -> Option<String> {
    if let Some(name) = SYSTEM_DB
        .as_ref()
        .and_then(|db| db.subsystems.get(&(v, d, sv, sd)))
    {
        return Some(name.clone());
    }
    pci_ids::Device::from_vid_pid(v, d)?
        .subsystems()
        .find(|s| s.subvendor() == sv && s.subdevice() == sd)
        .map(|s| s.name().to_string())
}

pub fn class(class: u8, subclass: u8) -> Option<String> {
    pci_ids::Subclass::from_cid_sid(class, subclass).map(|s| s.name().to_string())
}

pub fn source() -> &'static str {
    if SYSTEM_DB.is_some() {
        "system pci.ids"
    } else {
        "built-in pci-ids database"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vendor_device_subsystem() {
        let db = parse(
            "# comment\n10de  NVIDIA Corporation\n\t2508  GA106 [GeForce RTX 3050 OEM]\n\t\t17aa c97a  Example board\nC 03  Display controller\n",
        );
        assert_eq!(db.vendors[&0x10de], "NVIDIA Corporation");
        assert_eq!(
            db.devices[&(0x10de, 0x2508)],
            "GA106 [GeForce RTX 3050 OEM]"
        );
        assert_eq!(
            db.subsystems[&(0x10de, 0x2508, 0x17aa, 0xc97a)],
            "Example board"
        );
    }
}
