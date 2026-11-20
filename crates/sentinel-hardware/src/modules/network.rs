//! Network interfaces from `/sys/class/net`.
//!
//! Identity and link state only: no packets are captured, no traffic
//! counters are collected and nothing is configured. MAC addresses are
//! local identifiers; they stay in the inventory and are never logged.

use sentinel_inventory::{
    Availability, Confidence, Field, MacOrigin, ModuleOutcome, NetworkInterface, NetworkKind,
};

use crate::context::{DiscoveryContext, ModuleRun, Stopped, last_pci_address};
use crate::fields;
use crate::parsers::{hex_u16, key_values};

const NET: &str = "sys/class/net";

pub fn classify(
    arp_type: Option<u32>,
    devtype: Option<&str>,
    wireless: bool,
    has_device: bool,
) -> NetworkKind {
    match (arp_type, devtype) {
        (Some(772), _) => NetworkKind::Loopback,
        (_, Some("wlan")) => NetworkKind::Wireless,
        _ if wireless => NetworkKind::Wireless,
        (_, Some("wwan")) => NetworkKind::Wwan,
        (_, Some("bridge")) => NetworkKind::Bridge,
        (Some(65534 | 768 | 769 | 776 | 778 | 823), _) => NetworkKind::Tunnel,
        (Some(1), _) if has_device => NetworkKind::Ethernet,
        (Some(1), _) => NetworkKind::Virtual,
        (Some(519 | 530), _) => NetworkKind::Wwan,
        _ if !has_device => NetworkKind::Virtual,
        _ => NetworkKind::Other,
    }
}

pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<Vec<NetworkInterface>>, Stopped> {
    let root = &ctx.root;
    let Ok(names) = root.list(NET) else {
        return Ok(ModuleRun::new(
            Vec::new(),
            ModuleOutcome::Unavailable,
            0,
            vec!["/sys/class/net is not available".into()],
        ));
    };
    let mut interfaces = Vec::new();
    for name in names {
        ctx.check()?;
        let dir = format!("{NET}/{name}");
        if !root.is_dir(&dir) {
            continue;
        }
        let kr = Confidence::KernelReported;
        let read = |attr: &str| root.read_trimmed(&format!("{dir}/{attr}")).ok();
        let uevent = root
            .read(&format!("{dir}/uevent"))
            .map(|t| key_values(&t))
            .unwrap_or_default();
        let has_device = root.exists(&format!("{dir}/device"));
        let wireless =
            root.is_dir(&format!("{dir}/wireless")) || root.exists(&format!("{dir}/phy80211"));
        let arp = read("type").and_then(|v| v.parse::<u32>().ok());
        let kind = classify(
            arp,
            uevent.get("DEVTYPE").map(String::as_str),
            wireless,
            has_device,
        );

        let mac = if kind == NetworkKind::Loopback {
            Field::missing(Availability::Unsupported, None)
                .with_note("loopback has no hardware address")
        } else {
            fields::text(root, &format!("{dir}/address"), kr)
                .validate(|m| m != "00:00:00:00:00:00", "no hardware address")
        };
        let origin = match read("addr_assign_type").as_deref() {
            Some("0") => Field::detected(
                MacOrigin::Permanent,
                root.display(&format!("{dir}/addr_assign_type")),
                kr,
            ),
            Some("1") => Field::detected(
                MacOrigin::Random,
                root.display(&format!("{dir}/addr_assign_type")),
                kr,
            ),
            Some("2") => Field::detected(
                MacOrigin::Stolen,
                root.display(&format!("{dir}/addr_assign_type")),
                kr,
            ),
            Some("3") => Field::detected(
                MacOrigin::Set,
                root.display(&format!("{dir}/addr_assign_type")),
                kr,
            ),
            _ => Field::missing(Availability::Unavailable, None),
        };
        // `carrier` and `speed` fail with EINVAL while a link is down.
        let carrier = fields::flag(root, &format!("{dir}/carrier"), kr);
        let speed = match read("speed").and_then(|v| v.parse::<i64>().ok()) {
            Some(s) if s > 0 => {
                Field::detected(s as u32, root.display(&format!("{dir}/speed")), kr)
            }
            _ => Field::missing(
                Availability::Unavailable,
                Some(root.display(&format!("{dir}/speed"))),
            )
            .with_note("the driver reports no link speed (link down, or not an Ethernet device)"),
        };
        let device_path = root.canonical(&format!("{dir}/device")).unwrap_or_default();
        let device_bus = if !has_device {
            Field::missing(Availability::Unsupported, None).with_note("software interface")
        } else if let Some(pci) = last_pci_address(&device_path) {
            Field::detected(
                format!("PCI {pci}"),
                root.display(&format!("{dir}/device")),
                kr,
            )
        } else if device_path.contains("/usb") {
            let usb = device_path
                .split('/')
                .rev()
                .find(|p| !p.contains(':'))
                .unwrap_or("usb");
            Field::detected(
                format!("USB {usb}"),
                root.display(&format!("{dir}/device")),
                kr,
            )
        } else {
            Field::missing(Availability::Unknown, None)
        };
        let id = |attr: &str| read(&format!("device/{attr}")).and_then(|v| hex_u16(&v));
        let driver = match root.link_name(&format!("{dir}/device/driver")) {
            Some(d) => Field::detected(d, root.display(&format!("{dir}/device/driver")), kr),
            None => Field::missing(Availability::Unavailable, None),
        };
        interfaces.push(NetworkInterface {
            discovered_at: ctx.now,
            kind,
            mac_address: mac,
            mac_address_origin: origin,
            operational_state: fields::text(root, &format!("{dir}/operstate"), kr),
            carrier,
            speed_mbps: speed,
            mtu: fields::number(root, &format!("{dir}/mtu"), kr),
            driver,
            device_bus,
            vendor_id: has_device.then(|| id("vendor")).flatten(),
            device_id: has_device.then(|| id("device")).flatten(),
            wireless_phy: root.link_name(&format!("{dir}/phy80211")),
            name,
        });
    }
    let count = interfaces.len();
    Ok(ModuleRun::new(
        interfaces,
        ModuleOutcome::Completed,
        count,
        Vec::new(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_kinds() {
        assert_eq!(
            classify(Some(772), None, false, false),
            NetworkKind::Loopback
        );
        assert_eq!(
            classify(Some(1), Some("wlan"), true, true),
            NetworkKind::Wireless
        );
        assert_eq!(classify(Some(1), None, false, true), NetworkKind::Ethernet);
        assert_eq!(
            classify(Some(1), Some("bridge"), false, false),
            NetworkKind::Bridge
        );
        assert_eq!(classify(Some(1), None, false, false), NetworkKind::Virtual);
        assert_eq!(
            classify(Some(65534), None, false, false),
            NetworkKind::Tunnel
        );
    }
}
