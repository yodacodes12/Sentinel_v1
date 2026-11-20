//! Take three samples of every collector, two seconds apart, and print the
//! last snapshot as JSON: `cargo run -p sentinel-telemetry --example sample`.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sentinel_telemetry::collectors::SampleTime;
use sentinel_telemetry::{CollectorKind, Engine};

fn main() {
    let mut engine = Engine::for_host();
    let start = Instant::now();
    for round in 0..3 {
        if round > 0 {
            std::thread::sleep(Duration::from_secs(2));
        }
        for kind in CollectorKind::ALL {
            let wall_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let t = SampleTime {
                wall_ms,
                mono_s: start.elapsed().as_secs_f64(),
                interval_ms: 2000,
            };
            engine.run(kind, t);
        }
    }
    let out = serde_json::json!({
        "snapshot": engine.snapshot(),
        "collectors": engine.statuses(),
        "process_count": engine.process_list().map(|l| l.processes.len()),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&out).expect("serialises")
    );
}
