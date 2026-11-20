//! `sentinel hardware`, `sentinel utilities` and `sentinel scan`.

use std::collections::BTreeSet;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant};

use sentinel_client::{InventoryView, Sentinel, UtilitiesView, ViewSource};
use sentinel_core::{Result, SentinelError};
use sentinel_inventory::{
    Availability, DiscoveryJob, Field, InventoryOrigin, JobProgress, JobState, ModuleOutcome,
    SystemInventory, UtilityCatalog, UtilityCategory,
};

/// Exit code when a scan was cancelled.
pub const EXIT_CANCELLED: u8 = 12;

pub fn json<T: serde::Serialize>(value: &T) -> Result<()> {
    let text =
        serde_json::to_string_pretty(value).map_err(|e| SentinelError::Protocol(e.to_string()))?;
    println!("{text}");
    Ok(())
}

// ---------------------------------------------------------------- format

pub fn bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Decimal capacity as drives are sold (GB), with the binary value.
pub fn capacity(value: u64) -> String {
    format!("{:.1} GB ({})", value as f64 / 1e9, bytes(value))
}

pub fn ghz(khz: u64) -> String {
    format!("{:.2} GHz", khz as f64 / 1e6)
}

/// A field's value, or why it is missing. Never a made-up default.
pub fn show<T>(field: &Field<T>, render: impl Fn(&T) -> String) -> String {
    match field.value() {
        Some(value) => render(value),
        None => match field.availability() {
            Availability::PermissionDenied => "not available (administrator only)".into(),
            Availability::Unsupported => "not provided by this system".into(),
            Availability::Invalid => "not reported (placeholder or invalid value)".into(),
            _ => "not available".into(),
        },
    }
}

pub fn text(field: &Field<String>) -> String {
    show(field, Clone::clone)
}

pub fn when(secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ago = now.saturating_sub(secs);
    let rel = if ago < 60 {
        format!("{ago}s ago")
    } else if ago < 3600 {
        format!("{}m ago", ago / 60)
    } else if ago < 86_400 {
        format!("{}h ago", ago / 3600)
    } else {
        format!("{}d ago", ago / 86_400)
    };
    format!("{rel} (unix time {secs})")
}

fn origin_line(origin: Option<InventoryOrigin>, source: ViewSource, at: u64) -> String {
    match (origin, source) {
        (Some(InventoryOrigin::ThisSession), _) => {
            format!("Scanned {} by the running daemon.", when(at))
        }
        (_, ViewSource::CacheFile) => format!(
            "Cached result from a scan {}. The daemon is not running; this is not live.",
            when(at)
        ),
        _ => format!(
            "Cached result from a scan {}. Run `sentinel scan` to refresh.",
            when(at)
        ),
    }
}

// --------------------------------------------------------------- hardware

pub fn render_inventory(inv: &SystemInventory) -> String {
    let mut out = String::new();
    let section = |out: &mut String, title: &str| out.push_str(&format!("\n{title}\n"));
    let row = |out: &mut String, label: &str, value: String| {
        out.push_str(&format!("  {:<22}{value}\n", format!("{label}:")))
    };

    if let Some(root) = &inv.root_override {
        out.push_str(&format!(
            "NOTE: read from the test directory {root}, not this machine.\n"
        ));
    }
    section(&mut out, "SYSTEM");
    if let Some(s) = &inv.system {
        row(&mut out, "Manufacturer", text(&s.manufacturer));
        row(&mut out, "Product", text(&s.product_name));
        row(&mut out, "Family", text(&s.product_family));
        row(
            &mut out,
            "Chassis",
            show(&s.chassis, |c| format!("{c:?}").to_lowercase()),
        );
        row(&mut out, "Architecture", text(&s.architecture));
        row(&mut out, "Kernel", text(&s.kernel_release));
        row(&mut out, "Distribution", text(&s.os.pretty_name));
    }
    section(&mut out, "MOTHERBOARD AND FIRMWARE");
    if let Some(b) = &inv.motherboard {
        row(&mut out, "Board manufacturer", text(&b.board_manufacturer));
        row(&mut out, "Board name", text(&b.board_name));
        row(&mut out, "Board revision", text(&b.board_revision));
        row(
            &mut out,
            "BIOS",
            format!(
                "{} {} ({})",
                text(&b.bios_vendor),
                text(&b.bios_version),
                text(&b.bios_release_date)
            ),
        );
        row(
            &mut out,
            "Identification",
            format!(
                "{:?} (firmware-reported, not verified)",
                b.identification.level
            )
            .replace("BoardIdentified", "board identified"),
        );
    }
    if let Some(f) = &inv.firmware {
        row(
            &mut out,
            "Boot mode",
            show(&f.boot_mode, |m| format!("{m:?}").to_uppercase()),
        );
    }
    for cpu in &inv.processors {
        section(&mut out, &format!("PROCESSOR (package {})", cpu.package_id));
        row(&mut out, "Model", text(&cpu.model_name));
        row(&mut out, "Vendor", text(&cpu.vendor));
        row(
            &mut out,
            "Microarchitecture",
            show(&cpu.microarchitecture, |m| format!("{m} (inferred)")),
        );
        row(
            &mut out,
            "Physical cores",
            show(&cpu.physical_cores, u32::to_string),
        );
        row(
            &mut out,
            "Logical processors",
            show(&cpu.logical_processors, u32::to_string),
        );
        row(
            &mut out,
            "Hybrid",
            show(&cpu.hybrid, |h| if *h { "yes".into() } else { "no".into() }),
        );
        row(
            &mut out,
            "Frequency range",
            match (cpu.min_frequency_khz.value(), cpu.max_frequency_khz.value()) {
                (Some(min), Some(max)) => format!("{} – {}", ghz(*min), ghz(*max)),
                _ => show(&cpu.max_frequency_khz, |k| ghz(*k)),
            },
        );
        for cache in &cpu.caches {
            row(
                &mut out,
                &format!(
                    "L{} {} cache",
                    cache.level,
                    format!("{:?}", cache.kind).to_lowercase()
                ),
                format!(
                    "{} × {}",
                    cache.instances,
                    cache
                        .size_bytes
                        .map(bytes)
                        .unwrap_or_else(|| "size unknown".into())
                ),
            );
        }
        out.push_str("  Topology:\n");
        for core in &cpu.cores {
            let cpus: Vec<String> = core
                .logical_cpus
                .iter()
                .map(|c| format!("CPU {c}"))
                .collect();
            out.push_str(&format!("    Core {:<3} {}\n", core.index, cpus.join(", ")));
        }
    }
    section(&mut out, "MEMORY");
    if let Some(m) = &inv.memory {
        row(
            &mut out,
            "Installed (firmware)",
            show(&m.installed_bytes, |b| bytes(*b)),
        );
        row(
            &mut out,
            "Kernel-managed",
            show(&m.kernel_physical_bytes, |b| bytes(*b)),
        );
        row(&mut out, "Usable", show(&m.usable_bytes, |b| bytes(*b)));
        row(&mut out, "Swap", show(&m.swap_total_bytes, |b| bytes(*b)));
        row(&mut out, "Type", text(&m.memory_type));
        row(
            &mut out,
            "Speed",
            show(&m.speed_mts, |s| format!("{s} MT/s")),
        );
        if let Some(modules) = m.modules.value() {
            for module in modules.iter().filter(|m| m.populated) {
                row(
                    &mut out,
                    module.locator.as_deref().unwrap_or("Module"),
                    format!(
                        "{} {}",
                        module.size_bytes.map(bytes).unwrap_or_default(),
                        module.memory_type.clone().unwrap_or_default()
                    ),
                );
            }
        }
    }
    section(&mut out, "GRAPHICS");
    for g in &inv.graphics {
        row(
            &mut out,
            &g.pci_address,
            format!(
                "{} — {} [{}], driver {}",
                text(&g.model_name),
                show(&g.kind, |k| format!("{k:?}").to_lowercase()),
                show(&g.memory, |m| format!(
                    "{} {}",
                    bytes(m.bytes),
                    match m.kind {
                        sentinel_inventory::GpuMemoryKind::DedicatedVram => "dedicated VRAM",
                        sentinel_inventory::GpuMemoryKind::ReservedSystemMemory =>
                            "reserved from system RAM",
                    }
                )),
                text(&g.driver)
            ),
        );
    }
    if inv.graphics.is_empty() {
        out.push_str("  No PCI graphics device detected.\n");
    }
    section(&mut out, "STORAGE");
    for d in inv.storage.iter().filter(|d| !d.is_virtual()) {
        row(
            &mut out,
            &d.name,
            format!(
                "{} — {}, {:?}, {}, {} partition(s)",
                text(&d.model),
                show(&d.interface, |i| format!("{i:?}").to_uppercase()),
                d.kind,
                show(&d.capacity_bytes, |c| capacity(*c)),
                d.partitions.len()
            ),
        );
    }
    let virtual_count = inv.storage.iter().filter(|d| d.is_virtual()).count();
    if virtual_count > 0 {
        out.push_str(&format!(
            "  ({virtual_count} virtual block devices not shown)\n"
        ));
    }
    section(&mut out, "BATTERY AND POWER");
    for b in &inv.batteries {
        row(
            &mut out,
            &b.name,
            format!(
                "{} {}, {} at scan time, health {}",
                text(&b.manufacturer),
                text(&b.model_name),
                show(&b.status, |s| format!("{s:?}").to_lowercase()),
                show(&b.health_percent, |h| format!("{h:.1}% of design capacity")),
            ),
        );
    }
    if inv.batteries.is_empty() {
        out.push_str("  No battery detected.\n");
    }
    for p in &inv.power_supplies {
        row(
            &mut out,
            &p.name,
            format!(
                "{}, {}",
                text(&p.kind),
                show(&p.online, |o| if *o {
                    "online".into()
                } else {
                    "offline".into()
                })
            ),
        );
    }
    section(&mut out, "NETWORK");
    for n in &inv.network {
        row(
            &mut out,
            &n.name,
            format!(
                "{:?}, {}, driver {}",
                n.kind,
                text(&n.operational_state),
                text(&n.driver)
            ),
        );
    }
    section(&mut out, "USB");
    for u in inv.usb_devices.iter().filter(|u| !u.is_root_hub) {
        let name = u
            .product
            .value()
            .or(u.product_name.value())
            .cloned()
            .unwrap_or_else(|| "unnamed device".into());
        row(
            &mut out,
            &u.sysfs_name,
            format!(
                "{:04x}:{:04x} {name} ({})",
                u.vendor_id,
                u.product_id,
                u.interface_classes.join(", ")
            ),
        );
    }
    out.push_str("  Port numbers are Linux's, not physical sockets.\n");
    section(&mut out, "PCI");
    out.push_str(&format!(
        "  {} devices (`sentinel hardware --json` for details)\n",
        inv.pci_devices.len()
    ));
    section(&mut out, "SENSORS (interfaces only; no readings are taken)");
    let chips: BTreeSet<&str> = inv.sensors.iter().map(|s| s.chip.as_str()).collect();
    out.push_str(&format!(
        "  {} sensor inputs on {} chip(s): {}; {} thermal zone(s)\n",
        inv.sensors.len(),
        chips.len(),
        chips.into_iter().collect::<Vec<_>>().join(", "),
        inv.thermal_zones.len()
    ));
    out
}

pub fn render_modules(progress: &JobProgress) -> String {
    let mut out = String::new();
    for report in &progress.modules {
        let mark = match report.outcome {
            ModuleOutcome::Completed => "ok     ",
            ModuleOutcome::Partial => "partial",
            ModuleOutcome::Unavailable => "n/a    ",
            ModuleOutcome::Failed => "FAILED ",
            ModuleOutcome::Cancelled => "stopped",
        };
        out.push_str(&format!(
            "  [{mark}] {:<22} {:>4} item(s) {:>6} ms\n",
            report.module.label(),
            report.items,
            report.duration_ms
        ));
        for note in &report.notes {
            out.push_str(&format!("            {note}\n"));
        }
    }
    out
}

pub fn hardware(sentinel: &Sentinel, as_json: bool) -> Result<ExitCode> {
    let view: InventoryView = sentinel.inventory()?;
    if as_json {
        json(&view)?;
        return Ok(ExitCode::SUCCESS);
    }
    if let Some(warning) = &view.warning {
        eprintln!("sentinel: {warning}");
    }
    match &view.inventory {
        Some(inv) => {
            println!(
                "{}",
                origin_line(view.origin, view.source, inv.discovered_at)
            );
            print!("{}", render_inventory(inv));
            Ok(ExitCode::SUCCESS)
        }
        None => {
            println!(
                "No hardware inventory yet. Run `sentinel scan` (the daemon must be running)."
            );
            Ok(ExitCode::from(SentinelError::DaemonUnavailable.exit_code()))
        }
    }
}

pub fn capabilities(sentinel: &Sentinel, as_json: bool) -> Result<ExitCode> {
    let caps = sentinel.capabilities()?;
    if as_json {
        json(&caps)?;
    } else {
        for c in &caps.interfaces {
            println!(
                "  {:<18} {:<18} {}{}",
                c.id,
                format!("{:?}", c.status).to_lowercase(),
                c.path,
                c.note
                    .as_ref()
                    .map(|n| format!("  ({n})"))
                    .unwrap_or_default()
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

// -------------------------------------------------------------- utilities

pub fn render_utilities(
    catalog: &UtilityCatalog,
    all: bool,
    category: Option<UtilityCategory>,
    search: Option<&str>,
) -> String {
    let search = search.map(str::to_lowercase);
    let mut out = String::new();
    let mut current = None;
    let mut shown = 0;
    for u in &catalog.utilities {
        if (!all && !u.installed)
            || category.is_some_and(|c| c != u.category)
            || search.as_ref().is_some_and(|s| {
                !u.name.to_lowercase().contains(s) && !u.id.to_lowercase().contains(s)
            })
        {
            continue;
        }
        if current != Some(u.category) {
            out.push_str(&format!("\n{}\n", u.category.label().to_uppercase()));
            current = Some(u.category);
        }
        shown += 1;
        let state = if !u.installed {
            "not installed".to_string()
        } else if u.launch_available {
            "can open".to_string()
        } else {
            "command line".to_string()
        };
        out.push_str(&format!(
            "  {:<28} {:<14} {:<34} {}\n",
            u.name,
            state,
            u.id,
            u.version.as_deref().unwrap_or("")
        ));
    }
    if shown == 0 {
        out.push_str("No utilities match.\n");
    }
    out
}

pub fn utilities(
    sentinel: &Sentinel,
    as_json: bool,
    all: bool,
    category: Option<&str>,
    search: Option<&str>,
) -> Result<ExitCode> {
    let category = match category {
        Some(text) => Some(UtilityCategory::parse(text).ok_or_else(|| {
            SentinelError::InvalidConfig(format!(
                "unknown category {text:?}; use one of: {}",
                UtilityCategory::ALL
                    .iter()
                    .map(|c| serde_json::to_value(c)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?),
        None => None,
    };
    let view: UtilitiesView = sentinel.utilities()?;
    if as_json {
        json(&view)?;
        return Ok(ExitCode::SUCCESS);
    }
    match &view.catalog {
        Some(catalog) => {
            println!(
                "{}",
                origin_line(view.origin, view.source, catalog.discovered_at)
            );
            let installed = catalog.utilities.iter().filter(|u| u.installed).count();
            println!(
                "{installed} of {} known utilities are installed{}.",
                catalog.utilities.len(),
                if all {
                    ""
                } else {
                    " (use --all to list the rest)"
                }
            );
            print!("{}", render_utilities(catalog, all, category, search));
            println!(
                "\nOpen one with `sentinel utilities open <id>`. Sentinel never installs anything."
            );
            Ok(ExitCode::SUCCESS)
        }
        None => {
            println!("No utility catalogue yet. Run `sentinel scan` (the daemon must be running).");
            Ok(ExitCode::from(SentinelError::DaemonUnavailable.exit_code()))
        }
    }
}

pub fn services(sentinel: &Sentinel, as_json: bool, all: bool) -> Result<ExitCode> {
    let view = sentinel.utilities()?;
    let Some(catalog) = view.catalog else {
        println!("No service list yet. Run `sentinel scan` (the daemon must be running).");
        return Ok(ExitCode::from(SentinelError::DaemonUnavailable.exit_code()));
    };
    let services: Vec<_> = catalog
        .services
        .iter()
        .filter(|s| all || s.relevant_category.is_some())
        .collect();
    if as_json {
        json(&services)?;
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{}",
        origin_line(view.origin, view.source, catalog.discovered_at)
    );
    println!(
        "{} service(s){} — read-only listing; Sentinel never starts, stops or changes services.\n",
        services.len(),
        if all {
            ""
        } else {
            " related to hardware, power, storage and networking (use --all for every service)"
        }
    );
    for s in services {
        println!(
            "  {:<44} {:<6} {:<10} {:<10} {}",
            s.name,
            format!("{:?}", s.scope).to_lowercase(),
            s.active_state.as_deref().unwrap_or("-"),
            s.enabled_state.as_deref().unwrap_or("-"),
            s.description.as_deref().unwrap_or("")
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub fn open(sentinel: &Sentinel, id: &str) -> Result<ExitCode> {
    let utility = sentinel.launch_utility(id)?;
    println!("Opened {}.", utility.name);
    Ok(ExitCode::SUCCESS)
}

// ------------------------------------------------------------------- scan

pub fn scan(
    sentinel: &Sentinel,
    jobs: &[DiscoveryJob],
    wait: bool,
    as_json: bool,
) -> Result<ExitCode> {
    for job in jobs {
        sentinel.run_discovery(*job, true)?;
    }
    if !wait {
        let status = sentinel.discovery_status()?;
        if as_json {
            json(&status)?;
        } else {
            println!("Scan requested. Check progress with `sentinel scan --status`.");
        }
        return Ok(ExitCode::SUCCESS);
    }
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut code = 0u8;
    let mut finals = Vec::new();
    for job in jobs {
        if !as_json {
            println!("Scanning {}…", job.as_str());
        }
        let mut printed = 0;
        let progress = loop {
            let status = sentinel.discovery_status()?;
            let progress = status.get(*job).clone();
            if !as_json {
                for report in progress.modules.iter().skip(printed) {
                    let single = JobProgress {
                        modules: vec![report.clone()],
                        ..progress.clone()
                    };
                    print!("{}", render_modules(&single));
                }
                printed = progress.modules.len();
            }
            if !progress.state.is_active() {
                break progress;
            }
            if Instant::now() >= deadline {
                return Err(SentinelError::Timeout(
                    "waiting for the scan to finish".into(),
                ));
            }
            sleep(Duration::from_millis(150));
        };
        code = code.max(match progress.state {
            JobState::Completed => 0,
            JobState::Cancelled => EXIT_CANCELLED,
            JobState::TimedOut => SentinelError::Timeout(String::new()).exit_code(),
            _ => 1,
        });
        if !as_json {
            match progress.state {
                JobState::Completed => println!(
                    "Done{}.\n",
                    match progress.cache_saved {
                        Some(true) => "; saved to the local cache",
                        Some(false) => "; the result could not be cached (see `sentinel logs`)",
                        None => "",
                    }
                ),
                _ => println!(
                    "{}\n",
                    progress
                        .error
                        .as_deref()
                        .unwrap_or("The scan did not complete.")
                ),
            }
        }
        finals.push(progress);
    }
    if as_json {
        json(&finals)?;
    }
    Ok(ExitCode::from(code))
}

pub fn scan_status(sentinel: &Sentinel, as_json: bool) -> Result<ExitCode> {
    let status = sentinel.discovery_status()?;
    if as_json {
        json(&status)?;
        return Ok(ExitCode::SUCCESS);
    }
    for job in DiscoveryJob::ALL {
        let p = status.get(job);
        println!(
            "{}: {}{}{}",
            job.as_str(),
            format!("{:?}", p.state).to_lowercase(),
            p.current_module
                .map(|m| format!(", now: {}", m.label()))
                .unwrap_or_default(),
            p.last_successful_scan
                .map(|t| format!(", last successful scan {}", when(t)))
                .unwrap_or_default()
        );
        print!("{}", render_modules(p));
        if let Some(error) = &p.error {
            println!("  {error}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

pub fn cancel(sentinel: &Sentinel, jobs: &[DiscoveryJob]) -> Result<ExitCode> {
    for job in jobs {
        sentinel.cancel_discovery(*job)?;
    }
    println!("Cancellation requested. Previous results are kept.");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_inventory::Confidence;

    #[test]
    fn formatting() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(8 * 1024 * 1024 * 1024), "8.0 GiB");
        assert_eq!(ghz(4_151_730), "4.15 GHz");
        assert!(capacity(512_110_190_592).starts_with("512.1 GB"));
    }

    #[test]
    fn missing_values_are_explained_never_zero() {
        let denied: Field<u64> = Field::missing(Availability::PermissionDenied, None);
        assert_eq!(
            show(&denied, |b| bytes(*b)),
            "not available (administrator only)"
        );
        let placeholder: Field<String> = Field::missing(Availability::Invalid, None);
        assert!(text(&placeholder).contains("placeholder"));
        let present = Field::detected(3u32, "x", Confidence::Direct);
        assert_eq!(show(&present, u32::to_string), "3");
    }
}
