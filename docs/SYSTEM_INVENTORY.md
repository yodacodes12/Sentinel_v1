# System inventory

Code: `crates/sentinel-inventory` (types only, no I/O). Shared by the discovery crates, the daemon,
the IPC protocol, the client library, the CLI and, mirrored in `apps/desktop/src/types/inventory.ts`,
the frontend.

## Data provenance: `Field<T>`

Every discovered value is a `Field<T>`:

| Part | Meaning |
|---|---|
| `value` | Present **only** when the value was obtained |
| `availability` | `detected`, `unknown`, `unsupported`, `unavailable`, `permission_denied`, `invalid` |
| `confidence` | `direct`, `firmware_reported`, `kernel_reported`, `database`, `derived`, `inferred`, `unknown` |
| `source` | Where it came from, e.g. `/sys/class/dmi/id/board_name` |
| `notes` | Plain-language explanation (why missing, how inferred) |

Rules enforced by the type:

- A value exists exactly when availability is `detected`. The constructors make the opposite
  impossible, and deserialisation (cache files) rejects an inconsistent field.
- Unavailable information is never converted to zero or an empty string.
- Confidence can only be lowered (`at_most`), never raised: an inferred value never gets the
  authority of a direct reading.
- Firmware placeholders become `invalid` with an explanation.

The discovery timestamp of a field is the `discovered_at` of the record that contains it (each
section of the inventory carries one) rather than a per-field copy.

Confidence categories:

| Confidence | Meaning |
|---|---|
| `direct` | Read from the device's own interface (e.g. a USB descriptor, a sysfs device attribute) |
| `firmware_reported` | SMBIOS/DMI; vendors sometimes fill these in carelessly |
| `kernel_reported` | The kernel or a driver says so |
| `database` | Looked up by numeric ID in `pci.ids`/`usb.ids`; names can be generic or stale |
| `derived` | Calculated from other detected values by a fixed formula (battery health = full ÷ design capacity; memory in use = `MemTotal` − `MemAvailable`). As good as its inputs, not itself a reading. Added in Phase 5 |
| `inferred` | Concluded from indirect evidence (a lookup table, a device's place in the kernel tree); can be wrong even when the inputs are right (see HARDWARE_DISCOVERY.md) |
| `unknown` | Source or reliability cannot be established |

## The inventory

`SystemInventory` (schema version 2 since Phase 5; version-1 caches are ignored and the next scan replaces them) holds: `discovered_at`, `duration_ms`, an optional
`root_override` (set when read from a test tree; every UI labels such an inventory as not
describing this machine), the `fingerprint`, `system`, `firmware`, `motherboard`, `processors`,
`memory`, `graphics`, `storage`, `batteries`, `power_supplies`, `network`, `usb_devices`,
`pci_devices`, `sensors`, `thermal_zones`, and `discovery_status`: one `ModuleReport` per module
with its outcome (`completed`, `partial`, `unavailable`, `failed`, `cancelled`), item count,
duration and notes. Every section is optional or a list, so partial results are normal.

Units are in field names and follow the kernel: `_bytes`, `_khz`, `_uwh`, `_uah`, `_uv`, `_ua`,
`_uw`, `_kbps`, `_mts`. Display code converts them.

## Motherboard identity contract

`BoardIdentity` (`board.rs`) is the interface future schematic-discovery modules consume. It holds
system manufacturer, model, family and SKU; board manufacturer, name and revision; BIOS vendor,
version and date; and an `identification` assessment (`board_identified`, `system_only`,
`unidentified`) with `externally_verified` always false until documentation matching exists.
`search_terms()` produces the only strings a future search may use: detected, non-sensitive model
identifiers. Serial numbers and UUIDs are not in the type at all.

## Hardware fingerprint

`fingerprint.rs`: a 64-bit FNV-1a hash of manufacturer, product, board manufacturer/name/revision,
CPU model and the sorted PCI vendor:device IDs. It answers "is this the same hardware profile as my
cached inventory?". Limitations: identical machines share it; adding or removing a PCI device or a
firmware update that changes DMI strings changes it; it is not secret and not collision-resistant
against deliberate attack. It is never sent anywhere. Serials, UUIDs and MAC addresses are not
inputs.

## Cache

| | |
|---|---|
| Where | `scan_cache` table in `$XDG_DATA_HOME/sentinel/sentinel.db` (storage migration 2) |
| Keys | `hardware_inventory`, `utility_catalog` |
| Format | JSON document plus its schema version and timestamps |
| Written | By the daemon after a scan completes; never after a cancelled or timed-out scan |
| Read | By the daemon at start-up; by clients directly (read-only, nothing created) when the daemon is stopped |
| Versioning | An entry with another schema version is ignored and logged, never reinterpreted |

How freshness is shown: `InventoryOrigin::ThisSession` (scanned by the running daemon, still a
snapshot) or `Cache` (from an earlier scan). When the daemon is stopped the source is the cache
file. The CLI and every page label cached data as cached and never as live.

## Visualisation contract

Diagrams are generated from these records only:

- The CPU topology diagram draws exactly the cores and logical CPUs in `ProcessorInfo.cores`. No
  core is drawn that was not discovered; missing topology means no diagram.
- Memory modules are drawn only from SMBIOS records (`MemoryInfo.modules`); when those are not
  readable, no slot layout is shown.
- No motherboard drawing exists. The Motherboard page shows `BoardIdentity` and the state
  "Motherboard identity detected — schematic analysis not yet available."
- Rendering lives in the frontend (`app/inventoryView.ts`, `components/CpuTopology.tsx`), separate
  from discovery.

## Phase 5 additions (schema version 2)

- `CacheInfo.instance_cpus`: the logical CPUs of **every** instance of a cache level, not only the
  first (`shared_by` is kept and equals `instance_cpus[0]`). This is how the explorer shows that an
  L3 is one cache shared by all cores while each L2 belongs to one core.
- `Partition.device_path`, `StorageDevice.mapper_name` / `mapper_path`, `StorageDevice.slaves`.
- `BlockUse` on every storage device and partition, read at scan time: `mounts`
  (`Field<Vec<Mount>>` from `/proc/mounts`: mount point, filesystem type, read-only; detected and
  possibly empty when the table was readable, `unavailable` when it was not), `swap`
  (`Field<bool>` from `/proc/swaps`) and `holders` (sysfs `holders/`, e.g. the `dm-0` built on a
  partition). Partition contents are never read, so an unmounted partition's filesystem is not
  known and is shown as such.

The hardware explorer's **component index** (`sentinel_inventory::ComponentIndex`) is built
from the inventory, not stored in it; see [HARDWARE_EXPLORER.md](HARDWARE_EXPLORER.md).
