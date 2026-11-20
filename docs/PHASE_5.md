# Phase 5: Hardware explorer and component visualisation

Status: **COMPLETE** for the applicable definition of done (section K), with physical validation
limited to the one available machine. Phase 6 was not started.

Statuses used below: **Physically validated** (real hardware or the real window, checked against
an independent source), **Mock tested** (synthetic fixtures), **Not tested**.

## Stage 1 audit: what existed and what was missing

Phase 2 already discovered nearly everything the explorer needs, with provenance on every value
(`Field<T>`), and Phase 3 already keyed its samples by the same kernel names. The Hardware page
was a static list of field tables with one logical CPU diagram. Gaps found and closed:

| ID | Finding | Resolution |
|---|---|---|
| P5-01 | Cache discovery kept the sharing set of the first instance only, so "which cores share this L2" could not be shown | `CacheInfo.instance_cpus` records every instance |
| P5-02 | Battery health and memory-in-use were labelled *inferred* although they are calculated | New confidence `derived`; one existing assertion moved from `Inferred` to the more precise `Derived` |
| P5-03 | Partitions had no filesystem, mount or layering information | `BlockUse` (mounts, swap, holders), `slaves`, mapper name and device-node paths, from `/proc/mounts`, `/proc/swaps` and sysfs |
| P5-04 | No stable identity or relationships across sections (GPU ↔ PCI, disk ↔ controller, cache ↔ cores) | `ComponentIndex` |
| P5-05 | Phase 4 had not completed physical items (suspend, discharge, other vendors) | Not blocking: none of them is an explorer dependency. They stay open in PHASE_4.md |

Issues found while building and testing Phase 5:

| ID | Issue | Resolution |
|---|---|---|
| P5-06 | The safety suite's source scan rejected a `"/dev` literal in the index builder (keys built outside discovery) | Fixed properly: discovery records device-node paths; the index only copies them. The scan was not changed |
| P5-07 | Frontend fixture export was not deterministic (scan durations, module timestamps) | Normalised before export; the currency test now passes on every run |
| P5-08 | WebKitGTK did not expose nested `<span>` text (GPU memory block) to AT-SPI, so screen readers missed it | Rendered as paragraphs; now visible to AT-SPI |
| P5-09 | The battery view drew only energy (Wh); this laptop's battery reports charge (Ah), so its bars were empty | Bars use the battery's own unit, never converting; physically validated |
| P5-10 | One 20-second sample of the PCI page read 16.9 % window CPU | Not reproduced (1.0 %, 1.35 %, then 2.21 % over 60 s). Recorded in HARDWARE_EXPLORER.md §11; worth re-measuring on other machines |
| P5-11 | Phase 4's window scenarios expected the old Hardware heading "INVENTORY" | Updated to "HARDWARE EXPLORER" in two places; the checks are otherwise unchanged |

## A. Executive summary

The Hardware page is now an interactive explorer of the hardware Sentinel discovered. It
provides:

- an overview of every category, with live values when monitoring is on;
- a component tree per category, keyboard accessible;
- logical diagrams: core topology with live per-CPU load, cache sharing, memory capacity, GPU
  functional blocks, disk layouts and battery capacity;
- USB and PCI hierarchies;
- a reusable inspector, which shows for any component its specifications with a reliability
  label, its live readings, its relationships and how they are known, and when each was obtained.

The explorer is built on a new component index derived from the existing inventory. It uses the
existing telemetry and does no polling of its own.

On the development laptop, the real window shows what independent `/sys` and `/proc` reads show.
That covers the topology, caches, mounts, PCI/USB counts and battery capacities.

## B. Architecture changes

- **Model (Rust, `sentinel-inventory`)**:
  - Schema version 2 adds cache instances, block-device use and layering, and device-node paths.
  - New confidence `Derived`.
  - New pure module `components.rs` with `ComponentIndex::build`: stable IDs, parent/children
    with their basis, cross-links and telemetry references. Section 2 of HARDWARE_EXPLORER.md has
    the details.
- **Discovery (`sentinel-hardware`)**:
  - CPU caches record every instance.
  - Storage reads `/proc/mounts`, `/proc/swaps`, `holders/`, `slaves/` and `dm/name`. These are
    read-only and on the discovery allow-list.
  - The mount-path unescaping moved here from telemetry, and telemetry now uses it.
- **Client (`sentinel-client`)**: `InventoryView.components` is built from the inventory. The
  daemon and the IPC protocol are unchanged.
- **Frontend**:
  - Pure logic in `app/explorer.ts`.
  - Components `ComponentTree`, `ExplorerVisuals`, `Inspector` and `Reliability`.
  - `HardwarePage` lays them out and keeps the Phase 2 field tables under "All recorded fields".
  - `App` holds the explorer's category and per-category selection.

## C. Implemented visualisations

| Category | Visualisation |
|---|---|
| Overview | Tiles per category (count, key specification, live value), reliability legend |
| Processor | Package → cores → logical CPUs tree; topology diagram with live per-CPU load; cache hierarchy with each instance spanning its sharing cores; inspector for package, core, logical CPU, cache |
| Memory | Installed / kernel-managed / usable / swap bars with live use; firmware memory devices as listed; NUMA nodes |
| Graphics | PCI function → GPU → memory (dedicated / reserved system RAM / unknown) with live busy, temperature, clock, power |
| Storage | Disk → partitions → device-mapper volumes; proportional disk layout; mounts, swap, live I/O and filesystem usage |
| Battery | Design / full-charge / current capacity bars in the battery's own unit; live charge, power, runtime estimate |
| Network | Interfaces linked to their PCI/USB device; live traffic; masked MAC |
| USB | Root hubs → hubs → devices, Linux port numbers, controller link |
| PCI | Bridges → functions; IDs, class/subclass/prog-if, driver; unknown parents marked |
| Motherboard | Board identity (Phase 2), as an inspector entry |

## D. Live telemetry integration

Each component carries telemetry references keyed exactly as the Phase 3 collectors key their
samples:

- the kernel CPU number;
- the PCI address;
- the disk's kernel name;
- the filesystem's device node;
- the interface name;
- the battery name.

The explorer resolves those references in the app-wide telemetry snapshot. No collector was
added, nothing is sampled twice, and the explorer has no timers or subscriptions to clean up
(checked by a source test).

- A core's utilisation is the mean of its logical CPUs, labelled derived.
- Paused or failed polling is shown as stale; with monitoring off, no numbers are shown.
- The index is rebuilt only when a new inventory arrives, never on a telemetry update.
- On the real machine, every live sample maps to a component (Rust test against the live engine).

## E. Hardware validation

Physical machine: Dell Inspiron 15 3535, AMD Ryzen 3 7320U (4 cores / 8 threads, 4 MiB L3),
Radeon 610M (amdgpu), NVMe SSD with 7 partitions, Wi-Fi, charge-reporting battery (BAT1); Ubuntu
26.04, kernel 7.0, GNOME 50.1 on Wayland.

- **Physically validated** with `tests/desktop-e2e/explorer_scenarios.py` (12/12 in the real
  window, each result checked against an independent read):
  - CPU tree: 1 package, 4 cores and 13 cache instances, matching sysfs.
  - Core threads, matching `core_cpus_list`.
  - The L3 is one instance shared by all four cores, matching `shared_cpu_list`.
  - Live per-core load appears in the inspector.
  - Selection is kept across a page change and continued refreshes.
  - The root partition and its filesystem match `/proc/mounts`.
  - PCI (30) and USB (3) counts match sysfs, and every PCI function has a discovered bridge.
  - The GPU → PCI link works.
  - Battery capacities match sysfs in Ah.
  - Two MAC addresses are never shown unmasked.
- **Physically validated** by `real_machine_index_is_well_formed_and_matches_live_telemetry`: 119
  components, structure invariants, and the mapping of every CPU, disk, filesystem, GPU,
  interface and battery sample to a component.
- **Mock tested only**, on fixture machines whose `/sys` and `/proc` trees are generated by
  `tests/support/hwfixture.rs`:
  - an Intel hybrid laptop (4 P-cores + 8 E-cores, Intel iGPU plus NVIDIA dGPU, two firmware
    memory records, SATA SSD, two batteries);
  - an AMD laptop with a LUKS volume, a USB stick and missing PCI bridges;
  - a desktop with placeholder DMI and no battery;
  - an ARM board.
- **Not tested**: real Intel and NVIDIA hardware; readable firmware memory records (root-only on
  this laptop); NUMA; real LVM, LUKS or RAID; multi-socket machines; KDE and X11 (see Phase 4).

## F. UI and UX changes

The Hardware page now has three columns: on the left, the categories with counts, the
component tree for the current category and the scan control; in the centre, that category's
diagram; on the right, the inspector. The diagrams and the tree share one selection. Clicking a
cross-link, such as a GPU's PCI function, moves to that component in its own category.

- **Design**: the existing dark technical design is unchanged, with accents only for selection,
  live and derived/inferred states, and no animation, gradient or 3D.
- **Accessibility**: the tree follows the WAI-ARIA tree pattern, and diagram elements are
  labelled buttons such as "L3 cache, 4 MiB, shared by 4 cores".
- **Retained views**: the Phase 2 field tables, including MAC reveal and virtual-device
  toggling, sit under "All recorded fields", and an inventory without an index (old cache)
  still shows them.
- **Missing selection**: a component that is gone after a rescan is reported as "Component no
  longer detected", not replaced by another.

## G. Performance measurements

The details and method are in HARDWARE_EXPLORER.md §11. On the test laptop, with monitoring on
(Balanced) and 60 s per page, the window's process tree used:

| Page | CPU | Memory (RSS) |
|---|---|---|
| Processor, with a core selected | 2.00 % | about 470 MiB |
| PCI | 2.21 % | about 470 MiB |
| Storage | 2.05 % | about 473 MiB |
| Overview | 3.23 % | about 471 MiB |
| Telemetry | 5.35 % | about 467 MiB |

The explorer adds no measurable memory and costs less CPU than the Telemetry page. One
unreproduced 16.9 % sample is recorded as P5-10.

- Static render: 1.6–6.0 ms per page, 89–662 elements; the largest SVG has 86 shapes.
- Category switch in the real window: mean 0.27–0.31 s, including AT-SPI.
- JavaScript bundle: 410 kB (119 kB gzip).
- Daemon: unchanged (no new collector; discovery adds two small `/proc` reads per scan).

## H. Test results

| Suite | Result |
|---|---|
| Rust workspace (`cargo test --workspace`) | **385 passed, 0 failed** (36 test binaries). Phase 4 ended at 371; 14 new: 12 in `tests/hardware/explorer.rs`, 2 parser tests |
| Frontend (`npm test`) | **93 passed, 0 failed** (66 from earlier phases, 27 new in `explorer.test.tsx`) |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `tsc --noEmit` | Clean |
| Real window, explorer (`explorer_scenarios.py`) | **12/12** (resource run: 11/11 with 60 s per page, before the battery fix, which touched only the battery view) |
| Real window, Phase 4 regression (`window_scenarios.py`) | **12/12** |
| Headless browser, Phase 4 regression (`browser_settings.mjs`) | **7/7** |
| Release build: `sentineld`, `sentinel`, `sentinel-desktop` (`tauri build --no-bundle`) | Built |

No existing test was deleted or weakened. Three assertions were updated to match deliberate
changes, and each is still at least as strict:

- `used_bytes` is now `Derived`.
- The cache round-trip test uses the schema constant, and it now also asserts that a version-1
  cache is rejected.
- Phase 4's expected Hardware heading was updated.

`proc/mounts` and `proc/swaps` were added to the discovery allow-list for the new storage reads.

## I. Safety verification

- **Read-only, as before.** The new reads are `/proc/mounts`, `/proc/swaps` and sysfs `holders/`,
  `slaves/` and `dm/name`, all on the allow-list. The real-machine test that records every path
  discovery touches passes: only `sys`, `proc`, `etc` and `usr`.
- **Never accessed:**
  - partition tables and superblocks;
  - SMART data;
  - `/dev` nodes;
  - firmware memory records, which are root-only (privileges are never escalated).
- **Not added:**
  - writes;
  - commands;
  - network access;
  - uploads;
  - automatic fixes;
  - any setting change.
- **Safety suite:** 20/20, including the source scan that caught P5-06.
- **Privacy:** MAC addresses stay masked in the explorer, and fixtures contain only invented
  identifiers.

## J. Outstanding limitations

- **No physical layout.** There is no die floorplan, board layout, slot position or socket
  mapping; that is deliberate until verified documentation exists (Phase 8).
- **Filesystems.** The type of an unmounted partition is unknown. Mounts are those at scan time,
  so a filesystem mounted afterwards appears only after a rescan.
- **Memory.** Channel layout is not inferred, and the module view is mock tested only on this
  machine.
- **Not collected:** SMART health and IP configuration.
- **GPU internals.** No shader, compute or execution units are shown.
- **Not tested** on Intel, NVIDIA, NUMA, LVM, RAID or multi-socket hardware, nor on KDE or X11.
- **Carried over from Phase 4, still awaiting your decision:**
  - P4-15: the window keeps polling while minimised on GNOME Wayland.
  - P4-24: database write amplification.
- **Phase 4 physical items still open:** real suspend/resume, a full battery discharge, and other
  desktops.

## K. Phase completion status

**COMPLETE.** Every item of the plan's definition of done (§20) is satisfied:

- The explorer, navigation, categories, selection and detail panels are in place.
- CPU identification, topology, logical processors, caches, per-core telemetry, and honest
  reporting of unsupported topology.
- Memory capacities, live use, devices where the firmware lists them, and no fabricated slots.
- GPU identification and capabilities, integrated vs dedicated memory, and unavailable metrics.
- Storage devices, logical layering, partitions and filesystems.
- Battery, network, USB and PCI.
- Provenance on every value and stable IDs, with live updates and no duplicate collectors.
- Missing data is handled honestly, with no rendering performance problems.
- All tests pass, the release build succeeds, nothing modifies the OS and nothing repairs.

"Where available" items were validated physically where this laptop has them, and by fixtures
otherwise. Section E lists exactly which.

## L. Next phase

Phase 6, automatic schematic discovery and verification, can start from:

- `BoardIdentity` and its search terms (Phase 2);
- stable component IDs for every device it may need to match against documentation, such as
  `pci:…`, `usb:…`, `memory/device:…` and `board`;
- PCI vendor, device and subsystem IDs, and USB vendor and product IDs, per component;
- the provenance vocabulary, which verified documentation can extend without changing the
  model.

Verified physical placement would attach to component IDs without changing the logical index.
Phase 6 was not started.
