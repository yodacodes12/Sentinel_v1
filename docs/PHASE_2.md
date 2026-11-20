# Phase 2 engineering report

Status: **implemented and tested; the desktop shell is still uncompiled.** Hardware discovery,
the system inventory, utility and service discovery, caching, the IPC/CLI extensions and the
desktop pages are built and pass every test. The Tauri shell's new commands are written, but the
shell still cannot be compiled on the development machine because the WebKitGTK development
packages are not installed (unchanged from Phase 1). The new pages were checked by rendering the
production frontend bundle in a browser with real data from this machine (see "Desktop").

Before starting, the Phase 1 suites were re-run and passed (148 Rust, 14 frontend).

## 1. Implemented modules

| Crate / area | Contents |
|---|---|
| `sentinel-inventory` (new) | `Field<T>` provenance type, hardware records, `BoardIdentity` contract, utility catalogue types, discovery progress, hardware fingerprint, cache format |
| `sentinel-hardware` (new) | `SysRoot` read-only reader, parsers (cpuinfo, meminfo, CPU lists, key/value, SMBIOS type 17, pci/usb.ids streaming, class codes, microarchitecture table), 12 modules: system identity, firmware, motherboard, CPU, memory, GPU, storage, battery/power, network, USB, PCI, sensors; manager; capability detection |
| `sentinel-utilities` (new) | Desktop entries, package databases (dpkg, RPM, pacman, Flatpak, Snap), `PATH` executables, systemd services; built-in catalogue of 64 tools in 15 categories |
| `sentinel-storage` | Migration 2: `scan_cache` table; read-only open for clients |
| `sentinel-ipc` | Nine new requests, four new responses; 16 MiB response limit; unknown request fields rejected |
| `sentinel-daemon` | Scan coordinator: one worker thread only while scanning, queue without duplicates, cancellation, 60 s limit, cache load/save |
| `sentinel-client` | Inventory and utility views (daemon, or cache file when stopped), scan control, utility launching via `gio launch` |
| CLI | `sentinel scan`, `sentinel hardware`, `sentinel hardware capabilities`, `sentinel utilities`, `sentinel utilities services`, `sentinel utilities open` |
| Desktop frontend | Overview with real data and a generated CPU topology; Hardware (ten categories with source and confidence); Motherboard (identity, no drawing); Installed Utilities; Services and Startup; device identity in the header |
| Desktop shell | Seven new commands (written, not compiled) |

## 2. Repository changes

New: `crates/sentinel-inventory`, `crates/sentinel-hardware` (with `examples/scan.rs`),
`crates/sentinel-utilities` (with `examples/utilities.rs`), `tests/hardware`,
`tests/support/hwfixture.rs`, frontend `types/inventory.ts`, `app/inventoryView.ts`,
`app/inventory.test.tsx`, `hooks/useInventory.ts`, `components/{CpuTopology,DiscoveryPanel,FieldRow}.tsx`,
`pages/{HardwarePage,MotherboardPage,UtilitiesPage,ServicesPage}.tsx`, and
`docs/{HARDWARE_DISCOVERY,SYSTEM_INVENTORY,UTILITY_DISCOVERY,PHASE_2}.md`.

Removed: the illustrative motherboard drawing (`MotherboardDiagram.tsx`, `BOARD_REGIONS`,
`HARDWARE_CARDS`). The plan forbids a fabricated board diagram, and with a real identity on screen a
generic drawing would mislead. The Overview's prominent panel is now the CPU topology, generated
from discovered data.

Changed: storage, IPC, daemon, client, CLI, Tauri commands, the safety suite (see 9), the README
and all existing docs.

## 3. Hardware discovery sources

Listed per module in [HARDWARE_DISCOVERY.md](HARDWARE_DISCOVERY.md). In short: `/sys/class/dmi/id`,
`/sys/devices/system/{cpu,memory,node}`, `/sys/firmware/{efi,dmi/entries}`, `/sys/bus/{pci,usb}/devices`,
`/sys/block`, `/sys/class/{power_supply,net,hwmon,thermal}`, `/proc/{cpuinfo,meminfo,sys/kernel}`,
`/etc/os-release` or `/usr/lib/os-release`, and `/usr/share/hwdata/{pci,usb}.ids`. A real-machine
scan touches 1,707 paths, all inside an allow-list checked by a test.

## 4. Hardware detected on the development machine

Actual results from this machine (not fixtures):

| Category | Detected |
|---|---|
| System | Dell Inc. Inspiron 15 3535, family Inspiron, laptop chassis, x86_64, kernel 7.0.0-43-generic, Ubuntu 26.04.1 LTS |
| Motherboard | Dell Inc. 0N90HF, revision A02 (firmware-reported, not verified); identification level "board identified" |
| Firmware | Dell Inc. BIOS 1.32.0 (09/02/2026), UEFI boot |
| Processor | AMD Ryzen 3 7320U with Radeon Graphics; 4 physical cores, 8 logical processors (cores 0–3 with CPUs 0/1, 2/3, 4/5, 6/7); microarchitecture Zen 2 (inferred); L1d 4×32 KiB, L1i 4×32 KiB, L2 4×512 KiB, L3 1×4 MiB; frequency limit 0.43–2.40 GHz at the time of the CLI run (4.15 GHz in an earlier run: the driver's limit changes with boost and power state, which the field now notes) |
| Memory | 6.5 GiB usable, 7.5 GiB kernel-managed, 9.0 GiB swap, one NUMA node |
| Graphics | AMD Mendocino [Radeon 610M] at 0000:03:00.0, integrated (inferred), amdgpu, 512 MiB system RAM reserved (not dedicated VRAM), card1 + renderD128 |
| Storage | nvme0n1 "EG6 KIOXIA 512GB", NVMe SSD, 512.1 GB, 7 partitions; 38 snap loop devices listed as virtual |
| Battery and power | BAT1 (BYD, DELL PG8YJ47J), health 82.0 % of design capacity (charge-based); AC adapter |
| Network | wlp2s0 Wi-Fi (rtw88_8821ce, PCI 0000:02:00.0); docker0 bridge; lo |
| USB | 5 root hubs on 3 xHCI controllers with port connect types; Realtek Bluetooth (3-2), Genesys USB 2.0 hub (3-3), integrated webcam (5-1) |
| PCI | 30 devices with names, drivers and upstream bridges |
| Sensors | 16 hwmon inputs on 6 chips (acpitz, BAT1, nvme, k10temp, dell_smm with a fan, amdgpu) and 1 thermal zone; all readable by the user; no values read |

## 5. Hardware information that was unavailable

| Information | Why | How it is shown |
|---|---|---|
| Installed RAM, memory type, speed, slots, modules | SMBIOS records are readable only by root | "Administrator only"; no estimate, no slot layout |
| Serial numbers, UUID, asset tags, battery PPID | Not wanted; never opened | Absent from the model |
| Battery technology | Firmware reports "Unknown" | "Not reported (placeholder or invalid)" |
| Battery cycle count | Firmware reports 0 | Shown as 0 with a note that 0 often means "not counted" |
| Names for two USB devices (Bluetooth, webcam) | IDs not in `usb.ids` | Device descriptor names shown; database name "unknown" |
| Current CPU frequency, sensor readings, SMART health | Out of scope for Phase 2 (telemetry, later phase) | Not collected |
| Physical USB socket mapping | Needs board documentation | Linux port numbers, labelled as such |

## 6. Utility discovery results

On this machine: 136 desktop files in six application directories, dpkg (2,414 installed packages)
and Snap (23 snaps) read; pacman, RPM and Flatpak databases not present. 72 utilities listed
(64 from the catalogue plus 8 system tools found through desktop entries), **36 installed**, for
example htop 3.4.1, Resources 1.10.2, GNOME Disks, Disk Usage Analyzer, Logs, Settings, Firmware
Updater, App Center, Déjà Dup, lshw, lspci, lsusb, lscpu, dmidecode, upower, nmcli, mtr, journalctl,
fwupdmgr. 18 of them can be opened (they have desktop entries); the rest are command-line tools,
shown but never run. Services: 432 in a normal session (317 system, 115 user), 55 marked relevant.
A full utility scan takes about 2.5 s, almost all in the four `systemctl` listings.

## 7. Environments tested

| Environment | Kind of test |
|---|---|
| Ubuntu 26.04.1, kernel 7.0, AMD Ryzen 3 7320U laptop (Dell Inspiron 15 3535) | **Real hardware**: hardware and utility discovery, through the library, the daemon and the CLI |
| AMD laptop, 8 cores/16 threads, integrated GPU, NVMe, USB stick, one battery, Ubuntu os-release | Synthetic fixture |
| Intel hybrid laptop (4 P + 8 E cores), integrated + NVIDIA dedicated GPU, SATA SSD, two charge-reporting batteries, two DDR5 modules, Fedora (`/usr/lib/os-release`) | Synthetic fixture |
| Desktop: 16 cores/32 threads, placeholder DMI fields, 4 of 8 slots populated, dedicated AMD GPU, SATA HDD, no battery, root-only sensor, no ID database, legacy boot, Arch | Synthetic fixture |
| ARM board: no DMI, no PCI, no cpufreq, eMMC | Synthetic fixture |
| Empty root; broken modules; cancelled and timed-out scans | Synthetic fixture |
| Package databases: dpkg, pacman, Snap, Flatpak (user) | Synthetic fixtures (dpkg and Snap also real) |

No other real distribution or machine was available. Support for anything beyond the development
machine is inferred from fixtures, not demonstrated on hardware.

## 8. Test results

Run on the development machine after the final change:

| Suite | Tests | Result |
|---|---|---|
| Rust unit tests (11 packages) | 206 | pass |
| Hardware discovery (fixtures + real machine) | 20 | pass |
| Integration (real binaries) | 27 | pass |
| Safety (real binaries + source scans) | 20 | pass |
| **Rust total** | **273** | **pass** |
| Frontend (Vitest) | 41 | pass |
| `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `tsc --noEmit`, `vite build` | | clean |

The cross-process suites were run three more times in a row without a failure.

Of the hardware tests, 15 use synthetic fixtures and 5 read this machine. The real-machine tests
compare the inventory with independent readings (online CPU list, sibling groups, MemTotal, every
non-empty block device, every PCI device and network interface, battery count) rather than with
hard-coded values, so they hold on other machines too.

Problems found by the tests and fixed:

- Root-hub ports were missing: a root hub's interface is named `1-0:1.0`, not `usb1:1.0`.
- Serde silently accepted extra fields on requests without a payload (e.g. `"path": "/"`); the
  codec now rejects any field the typed request lacks.
- A scan cancelled while still queued did not say that the previous result is kept.
- The frontend's Overview squeezed the component panel when the module list grew; visual check.

Coverage gaps: the Tauri command layer still has no automated test (not compilable here); the
`useInventory` polling hook is not tested in a DOM; the RPM SQLite reader is untested (no RPM
database available; it is opened read-only and immutable); hybrid ARM core typing and multi-socket
topology are fixture-only.

## 9. Safety verification

Unchanged Phase 1 behavioural tests now run a full real-machine scan inside their `exercise` step,
so they also cover discovery: the daemon holds only Unix sockets, runs as the user with no
capabilities, returns to one thread and no children after scanning, writes only its own four files,
never terminates a bystander, never reports monitoring as active, and makes zero wake-ups when idle
after scanning.

New safety tests:

| Test | Verifies |
|---|---|
| `discovery_crates_never_write_or_change_anything` | No write API in the discovery crates; hardware reads only through `SysRoot` |
| `systemd_queries_only_list_and_never_change_anything` | `systemctl` arguments are only `list-units`/`list-unit-files` and flags; no service-changing verb anywhere; never killed; environment cleared |
| `discovery_makes_no_network_connections_while_scanning` | The daemon's sockets sampled throughout a scan: never TCP/UDP/raw |
| `discovery_logs_contain_no_machine_identifiers` | No MAC address, serial, UUID or product name in the log |
| `utility_discovery_never_runs_what_it_finds` | Planted executables named like real tools are found but never executed |
| `scanning_changes_no_system_file` | os-release, the dpkg database, RPM database and ID databases unchanged (size, mtime, ctime, inode); no RPM lock files |
| `real_machine_discovery_reads_only_expected_locations` (hardware suite) | 1,707 paths, all under `/sys`, parts of `/proc`, os-release and the ID databases; no filesystem walk |
| `serial_numbers_and_uuids_are_never_read` (hardware suite) | Planted serials and UUIDs never reach the inventory and their files are never opened |

Deliberate changes to Phase 1 rules, each with its reason in the test source:

- `processes_are_only_launched_by_the_launcher`: one more file may start processes,
  `sentinel-utilities/src/services/query.rs` (the fixed `systemctl` listings). The launcher may now
  also start `gio`/`gtk-launch` for a user's Open click; the names are pinned.
- `no_forbidden_capability_appears_in_the_sources`: the discovery crates may name system paths (they
  read them); the catalogue may name programs (data, no process API, checked); `query.rs` may name
  `systemctl`; the package and desktop adapters may name the Snap and Flatpak directories.
- `ipc_protocol_has_no_open_ended_requests`: nine new requests pinned; the only payloads are the
  configuration patch and a job enum.
- `desktop_shell_grants_the_webview_only_window_controls`: thirteen commands pinned; the only
  free-form argument is the utility ID.

Nothing was installed, no system configuration was changed, `sudo` was not used.

## 10. Resource consumption

Release build, measured from `/proc` with the daemon in a sandboxed home:

| Measure | Value |
|---|---|
| Hardware scan | 0.15 s wall time through the CLI; 40 ms total in the modules (pci.ids lookup and USB are the slowest) |
| Utility scan | 2.4–2.6 s, almost all waiting for `systemctl` |
| Daemon memory before / after both scans | RSS 5.7 MB → 7.2 MB; PSS 3.3 MB → 4.7 MB; private anonymous 0.55 MB → 1.7 MB (holds the inventory and catalogue) |
| After 20 more inventory reads | RSS 7.1 MB (no growth) |
| Threads | 1 when idle; 2 only while a scan runs |
| Idle after scanning, 30 s | 0 context switches, 0 CPU ticks |
| `sentinel hardware --json` | 19 ms average (process start, request, 189 KB answer) |
| Payloads | Inventory 189 KB, utility catalogue 168 KB (JSON) |
| Database with both caches | 372 KiB |
| `sentineld` binary | 3.70 MiB (2.33 MiB in Phase 1) |
| `sentinel` binary | 3.50 MiB (1.09 MiB in Phase 1); it now includes SQLite to read the cache while the daemon is stopped |

The desktop application's resource use is still unmeasured (the shell is not compiled).

## 11. Known limitations

1. **The desktop shell is uncompiled**, so the new Tauri commands and the Open button are untested in
   the real window. The pages were rendered in a browser from the production bundle with the bridge
   answered by real CLI output (screenshots in `target/ui-phase2-*.png`).
2. Installed RAM, memory type, speed and modules are unavailable as an ordinary user on typical
   systems (root-only SMBIOS records).
3. GPU integrated/dedicated classification is a heuristic covering Intel, AMD, NVIDIA and common
   virtual GPUs; others are "unknown".
4. The microarchitecture table covers common client CPUs only.
5. Values that change constantly (battery status and charge, link state, memory usage, frequency
   limits) are snapshots from scan time, labelled as such. There is no live monitoring.
6. Only the RPM database's package names are read, not versions; Berkeley-DB RPM databases are not
   read.
7. Inventories are not refreshed when devices are plugged in or removed; refresh is manual.
8. Utility launching depends on `gio` or `gtk-launch` being installed.
9. User services cannot be listed when the daemon has no access to the user's session bus (e.g. if
   started from a non-graphical login); this is reported as a partial result.
10. The approved reference image was still not available; the new pages follow the written design.

## 12. Outstanding issues

1. Install the Tauri prerequisites, compile the shell, and verify in the window: scan and cancel
   buttons, cached/live labelling after a restart, the Open button, and the desktop's idle resource
   use. Command:

   ```bash
   sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
     libjavascriptcoregtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev
   ```

2. Test on at least one non-Debian distribution and on Intel and NVIDIA hardware.
3. Compare the pages with the approved reference image.

## 13. Commands

```bash
cargo build && npm install && npm run build      # build daemon, CLI, frontend
./target/debug/sentinel start                    # start the daemon
./target/debug/sentinel scan                     # scan hardware and utilities (read-only)
./target/debug/sentinel scan --hardware          # refresh the hardware inventory only
./target/debug/sentinel scan --status            # progress; --cancel to stop a scan
./target/debug/sentinel hardware [--json]        # show the inventory (live or cached)
./target/debug/sentinel hardware capabilities    # which sources are readable
./target/debug/sentinel utilities [--all] [--category system_monitoring] [--search disk]
./target/debug/sentinel utilities services [--all]
./target/debug/sentinel utilities open htop      # open an installed application
./target/debug/sentinel stop
cargo test && npm test                           # all tests
npm run desktop:dev                              # desktop app (needs the WebKitGTK packages)
```

Phase 3 has not been started.
