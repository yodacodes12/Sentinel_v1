# Hardware explorer (Phase 5)

The Hardware page lets you explore the computer Sentinel discovered: an overview, a tree of
components per category, logical diagrams, live readings and an inspector that says where every
value came from. It shows **the hardware that exists, not the hardware Sentinel assumes exists**:
every shape is a discovered component, every relationship is one the kernel or firmware reported,
and nothing is drawn where it physically sits.

## 1. Architecture

```text
 sentinel-hardware ──scan──▶ SystemInventory ──IPC (unchanged)──▶ sentinel-client
   (read-only sysfs/proc)        (Phase 2 model,                     │  ComponentIndex::build(&inventory)
                                  schema v2)                          ▼
                                                     InventoryView { inventory, components }
                                                                      │  Tauri get_inventory
 sentinel-telemetry ──monitor──▶ TelemetrySnapshot ─get_metrics─▶     ▼
   (Phase 3 collectors)                                         React frontend
                                                                  useInventory  (on demand)
                                                                  useTelemetry  (one app-wide poll)
                                                                      │
                                                      app/explorer.ts (pure: maps, selection,
                                                      live resolution, spec rows, vocabulary)
                                                                      │
                                    HardwarePage ─ ComponentTree · ExplorerVisuals · Inspector
```

- **Discovery stays in Rust.** No React component reads the system or runs a command; the
  explorer's sources contain no timer, no `backend` call and no bridge import (checked by
  `explorer.test.tsx`).
- **One representation of the hardware.** The `SystemInventory` remains the only description of
  devices and their specifications. The `ComponentIndex` adds identity and relationships on top of
  it and carries only a `key` back to each record; it copies no specifications.
- **The daemon's IPC contract is unchanged.** The index is built in `sentinel-client` from the
  inventory the daemon (or the cache file) already returns, so a cached inventory gets the same
  explorer.
- **Topology is not rediscovered for telemetry.** The inventory is fetched when the service starts
  or a scan finishes; the index is rebuilt only then (`useMemo` on the index object). Telemetry
  updates re-render values only.

## 2. Normalised hardware model: `ComponentIndex`

Code: `crates/sentinel-inventory/src/components.rs` (pure, no I/O), mirrored in
`apps/desktop/src/types/inventory.ts`.

| Field | Meaning |
|---|---|
| `id` | Stable identifier from kernel names and bus addresses (table below) |
| `category` | processor, memory, graphics, storage, battery, network, usb, pci, motherboard |
| `kind` | cpu_package, cpu_core, logical_cpu, cache, memory, memory_device, numa_node, gpu, disk, partition, virtual_block, battery, power_supply, network_interface, usb_root_hub, usb_hub, usb_device, pci_device, pci_bridge, board |
| `name` | From detected values (model name, product string); falls back to the kernel name or numeric IDs, never to an invented model |
| `key` | The record in its inventory section (package ID, core index, CPU number, `level:kind:instance`, firmware record position, PCI address, kernel name) |
| `parent`, `children` | Hierarchy within the category |
| `parent_basis` | How the parent is known: `root`, `kernel_topology`, `bus_topology`, `firmware`, `parent_not_found` |
| `links` | Relations across the tree: `pci_function`, `controller`, `usb_device`, `cache`, `shared_by`, `built_on`, `used_by`, `local_cpu` |
| `telemetry` | Where live readings are: `cpu_total`, `logical_cpu{cpu}`, `memory`, `gpu{pci_address}`, `disk{name}`, `filesystem{device}`, `network_interface{name}`, `battery{name}` |

Identifiers:

| Component | ID | Stable because |
|---|---|---|
| CPU package / core / logical CPU | `cpu:0`, `cpu:0/core:3`, `cpu:0/cpu:6` | Package ID; core index ordered by lowest CPU number; kernel CPU number |
| Cache instance | `cpu:0/cache:l2-unified-3` | Level, type, instance ordered by lowest CPU |
| Memory, firmware memory device, NUMA node | `memory`, `memory/device:DIMM 0`, `memory/numa:0` | Firmware locator (position if none) |
| GPU, PCI function | `gpu:0000:03:00.0`, `pci:0000:03:00.0` | PCI address |
| Disk, partition, virtual block device | `storage:nvme0n1`, `storage:nvme0n1p2`, `storage:dm-0` | Kernel names |
| Battery, power supply | `battery:BAT0`, `power:AC` | Kernel names |
| Network interface | `net:wlp1s0` | Interface name |
| USB device | `usb:3-3`, `usb:usb1` | sysfs name (follows the port path) |
| Board | `board` | One per machine |

Rules the builder enforces (tested in `tests/hardware/explorer.rs`): identifiers are unique;
parents are listed before children; a child's parent lists it; links point only to components
that exist; a component whose reported parent was not discovered is **not attached to anything
else** but listed at the top level with `parent_not_found`; the index of a rescan of the same
hardware is identical.

## 3. CPU topology representation

Source: `/sys/devices/system/cpu/cpu*/topology` and `cache/index*` (Phase 2 discovery).

- Package → physical cores (kernel topology) → logical processors (hardware threads). Hybrid
  processors keep their core types (performance / efficiency, from `cpu_core`/`cpu_atom`) and
  their different thread counts.
- **Caches** are components of the package. Since Phase 5 the inventory keeps the sharing set of
  every instance (`instance_cpus`), so each instance links to the cores that share it: on the test
  laptop the L1d, L1i and L2 have one instance per core and the L3 is **one** instance shared by
  all four cores. The diagram draws each instance as one box spanning those cores. CPUs listed by
  the kernel but not present are dropped from the sharing set.
- NUMA nodes (memory category) link to their local CPUs.
- What is **not** shown: die and chiplet boundaries (shown only as the `die_id`/`cluster_id` the
  kernel reports), core positions, floorplans. The diagram is labelled "topology, not a die image";
  the inspector says "Position on the die: not known". The package ID is the kernel's numbering;
  the physical socket is not detectable from sysfs and the inspector says so.
- Instruction sets: notable extensions picked from the kernel's flag list (`sse4_2`, `avx`,
  `avx2`, `fma`, `avx512f`, `aes`, `sha_ni`, `vaes`, `vmx`, `svm`, ARM `asimd`, `sve`, ...), with
  the total flag count.

## 4. Memory topology representation

- Installed (firmware), kernel-managed and usable capacities are three separate bars, so the
  difference reserved by firmware and integrated graphics is visible; live use is drawn on the
  usable part only.
- Firmware memory-device records (SMBIOS type 17) become `memory_device` components **exactly as
  the firmware lists them**, including slots it lists as empty, with locator, bank label, form
  factor (soldered memory is usually "Row of chips"), speeds, manufacturer and part number.
- When the records cannot be read (administrator-only on most systems, or absent), no devices are
  shown and the page says why. Channels are **not** inferred from bank labels; slot positions are
  never drawn.

## 5. GPU capability model

A functional view per GPU: PCI function → GPU (type from Phase 2's labelled inference, driver,
DRM nodes) → memory. Memory is one of: *dedicated VRAM* (reported by the driver for a discrete
card), *system RAM reserved for the GPU* (integrated; "shared memory, not separate VRAM"), or
*configuration unknown* when the driver reports nothing. Live readings (busy, temperature, core
clock, power, VRAM or reserved-RAM and GTT use) come from the Phase 3 GPU collector, keyed by PCI
address, and appear only where the driver exposes them. Shader, compute or execution units are
not shown: Linux does not report them in a vendor-neutral way.

## 6. Storage hierarchy model

Physical device → partitions (sysfs) → block devices built on them (device-mapper, md RAID,
through `holders/` and `slaves/`) → filesystems and mount points (`/proc/mounts` at scan time) and
swap (`/proc/swaps`). Virtual devices without a parent (loop, zram) are top-level and marked
virtual. Each disk links to its controller's PCI function. The disk-layout diagram sizes
partitions by their reported sizes and orders them by number (not by position on the medium).
Unmounted partitions say "Not mounted at scan time; the filesystem of an unmounted device is not
known": Sentinel never reads partition tables or superblocks, never mounts and never tests a
drive. SMART health is not read (it needs administrator access).

Live: disk I/O by kernel name, filesystem usage by device node (`/dev/nvme0n1p7`,
`/dev/mapper/cryptroot`), the same keys the storage collector uses.

### Battery and network

- **Battery**: design, full-charge and current capacity as bars, in the unit the battery reports
  (energy in Wh, or charge in Ah; never converted between them). Capacities are firmware-reported;
  the share of design capacity is labelled derived; the view says full-charge capacity is the
  controller's estimate, not a measurement of the cells. Live charge, power and the runtime
  estimate (with its confidence) come from the Phase 3 battery collector.
- **Network**: every interface, physical ones linked to their PCI or USB device, with driver,
  link state and speed at scan time and live receive/send rates. MAC addresses are masked in the
  inspector (revealable only in the field table, as in Phase 2); IP configuration is not collected.

## 7. USB and PCI hierarchy model

- **USB**: root hubs (with their host controller's PCI function) → hubs → devices, from the
  kernel's parent links; each device shows vendor and product IDs, the strings the device itself
  reports and the ID-database names (labelled separately), class and interface classes, version,
  negotiated speed and the Linux port on its parent hub. Linux port numbers are not physical
  sockets; the inspector says the socket is not known.
- **PCI**: functions under the bridges the kernel reports (sysfs device path), with vendor/device
  IDs, class, subclass and programming interface, subsystem IDs, revision, driver and upstream
  bridge. This is bus topology, not a board diagram; the inspector says the position on the board
  is not known. A function whose upstream bridge was not discovered is marked "Parent not
  discovered". On the test laptop all 30 functions have their bridge discovered.

## 8. Provenance system

Every specification row in the inspector has a reliability label from one vocabulary, also used
by the field tables:

| Label | Meaning |
|---|---|
| Direct | Read directly from the device through a Linux hardware interface |
| Firmware | Firmware tables (SMBIOS/DMI) |
| Kernel | Reported by the Linux kernel or a driver (operating-system reported) |
| ID database | Name looked up by numeric ID in the installed `pci.ids` / `usb.ids` |
| Derived | Calculated from detected values by a fixed formula (new in Phase 5: battery health, memory in use, cache totals) |
| Inferred | Concluded from indirect evidence (microarchitecture, integrated vs dedicated GPU, storage interface) |
| Unknown | Source could not be established |
| Unsupported / Unavailable / Admin only / Invalid | Why there is no value |

The label's tooltip explains it and names the source path. The overview shows the full legend.
Confidence is not accuracy: a kernel-reported value can still be rounded. Live readings carry
their own quality from Phase 3 (measured, *Derived* for a core's mean of its threads, *Estimate*
for runtime). The inspector states when the specifications were scanned and when the live values
were sampled; stale values are labelled "Stale · last values received".

## 9. Frontend visualisation architecture

| File | Role |
|---|---|
| `app/explorer.ts` | Pure logic: index maps, ancestors, selection resolution, tree keyboard model, live state, live rows from telemetry refs, spec rows per kind, the reliability vocabulary |
| `components/ComponentTree.tsx` | WAI-ARIA tree: one tab stop, arrows move, right/left expand and collapse, Enter/Space selects; reveals a selection made elsewhere |
| `components/ExplorerVisuals.tsx` | `ProcessorDiagram` (existing `CpuTopology` with live load), `CacheHierarchy`, `MemoryMap`, `GpuDiagram`, `DiskLayout`, `BatteryCapacity`, `ExplorerOverview` |
| `components/Inspector.tsx` | One layout for every kind: identity, live, specifications with reliability, relationships with their basis, last updated |
| `components/Reliability.tsx` | Badge and legend |
| `pages/HardwarePage.tsx` | Layout: categories and tree, diagram, inspector; the Phase 2 field tables under "All recorded fields" |

Plain SVG, CSS grid and React; no new dependency. Diagram elements are buttons with accessible
names (for example "L3 cache, 4 MiB, shared by 4 cores"). The selected category and the
selection per category live in `App`, so they survive leaving the page; selections are component
IDs, so they survive refreshes and rescans. A selected component that disappears after a rescan
is reported ("Component no longer detected"), never silently replaced.

## 10. Live telemetry integration

The explorer receives the app-wide `useTelemetry` state (snapshot, monitoring report, stale
flag). `liveFor(component)` follows the component's telemetry references into that snapshot.
Nothing is collected twice; there is no subscription to clean up. Live values are shown only
while monitoring is active and the last poll succeeded; paused or failed polls show "Stale";
monitoring off shows no numbers at all. A component whose readings are not in the sample says
so ("Not in the network sample") rather than showing zero.

On the test laptop every live CPU, disk, filesystem, GPU, network interface and battery sample
maps to a component (`real_machine_index_is_well_formed_and_matches_live_telemetry`).

## 11. Performance

Measured on the test laptop (release build, monitoring on, Balanced profile), window process tree
including WebKit helpers:

| Page | Window CPU (% of one core, 60 s) | Window RSS |
|---|---|---|
| Overview | 3.23 % | 470.8 MiB |
| Telemetry (Phase 4 baseline page) | 5.35 % | 466.9 MiB |
| Hardware explorer, Processor, a core selected (live) | 2.00 % | 470.5 MiB |
| Hardware explorer, PCI tree | 2.21 % | 470.2 MiB |
| Hardware explorer, Storage | 2.05 % | 472.9 MiB |

One earlier 20-second sample of the PCI page read 16.9 %; it did not reproduce in a separate
20-second per-thread measurement (1.0 % with monitoring on, 1.35 % off) or in the 60-second run
above. It coincided with the end of a long AT-SPI tree walk by the test script and is recorded
here rather than discarded.

Rendering (static render of the whole page in Node on the test laptop, mean of 50 renders): 1.6–6.0 ms per
page; 89–662 elements; the processor page has the only SVG (74 shapes for 8 cores/16 threads,
86 for 12 hybrid cores). Category switch in the real window, including the AT-SPI round trip:
mean 0.31 s, slowest 0.61 s. Bundle: 410 kB JavaScript (119 kB gzip).

## 12. Limitations

- Physical layout of anything (die, board, slots, sockets) is out of scope until verified
  documentation exists (Phase 8).
- Filesystem type of unmounted partitions, SMART health, IP configuration and memory channel
  layout are not shown.
- Memory-device records are administrator-only on most systems, including the test laptop;
  Sentinel never asks for elevated privileges, so the module view is tested on fixtures only.
- Only one physical machine (AMD) was available; Intel hybrid, NVIDIA and multi-socket layouts
  are fixture-tested only. See [HARDWARE_COMPATIBILITY.md](HARDWARE_COMPATIBILITY.md).
