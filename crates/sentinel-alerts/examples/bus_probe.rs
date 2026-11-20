//! Check that Sentinel's D-Bus client can talk to this session's bus
//! without showing a notification: `cargo run -p sentinel-alerts --example bus_probe`.

fn main() {
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_default();
    let target =
        sentinel_alerts::dbus::parse_address(&address).expect("DBUS_SESSION_BUS_ADDRESS is set");
    let uid = rustix::process::getuid().as_raw();
    match sentinel_alerts::dbus::probe(&target, uid) {
        Ok(()) => {
            println!("session bus OK: message accepted and routed (ServiceUnknown as expected)")
        }
        Err(e) => {
            println!("session bus probe failed: {e}");
            std::process::exit(1);
        }
    }
}
