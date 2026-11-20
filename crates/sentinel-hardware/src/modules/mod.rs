//! One module per hardware area. Each reads its own sources and never
//! depends on another module's results, so one failing cannot stop the rest.

pub mod battery;
pub mod cpu;
pub mod firmware;
pub mod gpu;
pub mod memory;
pub mod motherboard;
pub mod network;
pub mod pci;
pub mod sensors;
pub mod storage;
pub mod system_identity;
pub mod usb;
