//! Per-interface traffic from `/sys/class/net/*/statistics` counter
//! deltas. Only counters: no packet is captured or inspected.

use std::collections::BTreeMap;

use sentinel_hardware::SysRoot;

use super::{RunReport, SampleTime};
use crate::counters::rate;
use crate::model::{InterfaceTraffic, NetworkReport, Reading, ReadingState, round1};

#[derive(Default)]
pub struct NetworkCollector {
    prev: BTreeMap<String, (f64, [u64; 4])>,
}

impl NetworkCollector {
    pub fn sample(&mut self, root: &SysRoot, t: SampleTime) -> (Option<NetworkReport>, RunReport) {
        let Ok(names) = root.list("sys/class/net") else {
            return (
                None,
                RunReport::unsupported("/sys/class/net is not available"),
            );
        };
        let mut interfaces = Vec::new();
        let mut next = BTreeMap::new();
        for name in names {
            let dir = format!("sys/class/net/{name}");
            let stat = |f: &str| {
                root.read_trimmed(&format!("{dir}/statistics/{f}"))
                    .ok()
                    .and_then(|v| v.parse::<u64>().ok())
            };
            let (Some(rx), Some(tx)) = (stat("rx_bytes"), stat("tx_bytes")) else {
                continue;
            };
            let (rxp, txp) = (
                stat("rx_packets").unwrap_or(0),
                stat("tx_packets").unwrap_or(0),
            );
            let cur = [rx, tx, rxp, txp];
            let prev = self.prev.get(&name);
            let r = |i: usize| match prev {
                Some((pt, p)) => match rate(p[i], cur[i], t.mono_s - pt) {
                    Some(v) => Reading::calculated(round1(v)),
                    // Counter reset (interface re-created): skip this interval.
                    None => Reading::missing(ReadingState::Error),
                },
                None => Reading::pending(),
            };
            interfaces.push(InterfaceTraffic {
                loopback: root
                    .read_trimmed(&format!("{dir}/type"))
                    .map(|v| v == "772")
                    .unwrap_or(false),
                operational_state: root.read_trimmed(&format!("{dir}/operstate")).ok(),
                rx_bps: r(0),
                tx_bps: r(1),
                rx_packets_per_s: r(2),
                tx_packets_per_s: r(3),
                rx_total_bytes: rx,
                tx_total_bytes: tx,
                rx_errors: stat("rx_errors").unwrap_or(0),
                tx_errors: stat("tx_errors").unwrap_or(0),
                rx_dropped: stat("rx_dropped").unwrap_or(0),
                tx_dropped: stat("tx_dropped").unwrap_or(0),
                name: name.clone(),
            });
            next.insert(name, (t.mono_s, cur));
        }
        // Interfaces that disappeared are dropped from the state.
        self.prev = next;
        (
            Some(NetworkReport {
                measured_at: t.wall_ms,
                interval_ms: t.interval_ms,
                interfaces,
            }),
            RunReport::ok(),
        )
    }
}
