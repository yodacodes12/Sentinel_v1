# Metric reference

Every metric Sentinel reports: where it comes from, its unit, and how it is calculated. Paths are
relative to `/`. "Series" is the key used for charts and history (`proc.*` and `*.freq` are kept
in memory only).

## Reading states and quality

Every value is a `Reading` with a **state**:

| State | Meaning |
|---|---|
| `ok` | Measured or calculated |
| `pending` | A rate or percentage needs two samples; this was the first |
| `unsupported` | This machine, kernel or driver does not provide it |
| `unavailable` | The source exists but gave nothing (device absent, firmware "not counted") |
| `permission_denied` | Readable only by administrators; never bypassed |
| `error` | The source failed, returned malformed data, or a counter went backwards |
| `unknown` | No source was found and the reason is not known |

and a **quality**: `direct` (read as is), `calculated` (exact arithmetic on direct values, such
as a counter delta) or `estimated` (a model: battery health, remaining runtime). The UI and CLI
show the state instead of a number whenever the state is not `ok`.

## CPU

| Metric | Source | Unit | Calculation | Series |
|---|---|---|---|---|
| Total utilisation | `proc/stat` `cpu` line | % | `100 × (Δtotal − Δidle − Δiowait) / Δtotal` over the last interval; `Δtotal` sums user, nice, system, idle, iowait, irq, softirq, steal | `cpu.total` |
| Per-CPU utilisation | `proc/stat` `cpuN` lines | % | Same, per logical CPU | `cpu.core.N` |
| User, nice, system, idle, iowait, irq, softirq, steal | `proc/stat` | % | Each field's delta / `Δtotal` | `cpu.iowait` |
| Frequency per CPU | `sys/devices/system/cpu/cpuN/cpufreq/scaling_cur_freq` | kHz | Direct; `unsupported` without cpufreq | `cpu.core.N.freq` |
| Average frequency | as above | kHz | Mean of the CPUs that report one | `cpu.freq` |
| Logical CPUs | `proc/stat` | count | Number of `cpuN` lines (online CPUs) | |

Guest time is already included in user time by the kernel and is not added again. If any counter
decreased (hot-plug, wrap) or no time passed, the interval yields no value.

## System load

| Metric | Source | Unit | Calculation | Series |
|---|---|---|---|---|
| Load average 1, 5, 15 min | `proc/loadavg` | runnable + uninterruptible tasks | Direct | `load.1` |
| Running / total tasks | `proc/loadavg` | count | Direct | |
| Uptime | `proc/uptime` | s | Direct | |
| CPU and I/O pressure | `proc/pressure/cpu`, `proc/pressure/io` | % of time | PSI `some` and `full`, `avg10` and `avg60`, direct | `cpu.pressure` |

## Memory

| Metric | Source | Unit | Calculation | Series |
|---|---|---|---|---|
| Total, free, available, buffers, active, inactive | `proc/meminfo` | bytes | Direct (kB × 1024) | `mem.available` |
| Used | `proc/meminfo` | bytes | `MemTotal − MemAvailable`. "Total − free" would count reclaimable cache as used | `mem.used` |
| Used % | | % | `used / MemTotal` | `mem.used_percent` |
| Cached | `proc/meminfo` | bytes | `Cached + SReclaimable` | `mem.cached` |
| Swap total, free, used | `proc/meminfo` | bytes | used = `SwapTotal − SwapFree` | `swap.used` |
| Swap in / out | `proc/vmstat` `pswpin`, `pswpout` | pages/s | Counter delta / elapsed | `swap.in`, `swap.out` |
| Memory pressure | `proc/pressure/memory` | % of time | PSI `some` and `full`, `avg10` and `avg60`, direct | `mem.pressure` |

Compressed swap (zram) counts as swap, as the kernel reports it.

## Processes

| Metric | Source | Unit | Calculation |
|---|---|---|---|
| CPU % | `proc/PID/stat` utime + stime | % of one CPU | `Δticks / CLK_TCK / Δt × 100`; 100 % = one fully busy CPU, so a process can exceed 100 % |
| Resident memory | `proc/PID/stat` rss | bytes | `rss pages × page size` |
| Memory % | | % | `RSS / MemTotal` |
| Virtual size | `proc/PID/stat` vsize | bytes | Direct |
| Threads, state, start time | `proc/PID/stat` | | Direct; start = boot time + start ticks / `CLK_TCK` |
| Name | `proc/PID/stat` comm | | Direct (the kernel's 15-character name) |
| Program | `proc/PID/cmdline` | | First 256 bytes, cut at the first argument, base name only |
| User | `proc/PID/status` Uid, `etc/passwd` | | Real uid, mapped to a name |

A process is identified by pid **and** start time, so a reused pid never borrows the previous
process's CPU time. Processes that exit between listing and reading are skipped. Processes whose
files are not readable are counted as "not readable by this user". Series `proc.PID.cpu` and
`proc.PID.rss` exist for the 10 busiest processes, in memory only.

## Battery and power

| Metric | Source (`sys/class/power_supply/BAT*`) | Unit | Calculation | Series |
|---|---|---|---|---|
| Charge level | `capacity` | % | Direct | `battery.NAME.percent` |
| State | `status` | | Charging, discharging, full, not charging, unknown | |
| Energy now / full / design | `energy_now`, `energy_full`, `energy_full_design` | Wh | µWh / 10⁶ | `battery.NAME.energy` |
| Charge now / full / design | `charge_*` (batteries that report charge) | Ah | µAh / 10⁶ | |
| Voltage | `voltage_now` | V | µV / 10⁶ | |
| Current | `current_now` | A | |µA| / 10⁶ | |
| Power | `power_now`, else `current_now × voltage_now` | W | Direct (µW / 10⁶), else calculated | `battery.NAME.power` |
| Health | `energy_full / energy_full_design` (or charge) | % | Estimated; firmware capacity figures are themselves estimates | |
| Cycle count | `cycle_count` | count | Direct; `0` is treated as "not counted" (`unavailable`) | |
| Temperature | `temp` of the battery itself | °C | Decidegrees / 10; shown only when the battery reports it | `battery.NAME.temp` |
| On mains power | `sys/class/power_supply/*` of type Mains or USB, `online` | | Direct; without an adapter entry, inferred from the battery state (calculated) | |
| Remaining runtime | | s | See below | |

**Remaining runtime** = remaining energy / average discharge power (or charge / current) over the
last 5 minutes. It is shown only while discharging, after at least 4 valid samples spanning at
least 60 seconds, and only if the power readings vary by less than 80 % (coefficient of
variation). Confidence is high below 15 % variation with 8 or more samples, medium below 35 %,
low otherwise. It is rounded to whole minutes and always labelled as an estimate. Plugging in
clears it at once. Power readings below 0.05 W are ignored (some firmware reports 0 right after
unplugging). The average draw shown with the estimate (`average_power_w`) is always in watts: for
a battery that reports charge, it is the average current times the present voltage, and it is
left out (`null`) when the battery reports no voltage. (Before Phase 4 this field carried amperes
for such batteries.)

## Temperatures and fans

| Metric | Source | Unit | Calculation | Series |
|---|---|---|---|---|
| hwmon temperature | `sys/class/hwmon/hwmonN/tempM_input` | °C | Millidegrees / 1000; values outside −60…250 °C are `error` | `temp.hwmonN/tempM` |
| Its limits | `tempM_max`, `tempM_crit` | °C | Driver-reported; never invented | |
| Thermal zone | `sys/class/thermal/thermal_zoneN/temp` | °C | Millidegrees / 1000 | `temp.thermal_zoneN` |
| Zone critical limit | `trip_point_K_temp` where `trip_point_K_type` is `critical` | °C | Firmware-defined | |
| Fan speed | `sys/class/hwmon/hwmonN/fanM_input` | RPM | Direct; `fanM_min`, `fanM_max` when present | `fan.hwmonN/fanM` |

The kernel's hwmon mirror of a thermal zone (under `/sys/devices/virtual/thermal/`) is skipped so
the zone is not counted twice. `pwm*` (fan control) files are never opened.

**What a sensor measures** is stated only on evidence: a driver that measures one kind of part
is *high* confidence (`k10temp`, `coretemp`, `zenpower` → CPU; `nvme`, `drivetemp` → storage;
`amdgpu`, `radeon`, `nouveau`, `i915`, `xe` → GPU; `jc42`, `spd5118` → memory; battery hwmon →
battery); a wireless driver's sensor (`iwlwifi`, `ath*`, `mt7*`, `rtw*`) is *medium*; a firmware
label such as "CPU", "Tctl", "SODIMM" or "Ambient" (e.g. from `dell_smm`) is *low*; anything else
is "unknown". ACPI thermal zones are "unknown": the firmware does not say where they are.

## GPU

Read from `sys/bus/pci/devices/ADDRESS` for every display-class device (PCI class `0x03xxxx`).

| Metric | Source | Unit | Drivers | Series |
|---|---|---|---|---|
| Busy | `gpu_busy_percent` | % | amdgpu | `gpu.ADDRESS.busy` |
| VRAM / GTT used and total | `mem_info_vram_*`, `mem_info_gtt_*` | bytes | amdgpu | `gpu.ADDRESS.vram` |
| Temperature | `hwmon/*/temp1_input` | °C | amdgpu, nouveau, others with hwmon | `gpu.ADDRESS.temp` |
| Power | `hwmon/*/power1_average` or `power1_input` | W | amdgpu | `gpu.ADDRESS.power` |
| Core clock | `hwmon/*/freq1_input` (Hz), else `drm/cardN/gt_cur_freq_mhz` | MHz | amdgpu; i915/xe | |

Memory model: *shared* for integrated GPUs (an AMD GPU sharing its PCI slot with AMD platform
functions, or Intel graphics on bus 0), *dedicated* for NVIDIA or a lone AMD card. On an
integrated GPU, "VRAM" is system memory reserved for graphics. NVIDIA's proprietary driver and
Intel's i915/xe expose no utilisation in sysfs; Sentinel says so rather than running
`nvidia-smi` or using perf counters.

## Storage

| Metric | Source | Unit | Calculation | Series |
|---|---|---|---|---|
| Read / write throughput | `proc/diskstats` sectors read / written | B/s | `Δsectors × 512 / Δt` (the kernel always counts 512-byte sectors here) | `disk.NAME.read`, `disk.NAME.write` |
| Read / write operations | `proc/diskstats` reads / writes completed | ops/s | Delta / Δt | |
| Busy | `proc/diskstats` time spent doing I/O | % | `Δio_ms / Δt / 10`, capped at 100 | `disk.NAME.busy` |
| Filesystem size, used, available | `statvfs` on each mount point from `proc/self/mounts` | bytes | used = total − free; used % = used / (used + available), like `df` | `fs.MOUNT.used_percent` |

Only physical disks (entries in `sys/block` that are not under `/virtual/`, so no loop, zram or
device-mapper devices) and local filesystems on `/dev/*` (ext2/3/4, xfs, btrfs, f2fs, vfat, exfat,
ntfs and similar) are included. Network and pseudo filesystems are never `statvfs`ed, so a hung
network mount cannot stall the collector. "Busy" is activity, not a health measure; disk health
(SMART) is not read.

## Network

| Metric | Source (`sys/class/net/IFACE/statistics`) | Unit | Calculation | Series |
|---|---|---|---|---|
| Receive / transmit | `rx_bytes`, `tx_bytes` | B/s | Delta / Δt | `net.IFACE.rx`, `net.IFACE.tx` |
| Packets | `rx_packets`, `tx_packets` | packets/s | Delta / Δt | |
| Totals, errors, dropped | `rx_*`, `tx_*` | count | Direct (since the interface came up) | |
| State | `operstate` | | Direct | |
| Loopback | `type` = 772 | | Direct | |

Counters only: no packet is captured or inspected. A counter that goes backwards (interface
re-created) is `error` for one interval.

## Health indicators

One status per area: CPU pressure, memory pressure, swap pressure, thermal, battery, storage
capacity, storage activity, network activity and sensor availability. Each carries the evidence
behind it in words. There is no combined score. Sustained conditions come only from active
alerts (which already require a duration); a current reading on its own can raise an area to
"elevated" at most, so a brief spike never shows as "high" or "critical".
