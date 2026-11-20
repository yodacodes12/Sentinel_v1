# Sentinel safety policy

Sentinel is an observer, analyser and recommender. It must never modify the user's operating
system or take corrective action. This applies to the daemon, the CLI, the desktop application
and every future module.

## What Sentinel must never do

- Install, remove, update or upgrade software or the operating system.
- Modify system configuration files, kernel parameters, the bootloader or firewall rules.
- Change BIOS, firmware, CPU, GPU, power-management or network settings.
- Enable, disable, start or stop system services.
- Kill, terminate, suspend, renice or otherwise affect the user's processes, including from the
  process list.
- Change CPU frequency governors, fan curves (`pwm*`), battery charge thresholds or any other
  writable sysfs control, even when the monitor shows the related value.
- Partition, format, mount or repair disks or filesystems.
- Change permissions of files it does not own.
- Run maintenance scripts, apply recommended fixes or execute repair commands.
- Run as root, or escalate privileges.

## What Sentinel may do

- Read system information through read-only interfaces: sysfs, procfs, os-release, ID databases,
  installed-package databases, desktop entries, and read-only systemd listings.
- Sample those interfaces while the user has turned monitoring on, and store the numbers in its
  own history database.
- Show a desktop notification for an alert, through the user's session bus, when the user allows
  notifications. Notifications have no buttons that act on anything.
- Open an installed application through the desktop's own launcher when the user clicks Open.
- Explain what it found, the evidence, why it matters and what the user could consider doing.
- Write its own configuration, logs, cache and database inside its own XDG directories.
- Start and stop its own daemon, at the user's request.

## Failing safely

When something is unsupported, inaccessible or fails, Sentinel reports the limitation, logs it,
and stops there. It does not try a different command that changes the system, does not ask for
more privileges, and does not invent information.

## How this is enforced

| Rule | Mechanism | Verified by |
|---|---|---|
| Never runs as root | `ensure_not_root` in the daemon, the client `start` path and the desktop shell | unit test; safety test checks the daemon's uids and capabilities in `/proc` |
| Starts no processes except its own | One launcher module; it accepts only Sentinel's two binary names; no shell | `processes_are_only_launched_by_the_launcher`; the daemon's `/proc/<pid>/task/*/children` is empty |
| Never signals other processes | `sentinel stop` uses an IPC request; there is no signal-sending code | source scan; a bystander process survives a full start/stop cycle |
| No package managers, service managers or disk tools | None are referenced anywhere | `no_forbidden_capability_appears_in_the_sources` |
| Writes only its own files | All paths come from `AppPaths`; directories verified as owned and private | `only_sentinels_own_files_are_written` lists every file created in a sandboxed home |
| Never deletes files it does not recognise | Stale-socket removal checks the file type; corrupt configuration and databases are preserved | unit and integration tests |
| No network listener | The only socket is a Unix socket | `daemon_opens_no_network_sockets` checks every socket the daemon holds |
| No arbitrary commands over IPC | Closed request enum with one typed payload | `ipc_protocol_has_no_open_ended_requests` pins the enum |
| Monitoring state is accurate | Requests succeed only once the state was reached; status reports the engine, not the preference | `monitoring_state_is_reported_accurately`; frontend tests |
| Monitoring off means no collection | The monitor thread exits; the daemon returns to one thread and makes no reads | `monitoring_off_stops_collection_and_the_daemon_goes_quiet`; `daemon_is_idle_when_nothing_talks_to_it` |
| No placeholder data shown as real | Hardware cards carry no values; the board is labelled illustrative on the drawing itself | frontend tests; `status_contains_no_telemetry_fields` |
| No fabricated measurements | Every metric is a `Reading` with a state; missing sources are `unsupported`, never `0`; rates need two samples | `no_sensors_means_unsupported_not_zero`, `a_failing_collector_does_not_stop_the_others`, frontend `telemetry.test.tsx` |
| No private process data | Reads via `SysRoot` (refuses `environ`); command line cut at the program name; processes never stored | `process_cpu_and_memory_are_calculated_and_arguments_are_never_kept`; integration `monitoring_reports_the_values_of_the_machine_it_reads` |
| Telemetry is read-only | No write API in `sentinel-telemetry`; `pwm*` and other control files are never opened | `discovery_crates_never_write_or_change_anything` (covers telemetry); `temperatures_and_fan_speeds_are_parsed_and_malformed_values_rejected` |
| Notifications cannot act | One D-Bus method, no actions | `notify_message_is_well_formed`; integration tests with a fake session bus |
| Nothing enabled at install | The systemd unit is optional and manual; nothing references it | `systemd_unit_is_a_manual_unprivileged_user_unit` |
| No unsafe code | `unsafe_code = "forbid"` for the workspace | `no_crate_uses_unsafe_code` |
| Discovery is read-only | No write API in the discovery crates; reads via `SysRoot` | `discovery_crates_never_write_or_change_anything`; `scanning_changes_no_system_file` |
| Discovery stays in its lane | Allow-listed read locations; no filesystem walk | `real_machine_discovery_reads_only_expected_locations` |
| Services are only listed | Four fixed `systemctl list-*` argument vectors, env cleared, never killed | `systemd_queries_only_list_and_never_change_anything` |
| Discovered programs are never run | Existence by `stat` only | `utility_discovery_never_runs_what_it_finds` |
| No identifiers leak | Serial/UUID files refused at the reader; MACs never logged | `serial_numbers_and_uuids_are_never_read`; `discovery_logs_contain_no_machine_identifiers` |
| No network during discovery | No network code | `discovery_makes_no_network_connections_while_scanning` |
| Network only for opt-in document discovery | One module (`sentinel-docs/src/net.rs`) may use HTTPS; online discovery and automatic downloads default to off; public HTTPS targets only | `network_code_is_confined_to_document_discovery`; `daemon_opens_no_network_sockets` now also runs a (default, offline) document search; `net.rs` and `download.rs` unit tests |
| Downloaded documents are never run | Content-sniffed, executables rejected, archives never extracted, nothing opened automatically | `download.rs` and `classify.rs` tests |
| No fabricated hardware | Values exist only when detected; no hard-coded product names in the UI | frontend `inventory.test.tsx`; hardware fixture tests |

The source scans are deliberately strict. Phase 2 needed system paths in the discovery crates
(Phase 3 adds the telemetry crate, which names `/proc` and `/sys` in its messages),
`systemctl` in one module, and program names in the utility catalogue; each exemption is listed in
`no_forbidden_capability_appears_in_the_sources` with its reason and backed by a narrower test.
Any further exemption must be made the same way.

## For contributors

Before adding a capability, ask whether it only reads. If a feature would need elevated
privileges to read something, it must be designed separately, with narrowly scoped access and a
written security review. Phase 1 has no privileged helper and none may be added casually.
