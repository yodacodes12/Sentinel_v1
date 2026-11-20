# Notification validation (Phase 4)

Whether an alert really reaches the Linux desktop, and only as often as it should.

## Environment

Dell Inspiron 15 3535 laptop, Ubuntu 26.04, kernel 7.0, GNOME 50.1 on Wayland. Notification
service: `org.freedesktop.Notifications` owned by `gnome-shell` (`GetServerInformation`:
gnome-shell, GNOME, 50.1, spec 1.2). Release build. Run on 2026-11-19.

## Method

`tests/desktop-e2e/notifications.py` runs the release daemon in a throwaway home with the user's
**real** session bus. The alert is controlled and harmless: the low-memory threshold is set to
99 % of memory available (the real reading, about 49 %, is "below" it), with `sustain_s = 5` to
keep the run short. No memory is consumed and nothing on the system changes; no hardware
condition is created. `dbus-monitor` (no privileges needed on the session bus) records every
`Notify` call, and GNOME Shell's accessibility tree is searched for the banner.

## Results

| Step | Required | Result | How verified |
|---|---|---|---|
| 1. Controlled test alert | Generate one safely | Threshold raised over IPC; nothing else changed | Script |
| 2. Backend detects it | | `memory_low` raised 8.0 s after the change (5 s hold + first passes) | `GetActiveAlerts` |
| 3. Alert persisted | | Written to the `alerts` table on raise | Integration test `sustained_low_memory_raises_one_alert_notifies_and_resolves` (reads `history.db`); `sqlite3` is not installed on this machine, so the script did not re-read it |
| 4. Notification integration invoked | | One `Notify` from Sentinel's connection: app name "Sentinel", title "Sentinel — Low available memory", body "Available memory has remained below 99% of total for at least 5 seconds. …", urgency hint | `dbus-monitor` capture |
| 5. **Desktop displays it** | Physically | **Yes.** The service accepted it (`delivered = 1`, `last_delivery_ok = true`); GNOME Shell's own accessibility tree contained the banner (a label "Low available memory") | Reply from the service; GNOME Shell's accessibility tree. No screenshot was taken (Wayland does not allow unattended screenshots), and no person was asked to confirm what they saw. |
| 6. Failure does not crash the daemon | | No bus at all, or no notification service: recorded (`failed`, `last_error`), monitoring continues | Integration test `a_missing_notification_service_is_recorded_and_monitoring_continues`; also every isolated test run in Phase 4 (no bus), e.g. the real `battery_low` alert during the long run: "notification could not be delivered; recorded locally" |
| 7. Not excessively noisy | | While active for 40 s: no repeat. On recovery: none. Raised again inside the cooldown: recorded, `notified = false`, no `Notify`. After a daemon restart with the condition still present: `notified = false`, no `Notify` | `dbus-monitor` count stayed at one Sentinel call throughout |

`dbus-monitor` shows a second `Notify` for the same banner, from GNOME Shell's own connection
(`sender=:1.33` with `x-shell-sender` set to Sentinel's connection): that is GNOME relaying it
internally, not a second call from Sentinel.

Step 7's restart case is the Phase 4 fix: before it, a condition still present after a daemon
restart (or monitoring off and on) raised a fresh alert that notified again at once, because the
cooldown memory started empty.

## Permissions and support

- Desktop notifications need no permission on GNOME for a session-bus client; nothing had to be
  allowed. Sentinel uses only `Notify`; it registers no actions, so a notification can never
  make Sentinel do anything.
- Where no notification service runs (a minimal window manager, a plain SSH session, a daemon
  started outside the desktop session), delivery fails, the failure and its reason are shown in
  `sentinel monitor status` and the desktop's monitoring status, and the alert is still recorded.
- The user can turn notifications off (Settings, or `notifications_enabled = false`); disabled
  notifications are never sent (integration test `disabled_notifications_are_never_sent`).

## Not tested

- KDE Plasma, XFCE, Cinnamon and other notification servers: not tested (no such desktop was
  available). They implement the same freedesktop interface.
- Do Not Disturb: GNOME suppresses banners while it is on; Sentinel cannot tell and does not try.
