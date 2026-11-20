# Hardware and environment compatibility (Phases 4 and 5)

What has been checked, and how. Statuses:

- **Physically validated**: run on real hardware or a real desktop, and the result checked
  against an independent source.
- **Partially validated**: run on real hardware, but part of the behaviour could not be
  observed.
- **Mock tested**: synthetic `/proc` and `/sys` fixtures (or a fake service) only.
- **Not tested**.

The only physical machine available was the development laptop: Dell Inspiron 15 3535, AMD Ryzen 3
7320U, 6.5 GiB RAM, Radeon 610M (integrated, amdgpu), NVMe SSD, Wi-Fi, a charge-reporting Li-ion
battery (BYD, DELL PG8YJ47J), one firmware-controlled fan (`dell_smm`); Ubuntu 26.04, kernel 7.0,
GNOME 50.1 on Wayland.

## The plan's matrix

| Environment | Validation | Status | Evidence and gaps |
|---|---|---|---|
| Ubuntu/Debian | Primary Linux compatibility | **Ubuntu: physically validated.** Debian: mock tested | Ubuntu 26.04: every Phase 4 test. Debian: an `os-release` fixture only |
| GNOME | Navigation and notifications | **Physically validated** | GNOME 50.1, Wayland: 12 real-window scenarios; a real notification delivered once ([NOTIFICATION_VALIDATION.md](NOTIFICATION_VALIDATION.md)). X11 sessions not tested |
| KDE Plasma | Notifications and UI | **Not tested** | No KDE system was available. Same freedesktop notification interface; WebKitGTK renders the window the same way |
| AMD GPU | Discovery and telemetry | **Integrated: physically validated.** Discrete: mock tested | Radeon 610M: identification, driver, busy %, VRAM (shared, labelled so), GTT, temperature, power, clock; VRAM, GTT and temperature equal the driver's files. Discrete AMD: desktop fixture |
| Intel GPU | Discovery and telemetry | **Mock tested** | Fixture (i915): listed, GT frequency only; utilisation honestly unsupported (needs perf counters) |
| NVIDIA GPU | Discovery and telemetry | **Mock tested** | Fixture: listed with driver; utilisation, memory and power unsupported with the reason (the proprietary driver exposes none in sysfs; `nvidia-smi` is never run) |
| CPU-only systems | Graceful GPU fallback | **Mock tested** | ARM board fixture without PCI: monitoring runs, GPU collector `unsupported`, no GPU invented (`machines_without_battery_or_gpu_are_monitored_honestly`) |
| 8 GB RAM systems | Resource consumption | **Partially validated** | The laptop has 6.5 GiB usable, so the numbers in [POWER_EFFICIENCY.md](POWER_EFFICIENCY.md) are from a smaller machine than 8 GB. Sentinel's memory does not grow with installed RAM |
| 16 GB RAM systems | Normal operation | **Not tested** | No such machine. Nothing in Sentinel depends on the amount of RAM; the process collector's cost grows with the number of processes |
| Laptop with battery | Battery telemetry and power impact | **Partially validated** | Presence, charge level, charge (Ah), voltage, current, calculated power, health (82 %), charging and discharging states: equal to the battery's files. A real `battery_low` alert at 15 % while discharging. Not observed: the full-charge state, an energy-reporting (Wh) battery (mock tested), runtime accuracy over a full discharge, and Sentinel's effect on battery life |
| Desktop without battery | Graceful battery fallback | **Mock tested** | Desktop fixture with an empty `power_supply`: no battery invented, no battery alert, power-aware sampling never switches to Eco |

## Other components

| Component | Status | Notes |
|---|---|---|
| CPU utilisation, total and per core (AMD, 8 logical CPUs) | Physically validated | Within 0.07 points of `/proc/stat` over 240 s |
| CPU frequency (amd-pstate) | Partially validated | Read from `scaling_cur_freq`; not compared with an independent source |
| Intel hybrid CPUs (P/E cores) | Mock tested | Fixture |
| Memory and swap (swap file) | Physically validated | Equal to `/proc/meminfo` |
| zram swap | Mock tested | |
| Processes | Physically validated | Threads exact, RSS within the movement between two reads; Phase 3 compared with `ps` |
| NVMe SSD (I/O, capacity, temperature) | Physically validated | Within 2.5 % of `/proc/diskstats` |
| SATA SSD, HDD, USB disks | Mock tested | |
| Wi-Fi throughput | Physically validated | Within 0.4 % of `/proc/net/dev` |
| Ethernet | Not tested physically | Same counters as Wi-Fi |
| Temperatures: k10temp, nvme, amdgpu, dell_smm, ACPI zone | Physically validated | Within 0.75 °C of each sensor |
| Fans (dell_smm) | Physically validated in Phase 3 | One fan, firmware-controlled |
| Sensors without limits | Physically validated | No alert without a driver or user threshold |
| Battery temperature | Physically validated (absent) | The `temp` attribute returns ENODEV here; correctly shown as not provided |
| Desktop notifications | Physically validated on GNOME | |
| Suspend and resume | **Not tested** | Detection logic unit tested; the laptop did not sleep during testing |

## Remaining physical validation

To call these validated, someone needs to run the existing checks (`tests/desktop-e2e/`) on:

1. **A real suspend and resume** with monitoring on and the window open: no stale values, no
   burst of notifications, no CPU spike, history with a gap (one `systemctl suspend` by the user).
2. **A full battery discharge** with the same workload, monitoring on and off, to compare the
   runtime estimate with the real time and to measure Sentinel's effect on battery life. Several
   hours; needs the laptop off its charger.
3. **The full-charge state** (leave the laptop charging until it reports `Full`).
4. **Intel and NVIDIA GPUs**, and a discrete AMD GPU.
5. **KDE Plasma** (notifications and the window), and an **X11** session.
6. **A desktop PC without a battery**, and a **16 GB** machine.
7. **A SATA or USB disk** and **Ethernet**.

## Phase 5: hardware explorer

Same physical machine. "Physically validated" means the real window, driven through AT-SPI
(`tests/desktop-e2e/explorer_scenarios.py`, 12/12), showed what an independent read of `/sys` or
`/proc` by the test script says, or a Rust test compared the index with the live telemetry engine
on this machine.

| Explorer area | Test laptop (AMD Ryzen 3 7320U, Radeon 610M) | Fixture machines |
|---|---|---|
| CPU package, cores, threads | **Physically validated**: 1 package, 4 cores, 8 logical CPUs; core 0 = CPU 0, CPU 1 as in `core_cpus_list` | AMD 8C/16T, Intel hybrid 4P+8E, ARM without cpufreq: mock tested |
| Cache hierarchy | **Physically validated**: 13 instances (4 × L1d, 4 × L1i, 4 × L2, 1 L3 shared by all 4 cores, as in `shared_cpu_list`) | L3 listing non-existent CPUs (0-999): clipped, mock tested |
| Per-core live utilisation | **Physically validated**: live in the inspector, state Live; every live CPU sample maps to a component | Mean of a core's threads (derived): mock tested |
| Memory capacities, live use | **Physically validated** (installed RAM is administrator-only here and shown as such) | — |
| Firmware memory devices | **Not testable here**: records are root-only (`/sys/firmware/dmi/entries/17-*/raw` mode 0400) | Two SODIMM records: mock tested |
| NUMA | Not present on this machine | Not tested |
| GPU (integrated, amdgpu) | **Physically validated**: Radeon 610M, amdgpu, reserved system RAM + GTT, busy/temperature/clock/power live | Intel iGPU and NVIDIA dGPU with unknown memory: mock tested. Real Intel/NVIDIA: **not tested** |
| Storage hierarchy | **Physically validated**: NVMe with 7 partitions; root `nvme0n1p7` ext4 and `/boot/efi` match `/proc/mounts`; NTFS partitions shown as not mounted | LUKS volume on a partition, USB stick mount with a space, swap file vs partition, unreadable mount table: mock tested. Real LVM/LUKS/RAID: **not tested** |
| Battery capacity view | **Physically validated**: BAT1 reports charge; design 3.65 Ah and full-charge 2.99 Ah shown as in sysfs, in Ah (no conversion to Wh) | Energy-reporting battery: mock tested |
| Network adapters, MAC masking | **Physically validated**: interfaces listed, 2 MAC addresses never shown unmasked | — |
| USB tree | **Physically validated**: 3 devices as in sysfs | Orphan device (parent not discovered): mock tested |
| PCI tree | **Physically validated**: 30 functions as in sysfs, every one under a discovered bridge; GPU→PCI link | Missing bridges (`parent not discovered`): mock tested |
| Motherboard identity | Physically validated (Phase 2) | — |
| Phase 4 desktop scenarios after the change | **Physically validated**: `window_scenarios.py` 12/12, `browser_settings.mjs` 7/7 | — |
