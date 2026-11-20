//! Print this machine's utility catalogue as JSON:
//! `cargo run -p sentinel-utilities --example utilities`.

fn main() {
    let scan = sentinel_utilities::discover(
        &sentinel_hardware::SysRoot::host(),
        &sentinel_utilities::UtilityEnv::from_env(),
        0,
        &Default::default(),
        &mut |_| {},
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&scan.catalog).expect("serialises")
    );
}
