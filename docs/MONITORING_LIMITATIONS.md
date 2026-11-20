# Monitoring limitations

What Sentinel cannot measure, or can measure only approximately, and why. Everything here was
written against one development laptop (AMD Ryzen 3 7320U, Radeon 610M integrated GPU, NVMe SSD,
Dell firmware, Ubuntu 26.04, kernel 7.0) plus synthetic fixtures. Other machines expose different
interfaces; Sentinel reports what it finds rather than assuming.

## By design

- **No privileged access.** Sentinel runs as the user. Anything the kernel makes root-only (some
  hwmon drivers, other users' `/proc/PID/status` in hardened setups, `/dev/*`, MSRs, SMART data)
  is shown as `permission_denied` or not offered. There is no helper that could read it.
- **No external programs.** `nvidia-smi`, `sensors`, `smartctl`, `intel_gpu_top` and similar are
  never run, so anything only they can provide is missing.
- **No perf counters, no eBPF, no packet capture.** No per-process network or disk traffic, no
  Intel GPU utilisation, no instruction-level CPU data.
- **Short-lived processes** that start and exit between two process samples are not seen. With
  nobody looking at the process list, it is sampled once a minute.

## CPU

- Utilisation is the kernel's accounting of time (`/proc/stat`), averaged over the sampling
  interval. A burst shorter than the interval is averaged in, not shown on its own.
- Frequency comes from `scaling_cur_freq`. With `amd-pstate` (this laptop) and `intel_pstate` it
  is the kernel's last estimate, not a reading at the instant of sampling, and may lag. Without
  cpufreq (some VMs, some ARM boards) frequency is `unsupported`.
- In a virtual machine, `steal` is reported but the host's load is not visible.
- Process CPU % is per CPU (100 % = one busy CPU). The kernel accounts in clock ticks
  (100 per second here), so a process using less than one tick per interval shows 0.

## Memory

- "Used" is `MemTotal − MemAvailable`. `MemAvailable` is the kernel's estimate of memory
  available without swapping; it is the right figure for users, but it is an estimate.
- Process memory is resident set size, which counts shared libraries in full for every process
  that maps them, so the per-process figures add up to more than the memory in use. Proportional
  set size (PSS) would need `smaps`, which Sentinel does not read.
- zram counts as swap, as the kernel reports it; its compression ratio is not shown.

## Battery

- **Charge level, energy and health are the firmware's numbers.** `energy_full` (and so health)
  is itself an estimate the battery's gauge maintains; it moves after calibration cycles.
- **Power draw** is the gauge's reading, usually averaged by the firmware over seconds to a minute.
  Batteries that report charge rather than energy (this laptop: `charge_now`, `current_now`) give
  power as current × voltage, a calculated value.
- **Remaining runtime** is an estimate from the last five minutes of discharge. It assumes the
  recent average continues: opening a game or a video call changes the real answer immediately.
  It is withheld until there are enough steady samples, and labelled with a confidence. It has
  not been validated against a full discharge on this machine (that would need a controlled
  run of several hours).
- **Battery temperature** is shown only when the battery reports its own. On this laptop the
  `temp` attribute exists but returns "No such device", so it is shown as not provided. A
  motherboard or CPU sensor is never presented as the battery temperature.
- **Cycle count** `0` is treated as "not counted", because many gauges never count.
- Charge thresholds and charging behaviour are not read or changed.

## Temperatures and fans

- Which part a sensor measures is known reliably only for drivers that measure one kind of part
  (`k10temp`, `nvme`, `amdgpu`...). Firmware labels (`dell_smm`: "CPU", "SODIMM", "Ambient") are
  shown with low confidence. ACPI thermal zones (`acpitz`) do not say where they are.
- Warning and critical limits come from the driver or firmware. Most sensors have none (on this
  laptop, 2 of 9), and Sentinel invents none: such sensors never alert unless the user sets a
  temperature threshold.
- **Fans** are visible only through an hwmon driver. Many laptops expose none; this one exposes one
  fan through `dell_smm` (500 RPM was read at idle). Firmware-controlled fans often report coarse
  steps rather than a continuous speed. Fan control files are never touched.
- **Reading some sensors is not free.** On this laptop a `dell_smm` read takes about 1.2 ms
  (it is a firmware SMM call) and an NVMe temperature read about 3 ms (a query to the drive).
  These dominate the thermal collector's time; see [POWER_EFFICIENCY.md](POWER_EFFICIENCY.md).

## GPU

- **AMD (amdgpu):** utilisation, memory, temperature, power and clock, where the card exposes them.
  On this laptop's integrated Radeon 610M, all of these are available; "VRAM" is system memory
  reserved for graphics and is labelled shared.
- **Intel (i915/xe):** current GT frequency only. Utilisation needs perf counters.
- **NVIDIA proprietary driver:** listed, with no utilisation, memory or power (none in sysfs).
  **nouveau:** temperature where hwmon provides it.
- A machine without a GPU, or with an unbound one, works normally; the GPU section says what it
  found.
- Not tested on real Intel or NVIDIA hardware in Phase 3; that support is verified with fixtures
  only.

## Storage

- Throughput and operations come from `/proc/diskstats`, so they include all I/O, from every
  process, including Sentinel's own history writes.
- **Busy %** is the share of time with at least one request in flight. NVMe drives serve many
  requests in parallel, so 100 % busy does not mean saturated. It is labelled as activity.
- Only physical disks and local filesystems are shown; network filesystems are never queried
  (a hung server would block the collector). Disk health (SMART) is not read.

## Network

- Per-interface kernel counters only. Virtual interfaces (Docker bridges, VPN tunnels) are listed
  like physical ones; traffic through a tunnel also appears on the underlying interface.
- No per-process or per-connection data, and no Wi-Fi signal quality (that needs nl80211).

## Time, sleep and accuracy

- Sampling intervals are targets: under heavy load the monitor thread may run late, and the next
  sample measures the real elapsed time, so rates stay correct.
- Rates use the monotonic clock, which does not count suspended time; history uses the wall
  clock. If the system clock jumps, history points can appear out of order or with a gap.
- After resume, the monitor notices that the wall clock moved further than the monotonic clock
  and restarts sampling as after a pause: fresh counters (no rate spans the sleep), a gap in the
  charts, and pending alert conditions forgotten, so the sleep never counts towards a sustained
  duration. The same happens when the clock is set by more than 5 s. The detection is unit
  tested; **no real suspend cycle has been tested** (the development laptop did not sleep during
  Phase 4 testing, and Sentinel's tests do not suspend the machine).
- The battery runtime estimator's 5-minute window is measured on the monotonic clock, so
  readings from just before a suspend still count just after it; the estimate is withheld again
  only if the readings disagree.

## Notifications

- Need a freedesktop notification service on the session bus (GNOME, KDE, most desktops). Without
  one, delivery fails, is recorded, and the alert is still in the history.
- A daemon started outside the desktop session (e.g. from a plain SSH session) may not find the
  session bus. It falls back to `$XDG_RUNTIME_DIR/bus`.
- Tested end-to-end against a fake session bus in the test suite, and in Phase 4 against the real
  GNOME Shell 50.1 notification service: one real notification was shown and not repeated (see
  [NOTIFICATION_VALIDATION.md](NOTIFICATION_VALIDATION.md)). KDE Plasma and other notification
  services have not been tested.
- One notification per raised condition and filesystem: an alert on several filesystems at once
  (e.g. `/` and `/boot/efi` both nearly full) notifies once for each.

## Platform

- Linux only. Tested on one laptop; behaviour on other kernels, firmware and drivers is predicted
  from their documented sysfs interfaces and fixtures, not observed.
