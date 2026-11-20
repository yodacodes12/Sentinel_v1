//! `sentineld`: the Sentinel background daemon.
//!
//! Normally started by `sentinel start` or the desktop app. It can also be
//! run directly, or by the optional systemd user unit, with `--foreground`.

use std::process::ExitCode;

use clap::Parser;
use sentinel_core::AppPaths;
use sentinel_daemon::{DaemonOptions, run};

#[derive(Parser)]
#[command(
    name = "sentineld",
    version,
    about = "Sentinel background daemon. Runs as your user; never as root."
)]
struct Args {
    /// Also write log lines to standard error.
    #[arg(long)]
    foreground: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let result = AppPaths::from_env().and_then(|paths| {
        run(DaemonOptions {
            paths,
            log_to_stderr: args.foreground,
            discovery_root: std::env::var_os(sentinel_daemon::run::DISCOVERY_ROOT_ENV)
                .filter(|v| !v.is_empty())
                .map(Into::into),
        })
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // The launcher may have closed stderr; a failed write is ignored
            // rather than allowed to panic.
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "sentineld: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
