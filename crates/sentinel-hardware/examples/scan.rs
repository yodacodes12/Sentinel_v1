//! Print the inventory of this machine as JSON: `cargo run -p sentinel-hardware --example scan`.
//! Optional argument: a fixture root to read instead of `/`.

fn main() {
    let root = match std::env::args().nth(1) {
        Some(path) => sentinel_hardware::SysRoot::at(path),
        None => sentinel_hardware::SysRoot::host(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ctx = sentinel_hardware::DiscoveryContext::new(root, now, Default::default());
    let outcome = sentinel_hardware::discover(&ctx, &mut |_| {});
    println!(
        "{}",
        serde_json::to_string_pretty(&outcome.inventory).expect("serialises")
    );
}
