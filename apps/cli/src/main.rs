//! `sentinel`: the Sentinel command-line interface.
//!
//! Every command goes through `sentinel-client`, the same backend interface
//! the desktop app uses.

mod commands;
mod docs;
mod inventory;
mod logs;
mod monitor;

use std::io::Write;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use sentinel_client::Sentinel;

#[derive(Parser)]
#[command(
    name = "sentinel",
    version,
    about = "Sentinel: Linux system intelligence hub (read-only by design)",
    long_about = "Sentinel observes, analyses and recommends. It never changes your system.\n\n\
                  It discovers your hardware and installed system utilities when you ask (`sentinel \
                  scan`) and monitors the system when you turn monitoring on (`sentinel monitor \
                  start`).",
    after_help = "Exit codes: 0 success, 1 general error, 2 invalid arguments, 3 daemon not \
                  running, 4 IPC failure, 5 invalid configuration, 6 permission denied, \
                  7 unsupported environment, 8 launch failure, 9 timeout, 11 request refused by \
                  the daemon, 12 scan cancelled, 13 monitoring is off."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the Sentinel daemon under your user account
    Start,
    /// Ask the Sentinel daemon to shut down gracefully
    Stop,
    /// Show whether the daemon is running, and its details
    Status {
        /// Print machine-readable JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Launch the Sentinel desktop application
    Ui,
    /// Show Sentinel's own log (never the system's logs)
    Logs {
        /// Keep printing new entries as they are written
        #[arg(short, long)]
        follow: bool,
        /// Number of most recent entries to show
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// Print the raw JSON log lines
        #[arg(long)]
        json: bool,
    },
    /// Show the hardware inventory from the last scan
    Hardware {
        #[command(subcommand)]
        action: Option<HardwareAction>,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show installed system utilities from the last scan
    Utilities {
        #[command(subcommand)]
        action: Option<UtilitiesAction>,
        /// Include known utilities that are not installed
        #[arg(long)]
        all: bool,
        /// Only this category, e.g. `system_monitoring`
        #[arg(long)]
        category: Option<String>,
        /// Only utilities whose name contains this text
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Control monitoring
    Monitor {
        #[command(subcommand)]
        action: MonitorAction,
    },
    /// Show live measurements (monitoring must be on)
    Metrics {
        /// cpu, memory, processes, battery, thermal, gpu, storage or network
        section: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show active alerts, or the alert history
    Alerts {
        #[command(subcommand)]
        action: Option<AlertsAction>,
        /// Show recent and past alerts, not only active ones
        #[arg(long)]
        history: bool,
        #[arg(long)]
        json: bool,
    },
    /// Motherboard documentation: show results, search, download
    Docs {
        #[command(subcommand)]
        action: Option<DocsAction>,
        /// Include documents for other hardware
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
    /// Scan hardware and utilities (read-only), or show or cancel a scan
    Scan {
        /// Only the hardware inventory
        #[arg(long, conflicts_with = "utilities")]
        hardware: bool,
        /// Only the utility catalogue
        #[arg(long)]
        utilities: bool,
        /// Show progress of the current or last scan
        #[arg(long, conflicts_with_all = ["cancel", "no_wait"])]
        status: bool,
        /// Stop a running scan; previous results are kept
        #[arg(long)]
        cancel: bool,
        /// Return immediately instead of waiting for the result
        #[arg(long)]
        no_wait: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum MonitorAction {
    /// Turn monitoring on (and keep it on after restarts)
    Start,
    /// Turn monitoring off (history is kept)
    Stop,
    /// Pause sampling temporarily
    Pause,
    /// Resume sampling after a pause
    Resume,
    /// Show the monitoring state and collectors
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Choose the sampling profile
    Profile {
        #[arg(value_parser = ["eco", "balanced", "performance"])]
        profile: String,
        /// Use Eco automatically while on battery
        #[arg(long, value_parser = ["on", "off"])]
        power_aware: Option<String>,
    },
}

#[derive(Subcommand)]
enum AlertsAction {
    /// Mark an alert as seen (changes nothing on the system)
    Ack { id: u64 },
}

#[derive(Subcommand)]
enum HardwareAction {
    /// Which Linux information sources exist and are readable
    Capabilities {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum DocsAction {
    /// Search for documentation (reuses recent results unless --refresh)
    Search {
        #[arg(long)]
        refresh: bool,
        /// Return immediately instead of waiting for the result
        #[arg(long)]
        no_wait: bool,
        #[arg(long)]
        json: bool,
    },
    /// Stop a running search or download
    Cancel,
    /// Download a document (official sources only; online discovery must be on)
    Download { id: u64 },
    /// Delete the downloaded copy of a document
    Remove { id: u64 },
    /// Forget search results (downloaded copies are kept)
    Clear,
}

#[derive(Subcommand)]
enum UtilitiesAction {
    /// Open an installed utility (applications with a desktop entry only)
    Open {
        /// Utility id as shown by `sentinel utilities`
        id: String,
    },
    /// List system and user services (read-only)
    Services {
        /// Every service, not only those related to hardware and the system
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = Sentinel::from_env().and_then(|sentinel| match cli.command {
        Command::Start => commands::start(&sentinel),
        Command::Stop => commands::stop(&sentinel),
        Command::Status { json } => commands::status(&sentinel, json),
        Command::Ui => commands::ui(),
        Command::Logs {
            follow,
            lines,
            json,
        } => logs::run(&sentinel.paths().daemon_log_file(), lines, follow, json),
        Command::Hardware { action, json } => match action {
            None => inventory::hardware(&sentinel, json),
            Some(HardwareAction::Capabilities { json }) => inventory::capabilities(&sentinel, json),
        },
        Command::Utilities {
            action,
            all,
            category,
            search,
            json,
        } => match action {
            None => {
                inventory::utilities(&sentinel, json, all, category.as_deref(), search.as_deref())
            }
            Some(UtilitiesAction::Open { id }) => inventory::open(&sentinel, &id),
            Some(UtilitiesAction::Services { all, json }) => {
                inventory::services(&sentinel, json, all)
            }
        },
        Command::Monitor { action } => {
            use sentinel_core::MonitoringCommand as M;
            match action {
                MonitorAction::Start => monitor::control(&sentinel, M::Start),
                MonitorAction::Stop => monitor::control(&sentinel, M::Stop),
                MonitorAction::Pause => monitor::control(&sentinel, M::Pause),
                MonitorAction::Resume => monitor::control(&sentinel, M::Resume),
                MonitorAction::Status { json } => monitor::status(&sentinel, json),
                MonitorAction::Profile {
                    profile,
                    power_aware,
                } => {
                    use sentinel_telemetry::model::SamplingProfile as P;
                    let p = match profile.as_str() {
                        "eco" => P::Eco,
                        "performance" => P::Performance,
                        _ => P::Balanced,
                    };
                    monitor::profile(&sentinel, p, power_aware.map(|v| v == "on"))
                }
            }
        }
        Command::Metrics { section, json } => monitor::metrics(&sentinel, section.as_deref(), json),
        Command::Alerts {
            action,
            history,
            json,
        } => match action {
            Some(AlertsAction::Ack { id }) => monitor::acknowledge(&sentinel, id),
            None => monitor::alerts(&sentinel, history, json),
        },
        Command::Docs { action, all, json } => match action {
            None => docs::show(&sentinel, all, json),
            Some(DocsAction::Search {
                refresh,
                no_wait,
                json,
            }) => docs::search(&sentinel, refresh, no_wait, json),
            Some(DocsAction::Cancel) => docs::act(sentinel.cancel_document_discovery()),
            Some(DocsAction::Download { id }) => docs::act(sentinel.download_document(id)),
            Some(DocsAction::Remove { id }) => docs::act(sentinel.remove_document_file(id)),
            Some(DocsAction::Clear) => docs::act(sentinel.clear_document_cache()),
        },
        Command::Scan {
            hardware,
            utilities,
            status,
            cancel,
            no_wait,
            json,
        } => {
            use sentinel_inventory::DiscoveryJob;
            let jobs: Vec<DiscoveryJob> = match (hardware, utilities) {
                (true, false) => vec![DiscoveryJob::Hardware],
                (false, true) => vec![DiscoveryJob::Utilities],
                _ => DiscoveryJob::ALL.to_vec(),
            };
            if status {
                inventory::scan_status(&sentinel, json)
            } else if cancel {
                inventory::cancel(&sentinel, &jobs)
            } else {
                inventory::scan(&sentinel, &jobs, !no_wait, json)
            }
        }
    });

    match result {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "sentinel: {error}");
            if let Some(hint) = commands::hint_for(&error) {
                let _ = writeln!(std::io::stderr(), "  {hint}");
            }
            ExitCode::from(error.exit_code())
        }
    }
}
