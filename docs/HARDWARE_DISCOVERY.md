# Hardware discovery

Code: `crates/sentinel-hardware`. Everything here **reads**. The crate has no API that writes,
creates, renames or deletes files, runs no programs, and never uses elevated privileges. The safety
suite enforces this (`discovery_crates_never_write_or_change_anything`).

## How it runs

1. A client asks the daemon to scan (`sentinel scan`, or the Scan button). Nothing is scanned
   automatically, at start-up or on a timer.
2. The daemon's coordinator (`sentinel-daemon/src/discovery.rs`) starts one worker thread. Scans run
   one at a time; a scan already queued or running is never queued twice. The thread exits when
   there is no more work.
3. `sentinel_hardware::discover` runs the modules in a fixed order, timing each one and reporting
   progress. System identity runs first.
4. A module that fails or finds nothing records why and the next module runs anyway. One broken
   source never stops the others.
5. Cancellation (or the 60 second limit) is checked between modules and inside loops. A stopped
   scan keeps the previous inventory and is not cached.
6. A complete scan replaces the inventory and is written to the cache (see
   [SYSTEM_INVENTORY.md](SYSTEM_INVENTORY.md)).

All reads go through `SysRoot` (`src/source.rs`), relative to a root directory: `/` in production,
a synthetic tree in tests. `SysRoot` refuses `..` components, anything under `/dev`, and every
attribute that identifies one machine or device (see "Identifiers never read" below).

## Linux interfaces used

| Module | Sources | Notes |
|---|---|---|
| System identity | `/sys/class/dmi/id/{sys_vendor,product_name,product_version,product_family,product_sku,chassis_type}`, `/proc/sys/kernel/{osrelease,version,arch}`, `/etc/os-release` (falls back to `/usr/lib/os-release`) | Firmware placeholders such as "To Be Filled By O.E.M." are rejected as *invalid*, not shown |
| Firmware | `/sys/class/dmi/id/bios_*`, `ec_firmware_release`; `/sys/firmware/efi` for boot mode | Legacy boot is inferred from the absence of `/sys/firmware/efi` |
| Motherboard | `/sys/class/dmi/id/board_{vendor,name,version}` plus the system fields | Board identity is never derived from the laptop model |
| CPU | `/sys/devices/system/cpu/{online,present}`, `cpuN/topology/*`, `cpuN/cache/indexN/*`, `cpuN/cpufreq/cpuinfo_{min,max}_freq`, `cpuN/cpu_capacity`, `/sys/devices/cpu_core/cpus` and `cpu_atom/cpus`, `/proc/cpuinfo` | Topology comes from sysfs only; `/proc/cpuinfo` gives identity and flags |
| Memory | `/proc/meminfo`, `/sys/devices/system/memory/`, `/sys/firmware/dmi/entries/17-*/raw` (SMBIOS type 17), `/sys/devices/system/node/` | SMBIOS records are usually root-only (see below) |
| Graphics | PCI display-class functions in `/sys/bus/pci/devices`, their `drm/`, `boot_vga`, `mem_info_vram_total` | |
| Storage | `/sys/block/*` (`size`, `queue/*`, `removable`, `ro`, `device/model`, `device/vendor`, `device/firmware_rev` or `device/rev`), partitions as children | `/dev` nodes are never opened |
| Battery and power | `/sys/class/power_supply/*` | Battery temperature, serial numbers and Dell PPIDs are not read |
| Network | `/sys/class/net/*` (`type`, `uevent`, `address`, `addr_assign_type`, `operstate`, `carrier`, `speed`, `mtu`, `device/driver`, `phy80211`) | No traffic is read or captured |
| USB | `/sys/bus/usb/devices/*` (descriptors, `busnum`, `devnum`, `speed`, `version`, interface classes, hub `*-portN/connect_type`) | |
| PCI | `/sys/bus/pci/devices/*` (`vendor`, `device`, `subsystem_*`, `class`, `revision`, `driver`), parent bridge from the device path | |
| Sensors | `/sys/class/hwmon/hwmonN/{name,*_input,*_label,device}`, `/sys/class/thermal/thermal_zoneN/{type,temp,trip_point_*}` | Readability is checked by opening the file; **no value is read** |
| Names for IDs | `/usr/share/hwdata/{pci,usb}.ids` (or `/usr/share/misc/`) | Streamed once per scan; only IDs present are kept; confidence `database` |
| Capabilities | Existence and readability of the directories above | `sentinel hardware capabilities` |

## Supported fields

See `crates/sentinel-inventory/src/hardware.rs`; every field is listed there with its unit. In
summary: manufacturer, product, family, SKU, chassis, architecture, kernel, distribution; BIOS vendor,
version, date, revision, EC revision, boot mode; board manufacturer, name, revision; per CPU package
the vendor, model, family/model/stepping, microarchitecture (inferred), physical cores, logical
processors, every core with its logical CPUs, die, cluster and core type, caches, flags and frequency
limits; installed, kernel-managed and usable memory, a usage snapshot, swap, memory type, speed,
slots and modules, NUMA nodes; GPU identity, kind, driver, DRM nodes and memory; storage model,
interface, kind, capacity, sector sizes, rotational, removable, read-only and partitions; battery
capacity (energy or charge), health, status, cycles, voltage; network kind, MAC, link state, speed,
driver and bus; USB IDs, names, speed, classes, hub ports and parents; PCI IDs, class, role, driver,
parent; sensor chip, kind, label, path, access and providing device; thermal zones.

## Inferences, and how they are labelled

These values are concluded rather than read, carry confidence `inferred`, and say so in the UI. (Values that are
only *calculated* from detected values, such as battery health and memory in use, carry `derived`
since Phase 5.)

- **Microarchitecture** from CPUID family/model, using a table of common client parts. Unknown
  combinations return "unknown" rather than a guess.
- **Integrated vs dedicated GPU.** Intel graphics on PCI bus 0 is integrated. An AMD GPU sharing its
  PCI slot with AMD platform functions (crypto, USB, audio co-processor) is an APU's integrated GPU;
  otherwise it is dedicated. NVIDIA is dedicated. Virtual-machine vendors are virtual. Anything else
  is "unknown". Integrated-GPU "VRAM" is reported as *system memory reserved for the GPU*, never as
  dedicated memory.
- **Core types** from `cpu_core`/`cpu_atom` (kernel-reported) or, failing that, differing
  `cpu_capacity` values.
- **Storage interface** from the device's position in the kernel device tree.
- **Battery health** = full capacity / design capacity, only from a matching pair (energy with
  energy, charge with charge).
- **Used memory** = MemTotal − MemAvailable, labelled as a snapshot.

## Permission limitations

On a standard Linux system an ordinary user cannot read:

- `/sys/firmware/dmi/tables/DMI` and `/sys/firmware/dmi/entries/*/raw`. As a result **installed
  RAM, memory type, speed and module layout are reported as "administrator only"** on most
  machines. Sentinel shows the kernel-managed and usable memory instead and never estimates the
  installed size.
- Serial numbers and the system UUID (also not wanted; see below).
- Some hwmon inputs on some drivers. These are listed with access "administrator only".

Sentinel never asks for elevated rights to get around this, and has no privileged helper.

## Identifiers never read

`SysRoot` refuses to open any file named `product_serial`, `board_serial`, `chassis_serial`,
`product_uuid`, `serial`, `serial_number`, `eppid`, `wwid`, `wwn`, `uuid`, `nguid`, `eui`,
`subsysnqn`, `machine-id`, `boot_id`, `board_asset_tag` or `chassis_asset_tag`. Fixture tests plant
such files and verify their contents never reach the inventory. MAC addresses *are* collected,
because the plan requires them; they stay in the local inventory, are masked in the UI until
revealed, and are never logged (verified by `discovery_logs_contain_no_machine_identifiers`).

## Distribution and hardware differences

- os-release: `/etc/os-release` on most distributions; Fedora-style systems may have only
  `/usr/lib/os-release`. Both are read; rolling releases without `VERSION_ID` show it as unavailable.
- ID databases live in `/usr/share/hwdata` (Fedora, Arch, recent Debian/Ubuntu) or
  `/usr/share/misc`. Without them devices are shown by numeric ID.
- ARM systems usually have no DMI and no PCI bus; identity then comes from the kernel and cpuinfo
  (implementer codes), and the board is "unidentified".
- `/proc/sys/kernel/arch` is missing on older kernels; the build target is used and marked inferred.

## Known unsupported or unverified hardware

- Multi-socket servers: the model supports several packages, tested only with synthetic fixtures.
- Hybrid ARM (big.LITTLE) core typing relies on `cpu_capacity`; not tested on real hardware.
- GPU classification for vendors other than Intel, AMD, NVIDIA and common virtual devices is
  "unknown" (e.g. ARM Mali, Apple, Moore Threads).
- Intel Arc discrete GPUs on bus 0 would be misclassified as integrated; none has been tested.
- SMART/NVMe health is not read (later phase).
- Thunderbolt, SD readers behind PCI, and NVMe-oF have not been tested.
- USB port numbers are Linux's; mapping them to physical sockets needs board documentation.

## Adding a discovery module

1. Add a `ModuleId` variant (in `sentinel-inventory/src/discovery.rs`, keep `HARDWARE` ordered),
   its label, and the record types in `hardware.rs` using `Field<T>` for every discovered value.
2. Write `crates/sentinel-hardware/src/modules/<name>.rs` with
   `pub fn discover(ctx: &DiscoveryContext) -> Result<ModuleRun<T>, Stopped>`. Read only through
   `ctx.root`; call `ctx.check()?` inside loops; return `Unavailable` with a note when the source
   does not exist, never an error.
3. Wire it into `manager.rs` and the `SystemInventory`.
4. Put pure parsing in `parsers/` with unit tests on literal input; add a fixture builder method in
   `tests/support/hwfixture.rs` and tests in `tests/hardware/main.rs`.
5. If a new top-level location is read, add it to `ALLOWED_PREFIXES` in
   `tests/hardware/main.rs`, with a reason in review.
6. Bump `INVENTORY_SCHEMA_VERSION` if the change is not backward compatible; old caches are then
   ignored, not misread.

## Block device use (Phase 5)

The storage module also records, at scan time, how each device and partition was in use:
mounts from `/proc/mounts` (every mount of a device node, so a btrfs volume with several
subvolume mounts lists them all), active swap partitions from `/proc/swaps` (swap files are not
partitions and are skipped), and device-mapper layering from sysfs `holders/` and `slaves/`, with
the mapper name from `dm/name`. These are the same tables the telemetry engine reads, so a
filesystem's live usage maps onto the right partition. The partition table and filesystem
superblocks are never read: the filesystem of a partition that is not mounted is not known, and
the explorer says so. `proc/mounts` and `proc/swaps` were added to the allow-list in
`tests/hardware` (`real_machine_discovery_reads_only_expected_locations`).
