//! Names for standard class codes (PCI, USB, SMBIOS chassis). These come
//! from the published specifications, not from vendor databases.

use sentinel_inventory::{ChassisType, PciRole};

/// PCI class name for a 24-bit class code (`0xCCSSPP`).
pub fn pci_class_name(class_code: u32) -> String {
    let base = (class_code >> 16) as u8;
    let sub = ((class_code >> 8) & 0xff) as u8;
    let name = match (base, sub) {
        (0x00, _) => "Unclassified device",
        (0x01, 0x00) => "SCSI storage controller",
        (0x01, 0x01) => "IDE interface",
        (0x01, 0x04) => "RAID bus controller",
        (0x01, 0x06) => "SATA controller",
        (0x01, 0x07) => "Serial Attached SCSI controller",
        (0x01, 0x08) => "Non-Volatile memory controller",
        (0x01, _) => "Mass storage controller",
        (0x02, 0x00) => "Ethernet controller",
        (0x02, 0x80) => "Network controller",
        (0x02, _) => "Network controller",
        (0x03, 0x00) => "VGA compatible controller",
        (0x03, 0x02) => "3D controller",
        (0x03, _) => "Display controller",
        (0x04, 0x01) => "Multimedia audio controller",
        (0x04, 0x03) => "Audio device",
        (0x04, _) => "Multimedia controller",
        (0x05, _) => "Memory controller",
        (0x06, 0x00) => "Host bridge",
        (0x06, 0x01) => "ISA bridge",
        (0x06, 0x04) => "PCI bridge",
        (0x06, _) => "Bridge",
        (0x07, _) => "Communication controller",
        (0x08, 0x05) => "SD host controller",
        (0x08, 0x06) => "IOMMU",
        (0x08, _) => "System peripheral",
        (0x09, _) => "Input device controller",
        (0x0c, 0x03) => "USB controller",
        (0x0c, 0x05) => "SMBus",
        (0x0c, 0x80) => "Serial bus controller",
        (0x0c, _) => "Serial bus controller",
        (0x0d, _) => "Wireless controller",
        (0x10, _) => "Encryption controller",
        (0x11, _) => "Signal processing controller",
        (0x12, _) => "Processing accelerator",
        (0x13, _) => "Non-essential instrumentation",
        _ => "Other device",
    };
    name.to_string()
}

pub fn pci_role(class_code: u32) -> PciRole {
    let base = (class_code >> 16) as u8;
    let sub = ((class_code >> 8) & 0xff) as u8;
    match (base, sub) {
        (0x03, _) => PciRole::Graphics,
        (0x02, _) | (0x0d, _) => PciRole::Network,
        (0x0c, 0x03) => PciRole::UsbController,
        (0x01, _) => PciRole::StorageController,
        (0x04, _) => PciRole::Audio,
        (0x06, _) => PciRole::Bridge,
        _ => PciRole::Other,
    }
}

/// USB (interface or device) class name.
pub fn usb_class_name(class: u8) -> &'static str {
    match class {
        0x00 => "Defined per interface",
        0x01 => "Audio",
        0x02 => "Communications",
        0x03 => "Human interface device",
        0x05 => "Physical",
        0x06 => "Imaging",
        0x07 => "Printer",
        0x08 => "Mass storage",
        0x09 => "Hub",
        0x0a => "CDC data",
        0x0b => "Smart card",
        0x0d => "Content security",
        0x0e => "Video",
        0x0f => "Personal healthcare",
        0x10 => "Audio/video",
        0x11 => "Billboard",
        0x12 => "USB Type-C bridge",
        0xdc => "Diagnostic",
        0xe0 => "Wireless controller",
        0xef => "Miscellaneous",
        0xfe => "Application specific",
        0xff => "Vendor specific",
        _ => "Unknown class",
    }
}

/// SMBIOS chassis type (DMI `chassis_type`), per the SMBIOS specification.
pub fn chassis_type(code: u32) -> Option<ChassisType> {
    Some(match code {
        3 | 4 | 5 | 6 | 7 | 15 | 16 => ChassisType::Desktop,
        8 | 9 | 10 | 14 => ChassisType::Laptop,
        31 => ChassisType::Convertible,
        30 | 32 => ChassisType::Tablet,
        17 | 23 | 25 | 28 | 29 => ChassisType::Server,
        13 => ChassisType::AllInOne,
        35 | 36 => ChassisType::MiniPc,
        1 | 2 => return None, // "Other", "Unknown"
        _ => ChassisType::Other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_classes() {
        assert_eq!(pci_class_name(0x030000), "VGA compatible controller");
        assert_eq!(pci_class_name(0x010802), "Non-Volatile memory controller");
        assert_eq!(pci_class_name(0x0c0330), "USB controller");
        assert_eq!(pci_class_name(0x028000), "Network controller");
        assert_eq!(pci_class_name(0xff0000), "Other device");
        assert_eq!(pci_role(0x030200), PciRole::Graphics);
        assert_eq!(pci_role(0x0c0330), PciRole::UsbController);
        assert_eq!(pci_role(0x060400), PciRole::Bridge);
        assert_eq!(pci_role(0x010601), PciRole::StorageController);
        assert_eq!(pci_role(0x0c0500), PciRole::Other);
    }

    #[test]
    fn usb_and_chassis() {
        assert_eq!(usb_class_name(0x08), "Mass storage");
        assert_eq!(usb_class_name(0x09), "Hub");
        assert_eq!(chassis_type(10), Some(ChassisType::Laptop));
        assert_eq!(chassis_type(3), Some(ChassisType::Desktop));
        assert_eq!(chassis_type(2), None);
    }
}
