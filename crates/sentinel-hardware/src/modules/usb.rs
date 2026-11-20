//! USB devices and the Linux USB topology from `/sys/bus/usb/devices`.
//!
//! Port numbers are the kernel's. They do not tell which physical socket on
//! the case a device is plugged into; that needs board documentation.
//! Device serial numbers are not read.

use sentinel_inventory::{Availability, Confidence, Field, ModuleOutcome, UsbDevice, UsbPort};

use crate::context::{DiscoveryContext, ModuleRun, Stopped, USB_DEVICES, last_pci_address};
use crate::fields;
use crate::parsers::{classes, hex_u16, hex_u32, usb_speed_kbps};

/// `(parent, port number)` of a USB device name. Root hubs (`usb3`) have
/// neither; `3-3` sits on port 3 of `usb3`; `3-3.1` on port 1 of `3-3`.
pub fn parent_and_port(name: &str) -> (Option<String>, Option<u32>) {
    if name.starts_with("usb") {
        return (None, None);
    }
    let Some((bus, path)) = name.split_once('-') else {
        return (None, None);
    };
    match path.rsplit_once('.') {
        Some((upstream, port)) => (Some(format!("{bus}-{upstream}")), port.parse().ok()),
        None => (Some(format!("usb{bus}")), path.parse().ok()),
    }
}

/// Name of the device that would sit on `port` of `hub`.
pub fn child_name(hub: &str, port: u32) -> String {
    match hub.strip_prefix("usb") {
        Some(bus) => format!("{bus}-{port}"),
        None => format!("{hub}.{port}"),
    }
}

pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<Vec<UsbDevice>>, Stopped> {
    let root = &ctx.root;
    let Ok(entries) = root.list(USB_DEVICES) else {
        return Ok(ModuleRun::new(
            Vec::new(),
            ModuleOutcome::Unavailable,
            0,
            vec!["/sys/bus/usb/devices is not available".into()],
        ));
    };
    let names = ctx.usb_names();
    let ids_source = ctx.usb_ids_source();
    let mut devices = Vec::new();
    for name in entries.iter().filter(|n| !n.contains(':')) {
        ctx.check()?;
        let dir = format!("{USB_DEVICES}/{name}");
        let read = |attr: &str| root.read_trimmed(&format!("{dir}/{attr}")).ok();
        let (Some(vendor_id), Some(product_id)) = (
            read("idVendor").and_then(|v| hex_u16(&v)),
            read("idProduct").and_then(|v| hex_u16(&v)),
        ) else {
            continue;
        };
        let device_class = read("bDeviceClass").and_then(|v| hex_u32(&v)).unwrap_or(0) as u8;
        let is_root_hub = name.starts_with("usb");
        let is_hub = device_class == 0x09;

        let mut interface_classes = Vec::new();
        let mut hub_interface = None;
        // Interfaces are `<name>:<config>.<n>`; a root hub `usbN` uses `N-0:`.
        let iface_prefix = match name.strip_prefix("usb") {
            Some(bus) => format!("{bus}-0:"),
            None => format!("{name}:"),
        };
        for iface in entries.iter().filter(|e| e.starts_with(&iface_prefix)) {
            if let Some(class) = root
                .read_trimmed(&format!("{USB_DEVICES}/{iface}/bInterfaceClass"))
                .ok()
                .and_then(|v| hex_u32(&v))
            {
                let label = classes::usb_class_name(class as u8).to_string();
                if !interface_classes.contains(&label) {
                    interface_classes.push(label);
                }
                if class == 0x09 && hub_interface.is_none() {
                    hub_interface = Some(iface.clone());
                }
            }
        }

        let mut ports = Vec::new();
        if let Some(iface) = &hub_interface {
            let port_prefix = format!("{name}-port");
            for entry in root
                .list(&format!("{USB_DEVICES}/{iface}"))
                .unwrap_or_default()
            {
                let Some(number) = entry
                    .strip_prefix(&port_prefix)
                    .and_then(|n| n.parse::<u32>().ok())
                else {
                    continue;
                };
                let child = child_name(name, number);
                ports.push(UsbPort {
                    number,
                    connect_type: root
                        .read_trimmed(&format!("{USB_DEVICES}/{iface}/{entry}/connect_type"))
                        .ok(),
                    device: entries.contains(&child).then_some(child),
                });
            }
            ports.sort_by_key(|p| p.number);
        }

        let (parent, port_number) = parent_and_port(name);
        let path = root.canonical(&dir).unwrap_or_default();
        let fr = Confidence::FirmwareReported;
        let db = |value: Option<&String>| match (names, value) {
            (None, _) => Field::missing(Availability::Unavailable, Some("usb.ids".into()))
                .with_note("the USB ID database (hwdata) is not installed"),
            (Some(_), Some(v)) => {
                Field::detected(v.clone(), ids_source.clone(), Confidence::Database)
            }
            (Some(_), None) => Field::missing(Availability::Unknown, Some(ids_source.clone())),
        };
        let speed = match read("speed").and_then(|v| usb_speed_kbps(&v)) {
            Some(k) => Field::detected(
                k,
                root.display(&format!("{dir}/speed")),
                Confidence::KernelReported,
            ),
            None => Field::missing(
                Availability::Unavailable,
                Some(root.display(&format!("{dir}/speed"))),
            ),
        };
        devices.push(UsbDevice {
            discovered_at: ctx.now,
            bus: read("busnum").and_then(|v| v.parse().ok()).unwrap_or(0),
            device_number: read("devnum").and_then(|v| v.parse().ok()).unwrap_or(0),
            vendor_id,
            product_id,
            manufacturer: fields::text(root, &format!("{dir}/manufacturer"), fr),
            product: fields::text(root, &format!("{dir}/product"), fr),
            vendor_name: db(names.and_then(|n| n.vendors.get(&vendor_id))),
            product_name: db(names.and_then(|n| n.devices.get(&(vendor_id, product_id)))),
            usb_version: fields::text(root, &format!("{dir}/version"), fr),
            speed_kbps: speed,
            device_class,
            class_name: classes::usb_class_name(device_class).to_string(),
            interface_classes,
            is_root_hub,
            is_hub,
            parent,
            port_number,
            ports,
            removable: fields::text(
                root,
                &format!("{dir}/removable"),
                Confidence::KernelReported,
            ),
            controller_pci_address: if is_root_hub {
                last_pci_address(&path)
            } else {
                None
            },
            sysfs_path: format!("/{path}"),
            sysfs_name: name.clone(),
        });
    }
    let mut notes = Vec::new();
    if names.is_none() {
        notes.push("USB ID database not installed: devices are shown by numeric ID".into());
    }
    let outcome = if notes.is_empty() {
        ModuleOutcome::Completed
    } else {
        ModuleOutcome::Partial
    };
    let count = devices.len();
    Ok(ModuleRun::new(devices, outcome, count, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_names() {
        assert_eq!(parent_and_port("usb3"), (None, None));
        assert_eq!(parent_and_port("3-3"), (Some("usb3".into()), Some(3)));
        assert_eq!(parent_and_port("3-3.1"), (Some("3-3".into()), Some(1)));
        assert_eq!(parent_and_port("1-2.4.2"), (Some("1-2.4".into()), Some(2)));
        assert_eq!(child_name("usb1", 2), "1-2");
        assert_eq!(child_name("3-3", 1), "3-3.1");
    }
}
