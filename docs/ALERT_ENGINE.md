# Alert engine

Alerts tell the user about a sustained condition, with the evidence, why it matters and what
they could consider doing. They never act: there is no automatic fix, and a notification has no
button that does anything. Code: `crates/sentinel-alerts`.

## How a condition becomes an alert

On every monitoring pass, each rule (`rules.rs`) looks at the latest snapshot and produces an
**observation** per subject (a sensor, a disk, a mount point, a battery): *triggered* with a
severity, *clear*, inside the *hysteresis band*, or *unknown* (no data). The state machine in
`evaluator.rs` keeps one state per `(rule, subject)`:

```text
Normal --triggered--> Pending --held for the rule's duration--> Active  (raised, notified)
Pending --clear or band--> Normal                                        (a spike is not an alert)
Active --triggered at a higher severity--> Active                        (escalated, notified again)
Active --in the band--> Active                                           (no flapping at the threshold)
Active --clear for the rule's clear time--> Resolved                     (recovery event)
any --unknown--> unchanged                                               (missing data decides nothing)
```

- **Sustained duration:** most rules must hold for `sustain_s` (default 60 s); CPU for
  `cpu_high_duration_s` (default 120 s); load, disk activity and battery discharge for 5 minutes.
- **Hysteresis:** each rule clears at a lower level than it triggers (the band below), and most
  also need the clear condition to last 30 or 60 seconds.
- **Duplicate suppression:** one alert per condition at a time; a continuing condition updates
  the existing alert's evidence and time.
- **Cooldown:** a condition that resolved less than `cooldown_s` ago (default 600 s) and returns
  is recorded but not notified again. Escalation always notifies. The cooldown survives
  restarts: when monitoring starts, alerts resolved within the cooldown (including those just
  closed because the previous session ended) are remembered, so restarting the daemon or turning
  monitoring off and on does not repeat a notification the user has just seen.
- **Recovery:** resolution is an event; the alert keeps its history with `resolved_at`.
- **Pause and restart:** pending conditions are forgotten on pause. Alerts left active by a
  previous daemon are closed at the next start with a note and evaluated again from scratch.
- **Acknowledging** marks the alert as seen. It changes nothing else.

## Rules and default thresholds

All thresholds are in the `alerts` section of `config.toml` and on the Settings page.

| Rule | Triggers | Severity | Clears (band) | Holds | Clear time |
|---|---|---|---|---|---|
| `cpu_high` | total CPU ≥ 90 % | warning | < 75 % | 120 s | 30 s |
| `load_high` | 1-min load ≥ 2.0 × logical CPUs | warning | < 1.5 × | 5 min | 60 s |
| `memory_low` | available ≤ 10 % (≤ 5 % critical) | warning / critical | > 15 % | 60 s | 30 s |
| `memory_pressure` | PSI memory `some avg60` ≥ 20 % | warning | < 10 % | 60 s | 60 s |
| `swap_activity` | swap in + out ≥ 500 pages/s | warning | < 250 | 60 s | 60 s |
| `temperature_high` | ≥ the sensor's driver-reported max, or the user's threshold; ≥ the driver's critical limit is critical | warning / critical | 5 °C below | 60 s (critical: 10 s) | 30 s |
| `battery_low` | ≤ 15 % while discharging (≤ 5 % critical) | warning / critical | > 20 % or not discharging | 2 battery readings (one battery interval: 10–30 s) | immediate |
| `battery_health` | estimated health ≤ 60 % of design | info | > 62 % | immediate | immediate |
| `storage_low` | free ≤ 10 % on a writable local filesystem (≤ 5 % critical) | warning / critical | > 12 % | 2 readings | immediate |
| `disk_busy` | a disk ≥ 90 % busy | warning | < 70 % | 5 min | 60 s |
| `battery_temperature` | the battery's own sensor ≥ the user's threshold (off by default) | warning | 3 °C below | 60 s | 30 s |
| `battery_discharge_high` | discharge ≥ the user's limit in W (off by default) | warning | < 80 % of it | 5 min | 60 s |
| `network_high` | total traffic ≥ the user's limit in Mbit/s (off by default) | info | < 75 % of it | 60 s | 60 s |

**Temperatures.** Sentinel has no built-in "safe temperature": what is normal depends on the part.
A temperature alert uses the driver's own `temp*_max` and `temp*_crit` or the firmware's critical
trip point, and otherwise only a threshold the user set (`temperature_warning_c`). A sensor with
neither gets no alert, and the alert says which source the threshold came from (`driver` or
`user`). Battery sensors are left to `battery_temperature`.

**Storage activity** is labelled as activity, not as a health problem. Disk health (SMART) is not
read in Phase 3.

## What an alert contains

`id`, `rule`, `category`, `severity` (info, warning, critical), `title`, `explanation`,
`recommendation` (text only), `metric` (the history series behind it), `subject`, `threshold`,
`threshold_source` (default, user, driver), `evidence` (the values in words, e.g. "3.0% of memory
available."), `detected_at`, `last_updated_at`, `resolved_at`, `status` (active, resolved),
`acknowledged`, `notified`.

Recommendations are phrased as things the user could consider ("Review the applications using the
most memory..."). None of them is a command, and Sentinel never carries one out.

## Notifications

`notifications.rs` delivers a raised or escalated alert, if the evaluator marked it for
notification and the user has notifications enabled, through the freedesktop notification service
on the user's session D-Bus (`dbus.rs`: connect, `EXTERNAL` authentication, `Hello`, one `Notify`
call, read the reply). The bus is found from `DBUS_SESSION_BUS_ADDRESS` or
`$XDG_RUNTIME_DIR/bus`. Urgency follows the severity. Markup is escaped, control characters are
removed, and there are no actions.

A failure (no bus, no notification service, a refusal, a 3-second timeout) is counted, the last
error is kept in the monitoring status, the alert stays in the history, and monitoring continues.
Notifications are never retried in a loop.

## Storage

Every raise, escalation, acknowledgement and resolution is written to the `alerts` table of
`history.db` as it happens. Alerts are kept for `retention.alert_days` (default 90). The Alerts page
and `sentinel alerts --history` show the current session's recent alerts merged with the stored
ones.

## Tests

- `evaluator.rs`: a short spike is not an alert; raise once, band, recovery; missing data changes
  nothing; escalation notifies again while the cooldown suppresses a repeat; acknowledging touches
  only that alert; a condition from an earlier session is not re-notified within the cooldown.
- `tests/rules.rs` (Phase 4): every one of the 13 rules through the real engine with synthetic
  snapshots: no alert for a spike shorter than the hold, exactly one raise at the right severity
  after it, no duplicates while the condition lasts, missing data changes nothing, no flapping in
  the band, resolution only after the clear time, a repeat inside the cooldown is recorded but not
  notified, critical levels and escalation, plus the rule-specific cases (driver or user limits
  for temperatures, battery sensors left to the battery rule, read-only filesystems and loopback
  traffic ignored, opt-in rules off by default, battery alerts only while discharging and only
  after two battery readings, repeated temperature warnings summarised once).
- `notifications.rs`, `dbus.rs`: message encoding and delivery. (`health.rs` is exercised through
  the integration tests; it has no unit tests of its own.)
- `tests/integration/telemetry.rs`: the real daemon raises one `memory_low` alert on a synthetic
  machine, sends exactly one notification to a fake session bus, keeps it active inside the band,
  resolves it only after 30 s clear, and never notifies the recovery; delivery failure is
  recorded; disabled notifications are never sent.
- Real desktop: a controlled alert delivered through GNOME Shell's notification service exactly
  once, see [NOTIFICATION_VALIDATION.md](NOTIFICATION_VALIDATION.md).
