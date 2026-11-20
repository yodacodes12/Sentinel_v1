# SENTINEL — PHASE 1
## Project Foundation, Architecture & Initial Application Skeleton

**Project:** Sentinel
**Platform:** Linux desktop
**Phase:** 1 — Foundation
**Development status:** Starting from scratch
**Primary objective:** Establish a safe, modular, lightweight and extensible foundation for the Sentinel application.

---

# 1. YOUR ROLE

You are the lead software architect and senior Linux systems engineer responsible for building Sentinel from scratch.

You must establish the foundation of a production-quality Linux desktop application.

Do not treat this as a simple dashboard project.

Sentinel is intended to become a unified Linux System Intelligence and Hardware Monitoring Hub. It will eventually discover hardware, monitor system resources, analyse system health, discover installed system utilities, locate motherboard schematics, interpret hardware diagrams and display interactive 2D representations of the user's actual hardware.

However, this document covers **Phase 1 only**.

Your responsibility is to establish the engineering foundation that allows these capabilities to be implemented in later phases without requiring architectural rewrites.

Follow the architecture, safety rules and development standards in this document.

Do not attempt to implement all future features during Phase 1.

# 2. PRODUCT IDENTITY

Sentinel is a native Linux desktop application accompanied by a lightweight background monitoring service and a command-line interface.

Its purpose is to provide users with one central place to understand their entire computer.

Sentinel will eventually bring together:

- Hardware identification and inventory.
- CPU and processor architecture information.
- RAM and memory analysis.
- Process monitoring.
- Battery and power information.
- Storage and disk health.
- GPU and graphics information.
- Network monitoring.
- USB and peripheral discovery.
- Motherboard schematic discovery.
- Interactive motherboard visualisation.
- System health analysis.
- Hardware diagnostics.
- Installed system utility discovery.
- System services and startup information.
- System event history.
- Desktop notifications.
- Maintenance recommendations.

Sentinel should provide its own monitoring and analysis wherever possible.

It should also discover existing Linux utilities and allow users to open them when appropriate.

Sentinel is a unified hub, not a replacement package manager or an application that attempts to take control of the Linux operating system.

# 3. ABSOLUTE SAFETY POLICY

This is the most important section of the entire project.

Sentinel MUST NEVER modify the user's operating system or perform corrective actions.

Sentinel is strictly an observer, analyser and recommender.

## 3.1 Prohibited operations

Sentinel must never independently:

- Install or uninstall applications or packages.
- Update or upgrade the operating system.
- Modify system configuration files.
- Modify BIOS or firmware settings.
- Change CPU frequencies or voltage settings.
- Change GPU frequencies or voltage settings.
- Change power-management settings.
- Change network configurations.
- Enable, disable, start or stop unrelated system services.
- Kill, terminate or suspend user processes.
- Modify disk partitions.
- Format or mount drives automatically.
- Repair filesystems.
- Modify bootloader configuration.
- Modify kernel parameters.
- Modify firewall rules.
- Change permissions of unrelated system files.
- Execute maintenance scripts.
- Apply recommended fixes.
- Execute shell commands intended to repair a detected issue.

The prohibited operations apply to the daemon, CLI, desktop application, background tasks and all future modules.

## 3.2 Recommendations are informational only

Sentinel may detect a problem and explain:

1. What happened.
2. Which hardware or software component is affected.
3. Why the issue matters.
4. What evidence supports the diagnosis.
5. What the user could consider doing.
6. Where the user can learn more.

For example:

**Detected:** High memory pressure.

**Explanation:** Available memory is low and the system is experiencing memory pressure.

**Recommendation:** Inspect applications consuming the most memory and consider closing applications that you no longer need.

Sentinel must not close those applications.

Another example:

**Detected:** Disk health warning.

**Recommendation:** Review the drive's SMART health information and consider backing up important files before investigating further.

Sentinel must not repair, format or replace anything.

## 3.3 Read-only architecture

Design the backend around read-only system inspection.

Prefer:

- `/proc`
- `/sys`
- udev monitoring
- read-only hardware discovery
- system information APIs
- systemd status queries
- journal inspection
- read-only SMART and NVMe health queries
- package database inspection
- desktop application discovery

The application may write its own configuration, logs, cache, history and database files inside its designated user-data directory.

These are application-owned files, not system configuration files.

## 3.4 No root daemon

Never run the entire Sentinel daemon as root.

The daemon must operate under the logged-in user account.

Do not request administrator privileges simply to make hardware information easier to collect.

If a future feature requires elevated privileges to read particular hardware information, that feature must be designed separately with narrowly scoped access and a documented security review.

Phase 1 must not introduce a privileged helper.

## 3.5 Fail safely

If an operation is unsupported, inaccessible or fails:

- Report the limitation.
- Log the error safely.
- Preserve the user's system state.
- Do not attempt alternative commands that modify the system.
- Do not silently escalate privileges.
- Do not invent hardware information.

A monitoring failure must never turn into a system modification.

# 4. APPROVED TECHNOLOGY STACK

Use the following stack unless technical validation identifies a concrete incompatibility.

## Backend

**Rust**

Responsibilities:

- Core application logic.
- Background daemon.
- Hardware discovery in future phases.
- Telemetry collection in future phases.
- IPC.
- CLI.
- Data processing.
- Event handling.

Use stable Rust.

Use Cargo workspaces to separate the backend modules.

## Desktop framework

**Tauri 2**

Responsibilities:

- Native Linux desktop application.
- Window management.
- Desktop application lifecycle.
- Communication with the frontend.
- Packaging integration.

Do not use Electron.

## Frontend

- React
- TypeScript
- Vite
- Tailwind CSS

Use a modular component architecture.

Use SVG for the future interactive 2D hardware and motherboard visualisations.

## CLI

Use Rust with Clap.

The CLI and desktop application must share the same backend interfaces and core functionality.

Do not implement two independent application backends.

## Local storage

Use SQLite for future structured application data.

Use an appropriate local configuration format for user preferences.

All application-owned data must remain local.

## Logging

Use structured Rust logging with tracing and tracing-subscriber.

Logs must not expose sensitive user information, private file contents, passwords, tokens or unnecessary hardware identifiers.

## IPC

Use Unix domain sockets for communication between the desktop application and the Sentinel daemon.

Define a versioned and typed IPC protocol.

Do not expose an unauthenticated network API.

# 5. HIGH-LEVEL ARCHITECTURE

Establish the following architecture:

```text
                    SENTINEL
                        |
          +-------------+-------------+
          |             |             |
       Desktop         CLI         Background
       App                           Daemon
          |             |             |
          +-------------+-------------+
                        |
                   IPC Protocol
                        |
                 Sentinel Core
                        |
          +-------------+-------------+
          |             |             |
       Config         Logging       Storage
                        |
                Future Service APIs
                        |
       +----------------+----------------+
       |                |                |
   Hardware          Telemetry       Intelligence
   Discovery          Engine           Engine
```

The desktop application must not directly implement hardware monitoring.

The backend owns hardware discovery, data collection and analysis.

The frontend receives structured information through defined interfaces.

Future modules must be replaceable and independently testable.

# 6. REPOSITORY STRUCTURE

Create a clean project structure.

Use this as the initial target:

```text
sentinel/
│
├── apps/
│   ├── desktop/
│   │   ├── src/
│   │   │   ├── app/
│   │   │   ├── components/
│   │   │   ├── layouts/
│   │   │   ├── pages/
│   │   │   ├── hooks/
│   │   │   ├── services/
│   │   │   ├── styles/
│   │   │   └── types/
│   │   │
│   │   ├── src-tauri/
│   │   └── package.json
│   │
│   └── cli/
│       └── src/
│
├── crates/
│   ├── sentinel-core/
│   ├── sentinel-daemon/
│   ├── sentinel-ipc/
│   ├── sentinel-config/
│   ├── sentinel-storage/
│   └── sentinel-logging/
│
├── resources/
│   ├── icons/
│   └── systemd/
│
├── tests/
│   ├── unit/
│   ├── integration/
│   └── fixtures/
│
├── docs/
│   ├── ARCHITECTURE.md
│   ├── SAFETY_POLICY.md
│   ├── SECURITY.md
│   ├── DEVELOPMENT.md
│   └── PHASE_1.md
│
├── Cargo.toml
├── package.json
├── README.md
└── .gitignore
```

You may refine the structure if necessary, but keep clear separation between the desktop frontend, daemon, CLI, core logic and supporting services.

Do not create empty modules for every future feature merely to make the project appear complete.

Create the modules needed for Phase 1 and establish documented extension points for later phases.

# 7. DAEMON FOUNDATION

Create the Sentinel background daemon.

The daemon is a separate process from the desktop application.

It must:

- Start under the current logged-in user.
- Expose a local IPC endpoint.
- Respond to health and status requests.
- Handle graceful shutdown.
- Detect an already-running instance.
- Handle stale socket files safely.
- Recover cleanly from unexpected shutdown.
- Produce structured logs.
- Avoid unnecessary background activity.

For Phase 1, the daemon is a foundation only.

Do not implement continuous hardware telemetry yet.

Do not run expensive background tasks.

Do not scan the entire filesystem.

Do not start hardware discovery automatically.

## User control

The user must control whether Sentinel is running.

The desktop window and daemon must have independent lifecycles.

Closing the desktop window must not automatically stop the daemon.

Stopping the daemon must stop Sentinel's background activity.

Installing Sentinel must not automatically enable persistent background monitoring.

If a systemd user-service unit is provided, make it manually controllable. Do not silently enable it during installation.

# 8. CLI FOUNDATION

Create the initial CLI.

The following command structure should be supported:

```bash
sentinel --help
sentinel --version

sentinel start
sentinel stop
sentinel status

sentinel ui
sentinel logs
```

Phase 1 command behaviour:

### `sentinel start`

Start the daemon under the current user.

If it is already running, report its status without creating another instance.

### `sentinel stop`

Request graceful daemon shutdown.

If it is not running, report that fact.

### `sentinel status`

Display:

- Sentinel version.
- Daemon running/stopped state.
- Daemon uptime, if running.
- IPC connection status.
- Current operating mode.
- Configuration location.

### `sentinel ui`

Launch the desktop application.

### `sentinel logs`

Display or follow Sentinel's own logs.

Do not use this command to expose unrestricted system logs.

All commands must provide clear errors and meaningful exit codes.

Do not implement future hardware commands during this phase.

# 9. DESKTOP UI — APPROVED DESIGN

The visual direction has already been approved.

Use the generated Sentinel dashboard reference image from the conversation as the design reference.

Do not replace the visual direction with a generic dashboard template.

## Design language

- Nearly black background.
- Dark charcoal panels.
- White and light-grey text.
- Bright cyan, blue, orange, purple and green accents.
- Thin, subtle panel borders.
- High-contrast hardware diagrams.
- Technical typography.
- Clean spacing.
- Consistent iconography.
- Professional engineering-workstation appearance.

The application should feel like a serious system intelligence tool.

Avoid unnecessary gradients, excessive glass effects, excessive rounded cards and decorative elements that reduce information density.

## Main dashboard layout

The top header should include:

- Sentinel logo and name.
- Detected device identity placeholder.
- Monitoring status.
- Global ON/OFF control.
- Window controls.

The left sidebar should contain:

- Overview.
- Hardware.
- Motherboard.
- Telemetry.
- Schematic Library.
- Diagnostics.
- Installed Utilities.
- Services and Startup.
- Alerts and Events.
- Settings.

Future navigation entries may initially display a clearly labelled development placeholder.

Do not fabricate monitoring information.

## Dashboard layout

The first screen should have:

1. System overview.
2. Hardware summary cards.
3. Motherboard visualisation panel.
4. Component information panel.
5. System telemetry panel.
6. Power mode and monitoring status.

The motherboard visualisation should occupy a prominent area.

Use the same visual hierarchy and composition as the approved reference image.

## Placeholder policy

Until real hardware discovery is implemented:

- Display an honest empty state or clearly marked demonstration data.
- Never present sample readings as actual readings.
- Never claim a motherboard has been identified when it has not.
- Never display an invented schematic as the user's actual motherboard.
- Make development/demo data visually distinguishable from live data.

The initial UI should still look polished and complete.

# 10. APPLICATION STATE AND CONFIGURATION

Create a central configuration system.

At minimum, establish these settings:

```rust
struct SentinelConfig {
    monitoring_enabled: bool,
    power_mode: PowerMode,
    notifications_enabled: bool,
    theme: Theme,
}

enum PowerMode {
    Eco,
    Balanced,
    Performance,
}

enum Theme {
    Dark,
    Black,
}
```

These settings establish the configuration model.

Do not pretend that power profiles already control real telemetry intervals during Phase 1.

Validate configuration when loading it.

Handle missing, corrupted and outdated configuration files safely.

Create a version field for future configuration migrations.

Use appropriate user-specific Linux directories:

- Configuration: XDG_CONFIG_HOME.
- Application data: XDG_DATA_HOME.
- Cache: XDG_CACHE_HOME.
- Logs: XDG_STATE_HOME or another documented application-specific location.

Do not hardcode a user's home directory.

# 11. IPC AND SERVICE CONTRACTS

Define typed IPC messages.

At minimum, support:

```text
GetDaemonStatus
StartMonitoring
StopMonitoring
PauseMonitoring
ResumeMonitoring
GetApplicationVersion
GetConfiguration
UpdateConfiguration
```

For Phase 1, implement the daemon lifecycle and status operations.

Monitoring commands should have safe, explicit placeholder behaviour until the actual monitoring engine is implemented.

Never acknowledge a monitoring operation as successful if the requested state was not actually achieved.

The protocol must include:

- Message version.
- Request identifier.
- Request type.
- Response type.
- Error representation.

Implement connection timeouts, malformed-message handling and graceful disconnection.

Validate all incoming requests.

Do not expose arbitrary command execution through IPC.

# 12. SECURITY AND PRIVACY FOUNDATION

Sentinel is a local application.

For Phase 1:

- No user accounts.
- No cloud backend.
- No telemetry upload.
- No analytics.
- No remote control.
- No remote command execution.
- No automatic software downloads.
- No unnecessary network listeners.

Future schematic discovery is allowed to search the internet for hardware documentation, but this will be implemented in a later phase.

Never upload private system information as part of a schematic search.

Use local hardware identifiers and carefully constructed search queries.

Do not expose serial numbers, UUIDs or personally identifying machine information in logs or public requests.

Set restrictive permissions on application-owned configuration, database, logs and IPC resources.

# 13. POWER EFFICIENCY FOUNDATION

Sentinel must be designed for low-resource Linux laptops.

Do not assume that the machine has a dedicated GPU.

The application must work using CPU-only processing.

Phase 1 performance requirements:

- No continuous hardware polling.
- No unnecessary background threads.
- No busy-wait loops.
- No continuous UI animations.
- No heavy AI models.
- No unnecessary network requests.
- No filesystem-wide scanning.
- No high-frequency refresh timers.

The daemon should remain mostly idle when it has no work to perform.

Do not sacrifice safety or correctness to meet an arbitrary memory target.

Measure the actual idle resource consumption and document it.

# 14. ERROR HANDLING

Create a consistent error-handling approach.

Handle:

- Daemon unavailable.
- IPC connection failure.
- Socket permission errors.
- Invalid configuration.
- Missing application directories.
- Database initialisation errors.
- Desktop application launch errors.
- Invalid CLI arguments.
- Unsupported operating-system environment.
- Unexpected daemon termination.

Errors should be understandable to ordinary users while retaining useful diagnostic detail in local logs.

Do not display raw stack traces in the normal UI.

# 15. TESTING REQUIREMENTS

Write tests as the foundation is developed.

## Rust unit tests

Test:

- Configuration parsing.
- Configuration validation.
- Configuration defaults.
- Error types.
- IPC message serialization.
- IPC message validation.
- Daemon state transitions.
- Duplicate daemon-start protection.
- Graceful shutdown logic.
- Path resolution.
- Logging initialisation.

## Integration tests

Test:

- CLI starting the daemon.
- CLI querying daemon status.
- CLI stopping the daemon.
- Desktop-to-daemon communication.
- Multiple clients connecting.
- Daemon restart after unexpected termination.
- Stale IPC socket handling.
- Invalid IPC requests.
- Missing configuration.
- Corrupted configuration.

## Safety tests

Verify:

- No system files are modified.
- No system services are modified.
- No unrelated process is terminated.
- No package manager is invoked.
- No privileged process is launched.
- No network listener is exposed.
- No monitoring is falsely reported as active.
- No placeholder telemetry is presented as real.

Use mocks and temporary directories wherever possible.

Never run destructive tests on the development machine.

# 16. DEVELOPMENT ENVIRONMENT

Target Linux first.

Use the currently available Linux development environment to validate the application.

Do not assume that a dedicated GPU is available.

Use stable, maintained dependencies.

Pin or lock dependency versions appropriately.

Provide reproducible development commands.

Create a README containing:

- Project description.
- Prerequisites.
- Development setup.
- Build commands.
- Run commands.
- Test commands.
- CLI usage.
- Architecture overview.
- Current implementation status.

Create a `.gitignore` suitable for Rust, Node, Tauri and local development.

# 17. PHASE 1 DELIVERABLES

At the end of Phase 1, the repository must contain:

1. A working Cargo workspace.
2. A working Tauri desktop application.
3. A React and TypeScript frontend.
4. A Rust background daemon.
5. A Rust CLI.
6. A functioning Unix domain socket IPC layer.
7. Configuration management.
8. Structured logging.
9. Error handling.
10. A polished initial Sentinel dashboard.
11. A manually controllable daemon lifecycle.
12. Unit tests.
13. Integration tests.
14. Safety tests.
15. Architecture documentation.
16. Security and safety documentation.
17. Development instructions.

The application must compile and run on the target Linux environment.

The daemon must be independently launchable.

The CLI must communicate with the daemon.

The desktop application must communicate with the daemon.

The UI must show the actual daemon state.

No future monitoring feature should be falsely represented as implemented.

# 18. IMPLEMENTATION ORDER

Follow this exact sequence.

### Step 1 — Technical validation

Before writing substantial code:

- Inspect the Linux environment.
- Verify Rust and Cargo availability.
- Verify Node.js and npm availability.
- Verify Tauri 2 Linux prerequisites.
- Identify the desktop environment and relevant packaging constraints.
- Confirm Unix domain socket support.
- Verify the intended dependency choices.

Document any compatibility issues.

Do not install packages or modify system configuration without the user's explicit permission.

If a dependency is missing, explain the installation command and wait for the user to install it rather than silently using elevated privileges.

### Step 2 — Repository foundation

Create the directory structure and Cargo workspace.

Establish shared types, error handling and project conventions.

### Step 3 — Daemon

Implement the daemon lifecycle, IPC server and status reporting.

Test multiple clients and shutdown behaviour.

### Step 4 — CLI

Implement the initial CLI commands.

Verify that each command interacts with the real daemon.

### Step 5 — Desktop application

Create the Tauri application and React frontend.

Implement the approved dark Sentinel design.

### Step 6 — Integration

Connect the UI to the daemon.

Display actual daemon state.

Ensure the monitoring control does not pretend to start a monitoring engine that does not exist yet.

### Step 7 — Testing and review

Run all tests.

Run production builds.

Inspect resource consumption.

Review security boundaries.

Verify that no prohibited system modification functionality has been introduced.

### Step 8 — Documentation

Update all documentation to reflect the actual implementation.

Do not document planned functionality as completed functionality.

# 19. DEFINITION OF DONE

Phase 1 is complete only when:

- The project builds successfully.
- The desktop application launches.
- The daemon runs independently.
- The CLI starts and stops the daemon.
- The desktop application communicates with the daemon.
- The UI displays real application state.
- The approved visual design has been implemented.
- Configuration is persistent and validated.
- Logs are generated locally.
- The application works without a dedicated GPU.
- The daemon does not require root.
- The daemon remains lightweight while idle.
- Tests pass.
- Safety boundaries have been verified.
- Documentation is complete.

A visually complete dashboard without a functioning backend is not a completed Phase 1.

Likewise, a functioning daemon without the desktop interface is not a completed Phase 1.

Both must work together.

# 20. ENGINEERING CONDUCT

Follow these rules throughout development:

1. Work phase by phase and step by step.
2. Do not generate the entire application in one massive code-generation operation.
3. Research technical APIs before implementing them.
4. Use official documentation wherever possible.
5. Never fabricate Linux capabilities.
6. Never invent hardware measurements.
7. Never present demonstration data as live data.
8. Never introduce automatic system modifications.
9. Keep modules small and independently testable.
10. Prefer maintainable engineering over unnecessary abstraction.
11. Explain important architectural decisions.
12. Run tests after meaningful implementation changes.
13. Fix regressions before moving forward.
14. Maintain documentation alongside implementation.
15. Report incomplete work honestly.

Do not proceed to Phase 2 automatically.

When Phase 1 is complete, stop and provide a detailed engineering report containing:

- What was implemented.
- Repository structure.
- Important architectural decisions.
- Commands to build and run Sentinel.
- Test results.
- Resource consumption measurements.
- Security and safety verification.
- Known limitations.
- Any outstanding issues.

Wait for the user's instruction before beginning Phase 2.

---

# INITIAL EXECUTION INSTRUCTION

Start with Step 1: Technical Validation.

Do not begin implementing the full application immediately.

First inspect the Linux environment, verify dependencies, validate the proposed architecture and identify any technical incompatibilities.

Present a concise technical validation report.

Then establish the repository foundation and proceed through the Phase 1 implementation steps in order.

The approved Sentinel visual reference is the dark dashboard image supplied in the project conversation. Use it as the visual direction.

**Remember: Sentinel observes, analyses and recommends. It never fixes or changes the user's system.**

==========================================================================================================================================================
==========================================================================================================================================================
==========================================================================================================================================================

# SENTINEL — PHASE 2
## Hardware Discovery, System Inventory & Utility Intelligence

**Project:** Sentinel
**Platform:** Linux desktop
**Phase:** 2
**Prerequisite:** Phase 1 completed and verified
**Primary objective:** Build Sentinel's hardware discovery and system inventory engine.

---

# 1. YOUR ROLE

Continue development of Sentinel using the existing Phase 1 repository.

You are responsible for implementing a reliable Linux hardware discovery and system inventory subsystem.

This phase gives Sentinel the ability to understand the computer on which it is running.

It must identify the laptop manufacturer, product model, motherboard information, processor, memory, storage, graphics, network interfaces, USB devices, battery and other discoverable hardware.

It must also discover the system utilities, desktop applications and relevant services already installed on the user's Linux installation.

Do not rebuild the Phase 1 application.

Do not replace the existing architecture without a documented technical reason.

Read the Phase 1 implementation, its architecture documentation and its final engineering report before beginning.

Extend the existing codebase.

# 2. CORE OBJECTIVE

When a user opens Sentinel, the application should be able to build a structured inventory of the computer.

For example, Sentinel should eventually be able to report:

```text
SYSTEM IDENTITY

Manufacturer: HP
Product: HP 15s-eq2xxx
Motherboard: Identified board model
BIOS: Detected firmware version
Architecture: x86_64

PROCESSOR

Manufacturer: AMD
Model: Ryzen 7 5700U
Physical cores: 8
Logical processors: 16
Architecture: Zen 2

MEMORY

Installed RAM: 16 GB
Memory type: DDR4
Memory speed: If available
Memory modules: If discoverable

GRAPHICS

GPU: AMD Radeon Graphics
Type: Integrated
Driver: Detected driver

STORAGE

Drive: Detected drive model
Interface: NVMe / SATA / other
Capacity: Actual reported capacity

BATTERY

Battery: Detected
Capacity: Actual reported value
Status: Charging / discharging / unknown
```

The values above are examples only.

Never hardcode these values or assume the user owns a particular laptop.

All displayed hardware information must originate from the actual machine or a clearly identified external documentation source.

# 3. ABSOLUTE SAFETY REQUIREMENTS

The Phase 1 safety policy remains authoritative.

Sentinel is strictly an observer, analyser and recommender.

It must NEVER modify the user's operating system.

This phase must be read-only.

Prohibited operations include:

- Installing or removing software.
- Updating packages.
- Modifying configuration files outside Sentinel's own application directory.
- Starting or stopping unrelated services.
- Changing permissions of system files.
- Changing CPU or GPU settings.
- Changing power profiles.
- Modifying network settings.
- Mounting or unmounting drives.
- Changing partitions.
- Killing processes.
- Modifying firmware.
- Running repair commands.
- Automatically invoking maintenance utilities.

Do not use sudo for hardware discovery.

Do not implement a privileged helper.

If an information source requires elevated privileges, report that the information is unavailable to the current user.

Do not attempt to bypass the restriction.

Sentinel must never execute arbitrary commands obtained from hardware metadata, device names, package names or discovered application descriptions.

# 4. HARDWARE DISCOVERY ARCHITECTURE

Create a dedicated hardware discovery subsystem.

Suggested structure:

```text
crates/
├── hardware-discovery/
│   ├── src/
│   │   ├── lib.rs
│   │   ├── discovery_manager.rs
│   │   ├── system_identity.rs
│   │   ├── motherboard.rs
│   │   ├── cpu.rs
│   │   ├── memory.rs
│   │   ├── gpu.rs
│   │   ├── storage.rs
│   │   ├── battery.rs
│   │   ├── network.rs
│   │   ├── usb.rs
│   │   ├── pci.rs
│   │   ├── sensors.rs
│   │   ├── firmware.rs
│   │   ├── topology.rs
│   │   └── parsers/
│   │
│   └── Cargo.toml
│
├── system-inventory/
│   ├── src/
│   │   ├── lib.rs
│   │   ├── inventory.rs
│   │   ├── device_registry.rs
│   │   ├── hardware_identity.rs
│   │   └── inventory_cache.rs
│   │
│   └── Cargo.toml
```

Adapt the structure to the existing Phase 1 repository.

Keep discovery modules independent.

A failure in one module must not prevent the remaining modules from working.

For example, if battery information is unavailable, CPU, memory and storage discovery must still succeed.

# 5. LINUX INFORMATION SOURCES

Prefer native Linux interfaces.

Use the following sources where applicable.

| Information | Primary source |
|---|---|
| System identity | `/sys/class/dmi/id/` |
| CPU topology | `/sys/devices/system/cpu/` |
| CPU details | `/proc/cpuinfo` |
| Memory | `/proc/meminfo` |
| PCI devices | `/sys/bus/pci/devices/` |
| USB devices | `/sys/bus/usb/devices/` |
| Storage | `/sys/block/` |
| Battery | `/sys/class/power_supply/` |
| Sensors | `/sys/class/hwmon/` |
| Thermal zones | `/sys/class/thermal/` |
| Network | `/sys/class/net/` |
| GPU | PCI sysfs and DRM sysfs |
| Firmware | DMI and firmware-related sysfs |
| Installed applications | Desktop entries and package databases |
| Services | systemd read-only interfaces |

Do not assume that every Linux distribution exposes all these interfaces.

Implement capability detection.

Handle missing files, unsupported fields, permission restrictions, malformed values and different hardware configurations.

Use safe filesystem reads and typed parsing.

Do not assume that a particular sysfs file always exists.

# 6. SYSTEM IDENTITY DISCOVERY

This is the first discovery module to implement.

Sentinel must identify:

- Manufacturer.
- Product name.
- Product version.
- Product family, if available.
- Board manufacturer.
- Board name.
- Board version.
- BIOS vendor.
- BIOS version.
- BIOS release date.
- System architecture.
- Kernel version.
- Linux distribution and version.

Read DMI/SMBIOS information where available.

Use `/etc/os-release` for distribution identification.

Use appropriate system interfaces for kernel and architecture information.

## 6.1 Hardware identity reliability

DMI values may be missing, generic, incorrectly populated or vendor-specific.

Do not assume a DMI field is always accurate.

For every field, track:

- Value.
- Source.
- Whether it was directly detected.
- Confidence.
- Last discovery time.
- Availability.

Do not expose serial numbers, UUIDs or other sensitive machine identifiers unnecessarily.

Do not include them in logs or future online searches.

## 6.2 Hardware fingerprint

Create a local hardware fingerprint for identifying the machine across inventory refreshes.

Use non-sensitive attributes such as:

- Manufacturer.
- Product model.
- Board identity.
- Board revision.
- CPU model.
- Hardware device identifiers.

Do not use serial numbers or system UUIDs in public search queries.

The fingerprint should be suitable for matching local hardware profiles.

Document its limitations.

# 7. CPU DISCOVERY

Implement processor identification and topology discovery.

Collect:

- CPU manufacturer.
- Exact processor model.
- CPU family.
- Model number.
- Stepping, if available.
- Architecture.
- Physical core count.
- Logical processor count.
- Thread topology.
- Socket/package information, if available.
- Cache hierarchy.
- Supported instruction flags, where available.
- Maximum and minimum frequencies, if exposed.
- Current frequency, if reliably exposed.

## 7.1 Core topology

This information is essential for future CPU visualisation.

Do not simply count the number of lines in `/proc/cpuinfo`.

Use Linux CPU topology interfaces to identify:

- Logical CPUs.
- Physical cores.
- Core IDs.
- Package IDs.
- Thread siblings.
- Core sibling relationships.
- Available cache information.

Represent each physical core and its logical processors explicitly.

Example:

```text
CPU PACKAGE 0

Core 0
  Logical CPU 0
  Logical CPU 1

Core 1
  Logical CPU 2
  Logical CPU 3

Core 2
  Logical CPU 4
  Logical CPU 5

Core 3
  Logical CPU 6
  Logical CPU 7
```

This is an illustrative topology, not an assumption about the user's processor.

Some CPUs have heterogeneous core types or multiple packages.

The data model must support:

- Different physical core counts.
- Different logical thread counts.
- Hybrid CPU architectures.
- Multiple packages.
- Missing topology information.

## 7.2 CPU architecture documentation

Collect verified processor identifiers that may be used in future manufacturer-documentation lookup.

Do not attempt transistor-level or die-level CPU reconstruction in this phase.

Do not invent a physical CPU diagram.

# 8. MEMORY DISCOVERY

Collect:

- Total installed memory, where discoverable.
- Total usable memory.
- Memory type, if available.
- Memory speed, if available.
- Number of populated memory slots, if available.
- Individual memory module information, if available.
- Memory device identifiers, if safely available.
- NUMA topology, if supported.

Use `/proc/meminfo` for operating-system memory information.

Use DMI memory information where available for physical module information.

Clearly distinguish:

- Physical installed RAM.
- Usable system memory.
- Currently used memory.
- Available memory.
- Cached memory.
- Swap capacity.

Do not calculate physical installed RAM from the currently used memory.

Do not claim a particular RAM type or frequency if the system does not expose reliable information.

Process-level memory monitoring is not part of this phase.

# 9. MOTHERBOARD DISCOVERY

Collect:

- Board manufacturer.
- Board product name.
- Board revision.
- System manufacturer.
- System model.
- BIOS version.
- BIOS vendor.
- Available board identifiers.

The objective is to build the identity required for future schematic discovery.

Do not search for or download schematics during Phase 2.

Automatic schematic discovery will be implemented in a later phase.

Create a documented interface that future schematic-discovery modules can consume.

Example:

```rust
struct BoardIdentity {
    system_manufacturer: Option<String>,
    system_model: Option<String>,
    board_manufacturer: Option<String>,
    board_name: Option<String>,
    board_revision: Option<String>,
    bios_vendor: Option<String>,
    bios_version: Option<String>,
}
```

Extend this model as necessary.

Do not assume that the laptop model and motherboard model are the same thing.

Do not declare a board revision verified merely because the laptop model matches.

# 10. GPU DISCOVERY

Discover integrated and dedicated graphics devices.

Collect:

- GPU manufacturer.
- GPU model.
- PCI vendor ID.
- PCI device ID.
- PCI subsystem identifiers.
- Driver.
- DRM device information, where available.
- Device classification.
- Memory information, where available.

Support multiple GPUs.

Distinguish:

- Integrated GPU.
- Dedicated GPU.
- Hybrid graphics.
- Software-rendered graphics, where applicable.

Do not assume that shared system memory is dedicated VRAM.

Do not require a dedicated GPU.

The discovery engine must work on CPU-only machines.

Do not run GPU benchmarks or stress tests.

Do not change graphics driver configuration.

# 11. STORAGE DISCOVERY

Enumerate storage devices.

Collect:

- Device model.
- Manufacturer, where available.
- Device type.
- Interface.
- Capacity.
- Logical and physical sector sizes.
- Device path.
- Removable status.
- Read-only status, if exposed.
- Partition relationships.
- NVMe or SATA classification, where identifiable.

Support:

- NVMe SSDs.
- SATA SSDs.
- HDDs.
- USB storage.
- Other block devices exposed by Linux.

Do not confuse a partition with a physical storage device.

Build a parent-child relationship between drives and their partitions.

Do not mount or unmount devices.

Do not modify partition tables.

Do not run destructive storage tests.

SMART health monitoring will be expanded in a later phase.

If an already available read-only identification interface can provide additional model information, document its use and limitations.

Do not require installing additional packages.

# 12. BATTERY DISCOVERY

Discover battery and power-supply interfaces.

Use `/sys/class/power_supply/`.

Collect fields when available:

- Battery name.
- Battery type.
- Manufacturer.
- Model.
- Serial number only for internal matching when necessary; do not expose it by default.
- Status.
- Capacity.
- Energy now.
- Energy full.
- Energy design.
- Charge now.
- Charge full.
- Voltage.
- Current.
- Power.
- Cycle count.
- Technology.
- AC adapter state.

Some devices expose energy values while others expose charge values.

Handle both.

Do not assume every battery exposes every field.

Do not calculate battery health if the necessary capacity fields are missing or incompatible.

Do not invent battery temperature.

Battery temperature, remaining runtime and live health analysis will be implemented in a later telemetry phase.

# 13. NETWORK DISCOVERY

Discover:

- Network interfaces.
- Interface names.
- MAC addresses.
- Interface type.
- Link state.
- Driver.
- Device identifiers.
- Ethernet and Wi-Fi classification.
- Available wireless interface information.

Do not collect packet contents.

Do not capture network traffic.

Do not modify network settings.

Treat MAC addresses as sensitive local identifiers and do not expose them in public search queries or ordinary logs.

Network traffic monitoring will be implemented in a later phase.

# 14. USB DISCOVERY

Enumerate currently connected USB devices.

Collect:

- Vendor ID.
- Product ID.
- Manufacturer.
- Product name.
- USB version.
- Bus number.
- Device number.
- Device class.
- Connection speed, where available.
- Parent hub relationships.
- Sysfs topology path.

Build a structured USB topology.

Example:

```text
USB CONTROLLER

  Root Hub
    Port 1
      USB Storage Device

    Port 2
      Available / No device detected

    Port 3
      USB Hub
        Port 1
          USB Mouse
```

Do not assume that Linux USB port numbering directly maps to physical laptop sockets.

Physical connector mapping will require verified motherboard documentation or other reliable mapping information.

Do not claim that a particular USB-A or USB-C socket is occupied unless the physical mapping has been established.

Live USB connection event monitoring will be expanded in a later phase.

# 15. PCI AND HARDWARE TOPOLOGY

Discover PCI devices.

Collect:

- Vendor ID.
- Device ID.
- Subsystem IDs.
- Class.
- Driver.
- PCI address.
- Parent/child relationships, where available.

Create a structured topology model.

The topology will later help Sentinel associate:

- GPU devices.
- Network controllers.
- USB controllers.
- Storage controllers.
- Audio devices.
- Other motherboard-connected hardware.

Do not invent electrical connections from PCI topology.

PCI topology is not the same as a complete motherboard schematic.

# 16. SENSOR CAPABILITY DISCOVERY

Enumerate available hardware sensor interfaces.

Discover:

- Temperature sensors.
- Fan sensors.
- Voltage sensors.
- Power sensors.
- Thermal zones.
- Sensor labels.
- Sensor paths.

This phase only discovers sensor availability and metadata.

Do not implement continuous telemetry.

Do not assume that every temperature sensor belongs to a known motherboard component.

Store the sensor source and label so future telemetry and visualisation modules can use them.

Never fabricate sensor values.

# 17. SYSTEM INVENTORY MODEL

Create a central inventory model that combines the information collected by all discovery modules.

The inventory must support partial results.

Example conceptual structure:

```rust
struct SystemInventory {
    schema_version: u32,
    discovered_at: i64,

    system: SystemIdentity,
    motherboard: Option<BoardIdentity>,

    processors: Vec<ProcessorInfo>,
    memory: Option<MemoryInfo>,
    graphics: Vec<GpuInfo>,
    storage: Vec<StorageDevice>,
    batteries: Vec<BatteryInfo>,
    network: Vec<NetworkInterface>,
    usb_devices: Vec<UsbDevice>,
    pci_devices: Vec<PciDevice>,
    sensors: Vec<SensorInfo>,

    discovery_status: Vec<DiscoveryStatus>,
}
```

Use proper typed models.

Do not store every field as an arbitrary string.

Use appropriate numeric types and units.

Distinguish:

- Unknown.
- Unsupported.
- Unavailable.
- Permission denied.
- Successfully detected.

Do not convert unavailable information into zero.

Every discovered field should be traceable to its source.

# 18. DATA PROVENANCE AND CONFIDENCE

Sentinel must be transparent about where its information comes from.

Create a common representation for discovered values.

It should support:

- Value.
- Source.
- Discovery timestamp.
- Confidence.
- Availability.
- Optional notes.

Suggested confidence categories:

```text
DIRECT
    Read directly from a Linux hardware interface.

FIRMWARE_REPORTED
    Reported by SMBIOS or firmware.

KERNEL_REPORTED
    Reported by the Linux kernel or driver.

INFERRED
    Derived from multiple detected values.

UNKNOWN
    Source or reliability cannot be established.
```

Do not give an inferred value the same authority as a directly detected value.

Do not display fake precision.

# 19. INSTALLED SYSTEM UTILITY DISCOVERY

This is a major Sentinel feature.

Sentinel should help users understand what system-related applications and tools are already installed on their Linux machine.

It must create a local catalogue of discovered utilities.

Categories should include:

- System monitoring.
- Hardware information.
- Disk management.
- Storage health.
- Battery and power.
- Sensors and temperatures.
- Network management.
- Network diagnostics.
- Process management.
- System logs.
- Service management.
- Package management.
- Graphics and drivers.
- Backup and recovery.
- System configuration.

Examples of tools Sentinel may discover:

```text
btop
htop
GNOME System Monitor
KDE System Monitor
Hardinfo
CPU-X
inxi
lshw
lm-sensors
Psensor
GNOME Disks
GParted
smartctl
nvme-cli
upower
tlp
NetworkManager
Wireshark
journalctl
Cockpit
```

These are examples only.

Do not assume any of them are installed.

## 19.1 Discovery sources

Use multiple discovery sources.

### Desktop applications

Inspect standard XDG application directories and `.desktop` files.

Identify:

- Application name.
- Desktop entry identifier.
- Category.
- Description.
- Executable or launch target.
- Icon.
- Installation scope.
- Whether the application is available to the current user.

### Package managers

Create a package-discovery abstraction.

Support appropriate adapters for the Linux distributions we intend to support.

Potential package systems include:

- Debian/APT/dpkg.
- Fedora/RPM/DNF.
- Arch/pacman.
- Flatpak.
- Snap.

Only inspect installed-package databases.

Do not invoke installation, removal, upgrade or repository-modification operations.

Do not require every package manager to be installed.

### Executables

Inspect available executable names through the user's PATH and known standard binary directories.

Avoid scanning the entire filesystem.

Do not execute discovered binaries simply to determine whether they exist.

### System services

Discover relevant system and user services.

Collect:

- Service name.
- Description.
- Unit file location, where available.
- Loaded state.
- Active state.
- Enabled state, if reliably available.
- User or system scope.

Use read-only systemd queries.

Do not start, stop, enable or disable services.

## 19.2 Utility catalogue model

Create a structured representation:

```rust
struct SystemUtility {
    id: String,
    name: String,
    description: Option<String>,
    category: UtilityCategory,

    installed: bool,
    version: Option<String>,

    executable: Option<String>,
    desktop_entry: Option<String>,

    source: DiscoverySource,
    launch_available: bool,
}
```

Extend the model where required.

Avoid storing arbitrary executable commands from untrusted desktop entries and package metadata as executable instructions.

Launching an application is a separate, user-initiated action.

Do not launch applications automatically.

## 19.3 Sentinel integration

Create an Installed Utilities section in the desktop application.

Users should be able to:

- Browse detected utilities.
- Filter by category.
- Search by name.
- See whether a utility is installed.
- Read a short description.
- See where the utility was discovered.
- Open an installed utility through an explicit user action.

Do not install missing utilities.

Do not automatically recommend package installation commands in this phase.

# 20. INVENTORY REFRESH AND CACHING

Hardware inventory should not be continuously rescanned.

Implement an explicit discovery workflow.

The user should be able to:

- Run an initial hardware scan.
- Refresh the inventory manually.
- View the last scan time.
- See which modules completed successfully.
- See which modules failed.
- View partial results.

The daemon should coordinate discovery requests.

Do not run multiple full scans simultaneously.

Implement safe cancellation if a scan is interrupted.

Use a versioned local cache.

Persist hardware inventory in Sentinel's own application data directory.

On application startup, display cached inventory when available and indicate that it is cached.

Do not claim cached information is live.

A future phase will introduce event-driven device updates and periodic telemetry.

# 21. IPC EXPANSION

Extend the Phase 1 IPC protocol.

Add typed operations such as:

```text
RunHardwareDiscovery
GetHardwareInventory
RefreshHardwareInventory
GetDiscoveryStatus

DiscoverSystemUtilities
GetSystemUtilities
RefreshSystemUtilities

GetHardwareCapabilities
```

Every operation must:

- Validate the request.
- Return structured responses.
- Include request identifiers.
- Include clear error information.
- Handle cancellation or timeout appropriately.

Do not expose arbitrary filesystem paths or command execution through IPC.

The frontend must not independently access `/proc`, `/sys` or package databases.

All discovery must happen through the backend.

# 22. DESKTOP UI IMPLEMENTATION

Follow the already approved Sentinel visual direction.

Do not redesign the application.

Keep the dark black background, white typography, subtle borders and colourful hardware accents.

Use the generated Sentinel dashboard image as the primary visual reference.

## 22.1 Overview page

Replace demonstration hardware values with real inventory data.

The overview should show:

- Manufacturer.
- Laptop model.
- Motherboard identity.
- CPU model.
- Physical core count.
- Logical processor count.
- Installed RAM.
- GPU information.
- Storage information.
- Battery availability.
- Detected hardware count.
- Last inventory scan time.

Clearly distinguish detected and unavailable information.

Do not show live CPU usage or live memory consumption in this phase.

Those belong to Phase 3.

## 22.2 Hardware page

Create a hardware inventory page.

Organise hardware into categories:

- Processor.
- Memory.
- Motherboard.
- Graphics.
- Storage.
- Battery.
- Network.
- USB.
- PCI devices.
- Sensors.

Each category should open a detailed information view.

Show:

- Hardware name.
- Manufacturer.
- Model.
- Identifiers where appropriate.
- Relevant specifications.
- Information source.
- Discovery status.

## 22.3 CPU page

Display the actual CPU model and detected topology.

Build an initial CPU topology visualisation.

Show the correct number of physical cores and logical processors.

This is a topology diagram, not a fabricated physical CPU die image.

Use different colours for core blocks and their logical processor relationships.

Allow the user to select an individual core and view its static hardware identity.

Do not display live core utilisation yet.

## 22.4 Memory page

Display actual installed and usable memory information.

Where memory modules can be discovered, show their individual information.

A graphical memory module layout may be used when module and slot information is sufficiently reliable.

Do not invent memory slot positions or module configurations.

Do not display process-level memory usage yet.

## 22.5 Motherboard page

Display the identified board information.

Show:

- System model.
- Board manufacturer.
- Board name.
- Board revision.
- BIOS information.
- Identification confidence.

Do not display a fabricated physical motherboard diagram.

Until schematic analysis is implemented, use an explicit state:

"Motherboard identity detected — schematic analysis not yet available."

## 22.6 Installed Utilities page

Create a searchable, categorised utility catalogue.

Make application launch an explicit user action.

Show unavailable utilities without pretending they are installed.

## 22.7 Discovery status

Provide a clear view of discovery progress.

Display:

- Current discovery module.
- Modules completed.
- Modules unavailable.
- Errors.
- Last successful scan.

Do not block the entire UI while scanning.

# 23. VISUALISATION DATA CONTRACT

All diagrams must be generated from structured hardware information.

Do not hardcode diagrams for a particular laptop.

For the CPU topology visualisation, generate the diagram dynamically from detected cores and threads.

For memory, generate the module representation only when reliable slot/module data is available.

For motherboard identification, provide the structured board identity to future schematic modules.

Use SVG for the first hardware diagrams.

Keep rendering logic separate from hardware discovery logic.

# 24. TESTING

Implement extensive tests.

## Unit tests

Test:

- DMI parsing.
- CPU topology parsing.
- CPU model parsing.
- Memory parsing.
- GPU identification.
- PCI identification.
- USB topology parsing.
- Storage capacity parsing.
- Battery field parsing.
- Sensor discovery.
- Network interface discovery.
- Distribution identification.
- Desktop entry parsing.
- Utility categorisation.
- Package adapter parsing.
- Data provenance.
- Confidence assignment.
- Missing-field handling.
- Invalid-field handling.

## Fixture-based tests

Do not rely exclusively on the developer's current laptop.

Create fixtures representing:

- AMD laptop.
- Intel laptop.
- Laptop with integrated GPU.
- Laptop with integrated and dedicated GPUs.
- Four-core CPU.
- Eight-core CPU.
- Sixteen-core CPU.
- Hybrid CPU.
- Multiple memory modules.
- Missing battery information.
- Multiple batteries.
- NVMe storage.
- SATA storage.
- USB storage.
- Missing DMI fields.
- Permission-restricted sensor data.
- Different Linux distributions.

These fixtures must be synthetic or sanitised.

Do not publish private hardware identifiers.

## Integration tests

Test:

- Full hardware discovery.
- Partial discovery.
- Manual refresh.
- Cache loading.
- Cache refresh.
- Utility discovery.
- Desktop application rendering.
- IPC communication.
- Scan cancellation.
- Discovery failure recovery.

## Safety tests

Verify:

- No system configuration files are changed.
- No packages are installed or removed.
- No unrelated services are modified.
- No processes are terminated.
- No privileged operations are invoked.
- No filesystem-wide scan occurs.
- No network request occurs during hardware discovery.
- No private hardware identifiers are included in logs.
- No fabricated hardware data is displayed.

# 25. PERFORMANCE REQUIREMENTS

Hardware discovery should be efficient.

Requirements:

- Do not repeatedly read every sysfs interface unnecessarily.
- Avoid duplicate scans.
- Cache stable hardware information.
- Do not poll continuously.
- Do not execute expensive external commands unnecessarily.
- Avoid loading entire package databases into memory when unnecessary.
- Do not use a GPU.
- Do not run benchmarks.
- Do not stress-test hardware.

Measure discovery duration and memory consumption.

The application should remain responsive while scanning.

# 26. DOCUMENTATION

Update:

- README.md
- ARCHITECTURE.md
- SECURITY.md
- SAFETY_POLICY.md
- DEVELOPMENT.md

Create:

- HARDWARE_DISCOVERY.md
- SYSTEM_INVENTORY.md
- UTILITY_DISCOVERY.md

Document:

- Linux interfaces used.
- Supported discovery fields.
- Hardware limitations.
- Distribution-specific differences.
- Permission limitations.
- Data provenance.
- Known unsupported hardware.
- How to add new discovery modules.

Do not claim complete Linux hardware support.

Document tested environments honestly.

# 27. DEFINITION OF DONE

Phase 2 is complete only when:

1. Sentinel correctly identifies the system manufacturer and model where firmware provides reliable information.
2. Motherboard and BIOS identity are collected where available.
3. CPU model and actual topology are discovered.
4. Installed and usable memory information is collected.
5. GPU devices are discovered.
6. Storage devices and partitions are enumerated.
7. Battery information is collected where available.
8. Network interfaces are enumerated.
9. USB devices and their Linux topology are discovered.
10. PCI devices are enumerated.
11. Sensor capabilities are discovered.
12. Installed desktop utilities are catalogued.
13. Relevant system services are discovered.
14. Inventory is displayed in the Sentinel desktop interface.
15. CPU topology is visualised dynamically.
16. Discovery sources and confidence are visible.
17. Missing and unsupported information is handled honestly.
18. Cached inventory works correctly.
19. Manual inventory refresh works.
20. All tests pass.
21. Resource consumption is measured.
22. No prohibited system modifications occur.

# 28. STRICT PHASE BOUNDARY

Do not implement Phase 3 or later features.

The following are explicitly out of scope:

- Continuous CPU monitoring.
- Continuous per-core utilisation monitoring.
- Process-level CPU usage.
- Process-level memory breakdown.
- Continuous temperature monitoring.
- Live battery runtime estimation.
- Live power consumption graphs.
- Desktop warning notifications.
- Continuous USB event monitoring.
- Automatic motherboard schematic searches.
- PDF schematic processing.
- OCR.
- Physical motherboard rendering.
- AI hardware analysis.
- Automatic maintenance actions.

Create the data models and extension points needed for these features, but do not implement them yet.

# 29. FINAL ENGINEERING REPORT

When Phase 2 is complete, stop.

Do not automatically proceed to Phase 3.

Provide a detailed report covering:

1. Implemented modules.
2. Repository changes.
3. Hardware discovery sources.
4. Hardware successfully detected on the actual development machine.
5. Hardware information that was unavailable.
6. Utility discovery results.
7. Linux distributions and hardware configurations tested.
8. Unit and integration test results.
9. Safety verification.
10. Resource consumption.
11. Known limitations.
12. Outstanding issues.
13. Commands to run Sentinel and refresh inventory.

Distinguish actual test results from synthetic fixture results.

Do not claim a hardware category is supported merely because a parser or interface exists.

---

# INITIAL EXECUTION INSTRUCTION

Read the Phase 1 source code and documentation completely.

Verify that Phase 1 is functioning before making changes.

Inspect the current Linux environment and identify which hardware interfaces are available.

Then implement Phase 2 in the exact order specified.

Do not replace the existing architecture.

Do not implement continuous telemetry or schematic discovery.

Do not install packages or modify system configuration without explicit user permission.

Do not use sudo.

Test every discovery module independently and then test the complete inventory pipeline.

Use actual hardware information from the development machine.

Never substitute fabricated sample values for missing information.

**Sentinel must observe the system accurately without changing it.**

Stop after Phase 2 and wait for the user's instruction before proceeding.

==========================================================================================================================================================
==========================================================================================================================================================
==========================================================================================================================================================

# SENTINEL — PHASE 3
## Live System Monitoring, Telemetry Engine & Intelligent Alerts

**Project:** Sentinel
**Platform:** Linux desktop
**Phase:** 3
**Prerequisites:** Phase 1 and Phase 2 completed and verified
**Primary objective:** Implement a lightweight, reliable, continuously operating Linux system monitoring engine.

---

# 1. YOUR ROLE

Continue developing Sentinel using the existing repository.

You are the lead Linux systems engineer responsible for implementing the real-time telemetry and monitoring subsystem.

Phase 1 established the application foundation.

Phase 2 implemented hardware discovery and system inventory.

Phase 3 must build on both.

The objective is to make Sentinel capable of monitoring the actual computer continuously and presenting its changing state through the desktop application.

Sentinel must monitor:

- CPU utilisation.
- Individual CPU core utilisation.
- Memory consumption.
- Process-level resource consumption.
- Swap usage.
- CPU temperatures.
- GPU utilisation and temperature, where supported.
- Battery status and health indicators.
- Battery temperature, where exposed.
- Estimated remaining battery runtime.
- Storage utilisation and I/O activity.
- Network throughput.
- System load.
- Fan speeds, where available.
- Thermal conditions.
- Power-related information.

It must also detect meaningful changes, maintain a local history of measurements and issue desktop notifications when important thresholds are reached.

Do not rebuild the application.

Do not replace the Phase 1 or Phase 2 architecture without a documented technical reason.

Read the existing code, architecture documentation, safety policy and previous engineering reports before making changes.

# 2. ABSOLUTE SAFETY POLICY

The safety policy established in Phase 1 remains permanent.

Sentinel is an observer, analyser and recommender.

It must never perform system repairs or modify the user's operating system.

This applies to every monitoring module, background task, notification, diagnostic and future AI component.

## 2.1 Sentinel must never

- Kill or terminate processes.
- Suspend processes.
- Change process priorities.
- Change CPU frequencies.
- Change CPU voltage.
- Change GPU frequencies.
- Change GPU voltage.
- Change fan speeds.
- Change thermal limits.
- Change power profiles.
- Change battery charging limits.
- Change network configuration.
- Modify firewall rules.
- Modify system services.
- Modify kernel settings.
- Install or uninstall packages.
- Execute repair commands.
- Modify storage devices.
- Change partitions.
- Apply suggested fixes.

Do not introduce a generic command execution facility.

Do not execute commands obtained from telemetry, hardware metadata or alert recommendations.

## 2.2 Recommendations are informational

Example:

**Alert:** CPU temperature is unusually high.

**Explanation:** The detected CPU temperature has remained above the configured warning threshold.

**Recommendation:** Check whether demanding applications are running and ensure the laptop's ventilation openings are unobstructed.

Sentinel must not terminate applications, alter fan speeds or change CPU settings.

Another example:

**Alert:** Memory pressure is high.

**Recommendation:** Review the applications consuming the most memory and consider closing applications you do not currently need.

Sentinel must not close those applications.

## 2.3 Read-only monitoring

Use read-only Linux interfaces.

Prefer:

- `/proc`
- `/sys`
- hwmon
- thermal zones
- DRM interfaces
- udev information
- read-only systemd interfaces
- existing read-only monitoring APIs

Do not use sudo.

Do not run the daemon as root.

If a metric is unavailable to the current user, report that it is unavailable.

Never bypass permissions.

# 3. MONITORING ARCHITECTURE

Create a dedicated telemetry subsystem.

Suggested architecture:

```text
                 SENTINEL DESKTOP
                        |
                  Monitoring UI
                        |
                    IPC Layer
                        |
                 SENTINEL DAEMON
                        |
                TELEMETRY ENGINE
                        |
          +-------------+-------------+
          |             |             |
         CPU          Memory        Battery
          |             |             |
          +-------------+-------------+
                        |
          +-------------+-------------+
          |             |             |
       Storage        Network       Sensors
          |             |             |
          +-------------+-------------+
                        |
                METRIC AGGREGATOR
                        |
               ALERT EVALUATION
                        |
                LOCAL HISTORY
                        |
              NOTIFICATION SERVICE
```

Implement independent collectors.

Each collector must be responsible for obtaining and normalising its own metrics.

The telemetry engine coordinates collectors and publishes structured measurements.

A failure in one collector must not prevent the others from operating.

For example, if the GPU driver does not expose temperature information, CPU, memory, battery and storage monitoring must continue normally.

# 4. TELEMETRY MODULE STRUCTURE

Extend the existing project.

Suggested structure:

```text
crates/
├── sentinel-telemetry/
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── scheduler.rs
│       ├── collector_registry.rs
│       ├── metric.rs
│       ├── aggregator.rs
│       ├── sampling.rs
│       └── collectors/
│           ├── cpu.rs
│           ├── memory.rs
│           ├── process.rs
│           ├── battery.rs
│           ├── thermal.rs
│           ├── gpu.rs
│           ├── storage.rs
│           ├── network.rs
│           ├── fan.rs
│           └── system_load.rs
│
├── sentinel-alerts/
│   └── src/
│       ├── lib.rs
│       ├── engine.rs
│       ├── rules.rs
│       ├── thresholds.rs
│       ├── evaluator.rs
│       └── notifications.rs
│
└── sentinel-history/
    └── src/
        ├── lib.rs
        ├── repository.rs
        ├── retention.rs
        └── queries.rs
```

Adapt the structure to the existing repository.

Keep telemetry collection, alert evaluation, persistence and notification delivery separate.

# 5. METRIC DATA MODEL

Establish a common typed representation for telemetry measurements.

Every metric should support:

- Metric identifier.
- Value.
- Unit.
- Hardware or process identifier, when applicable.
- Measurement timestamp.
- Data source.
- Availability state.
- Collection interval.
- Confidence or quality indicator, where relevant.

Example:

```rust
struct Metric {
    id: MetricId,
    value: MetricValue,
    unit: MetricUnit,
    source: MetricSource,
    measured_at: i64,
    quality: MetricQuality,
}

enum MetricQuality {
    Direct,
    Calculated,
    Estimated,
    Unavailable,
}
```

Use appropriate types for each measurement.

Do not store all metrics as floating-point numbers without considering precision and units.

Use monotonic time for interval calculations and wall-clock timestamps for history.

Distinguish:

- Zero.
- Unknown.
- Unsupported.
- Unavailable.
- Permission denied.
- Collection error.

Never convert unavailable values to zero.

Never fabricate readings.

# 6. MONITORING CONTROL AND AUTONOMY

The user must have complete control over Sentinel monitoring.

Monitoring must be explicitly controllable through the desktop application and CLI.

Support these states:

```text
OFF
STARTING
ACTIVE
PAUSED
STOPPING
ERROR
```

## 6.1 Desktop controls

The main dashboard must have a prominent monitoring ON/OFF control.

When OFF:

- Stop active telemetry collection.
- Stop periodic sampling.
- Stop monitoring-related alert evaluation.
- Stop monitoring-related notifications.
- Preserve previously collected history.
- Keep the desktop application usable.
- Keep the daemon available for status and configuration operations.

When ON:

- Start the telemetry engine.
- Initialise supported collectors.
- Begin sampling according to the selected power mode.
- Display actual collection status.

When PAUSED:

- Temporarily suspend sampling.
- Preserve existing data.
- Do not claim that monitoring is active.

The monitoring state must be reflected accurately in the UI.

Do not automatically enable persistent monitoring after installation.

If the daemon starts after a reboot, monitoring must follow the user's saved preference and the documented startup policy. Do not silently start monitoring if the user has disabled it.

## 6.2 CLI commands

Extend the CLI:

```bash
sentinel monitor start
sentinel monitor stop
sentinel monitor pause
sentinel monitor resume
sentinel monitor status
```

Also support:

```bash
sentinel metrics
sentinel metrics cpu
sentinel metrics memory
sentinel metrics battery
sentinel metrics storage
sentinel metrics network
```

Commands must return actual telemetry state.

If monitoring is OFF, do not pretend to return live measurements.

A one-time read-only diagnostic query may be implemented separately if clearly labelled as a snapshot rather than continuous monitoring.

# 7. SAMPLING AND POWER EFFICIENCY

This is a critical part of Sentinel.

Sentinel must not consume excessive CPU, memory or battery while monitoring the system.

Do not implement an aggressive polling loop.

Do not use busy-waiting.

Do not spawn a thread for every metric.

Use a central scheduler and asynchronous or otherwise efficient collection architecture.

## 7.1 Power modes

Implement three sampling profiles.

### Eco mode

Designed for battery-powered laptops and low-resource systems.

Suggested initial intervals:

| Metric | Interval |
|---|---:|
| CPU | 5 seconds |
| Memory | 5 seconds |
| Processes | 10 seconds |
| Battery | 30 seconds |
| Temperature | 10 seconds |
| GPU | 10 seconds |
| Storage I/O | 5 seconds |
| Network | 5 seconds |
| Fan | 10 seconds |

### Balanced mode

Default mode.

| Metric | Interval |
|---|---:|
| CPU | 2 seconds |
| Memory | 2 seconds |
| Processes | 5 seconds |
| Battery | 15 seconds |
| Temperature | 5 seconds |
| GPU | 5 seconds |
| Storage I/O | 2 seconds |
| Network | 2 seconds |
| Fan | 5 seconds |

### Performance mode

Designed for users who want more responsive graphs.

Suggested initial intervals:

| Metric | Interval |
|---|---:|
| CPU | 1 second |
| Memory | 1 second |
| Processes | 2 seconds |
| Battery | 10 seconds |
| Temperature | 2 seconds |
| GPU | 2 seconds |
| Storage I/O | 1 second |
| Network | 1 second |
| Fan | 2 seconds |

These are initial design values, not mandatory universal polling rates.

Validate actual hardware and Linux interface behaviour.

Make intervals configurable internally and document the resulting resource consumption.

Do not sample faster than a source can reliably provide new information.

## 7.2 Adaptive sampling

Where practical, use adaptive sampling.

Examples:

- If the system is idle, avoid unnecessary expensive collectors.
- If a sensor value is unavailable, do not repeatedly attempt to read it at high frequency.
- If the desktop UI is closed, keep the configured monitoring policy but avoid UI-specific work.
- If the laptop is on battery, favour Eco mode when the user has enabled automatic power-aware sampling.
- If a collector fails repeatedly, apply backoff rather than continuously retrying.

Do not silently change the user's selected power profile.

If automatic power-aware sampling is enabled, make it visible and configurable.

## 7.3 Performance measurement

Measure:

- Daemon CPU consumption while idle.
- Daemon CPU consumption while monitoring.
- Resident memory usage.
- Wake-up frequency.
- Collection duration.
- Storage writes.
- Notification overhead.

Provide benchmark results for Eco, Balanced and Performance modes.

Do not claim that Sentinel consumes negligible resources without measuring it.

# 8. CPU MONITORING

Implement real-time CPU monitoring.

Collect:

- Total CPU utilisation.
- User time.
- System time.
- Idle time.
- I/O wait.
- Nice time.
- IRQ time.
- Soft IRQ time.
- Steal time, where relevant.
- Per-core utilisation.
- CPU load averages.
- CPU frequency, where exposed.
- CPU temperature, through the thermal subsystem.

Use `/proc/stat` and appropriate sysfs interfaces.

## 8.1 Utilisation calculation

Calculate utilisation using differences between cumulative CPU counters.

Do not treat cumulative counters as instantaneous percentages.

Handle:

- Counter rollover or unexpected decreases.
- CPU hotplug.
- Newly discovered logical processors.
- Missing CPU data.
- Invalid sampling intervals.

The result must reflect the actual number of detected logical CPUs.

## 8.2 Per-core monitoring

Use the CPU topology discovered in Phase 2.

Display individual core utilisation.

Support:

- Four-core processors.
- Eight-core processors.
- Sixteen-core processors.
- Hybrid processors.
- Different thread counts.
- Multiple CPU packages.

Never hardcode a specific processor configuration.

## 8.3 CPU page

When the user opens CPU:

Display:

- Processor manufacturer and model.
- Physical core count.
- Logical processor count.
- Overall CPU utilisation.
- Per-core utilisation.
- CPU frequency, if available.
- Temperature sensors associated with the CPU, if identifiable.
- Load averages.
- Historical utilisation graph.

Keep the CPU topology diagram from Phase 2.

Make each core selectable.

Selecting a core should reveal its current utilisation, logical processor relationships and other available information.

Do not claim that a software topology diagram is a physical silicon die schematic.

A physical CPU die visualisation belongs to a later documentation and visualisation phase.

# 9. MEMORY AND PROCESS MONITORING

Implement real-time memory monitoring.

Use `/proc/meminfo` and appropriate process interfaces.

Collect:

- Total memory.
- Used memory.
- Available memory.
- Free memory.
- Buffers.
- Cached memory.
- Active memory.
- Inactive memory.
- Swap total.
- Swap used.
- Swap free.
- Memory pressure information, where available.

Use the appropriate Linux memory accounting definitions.

Do not incorrectly calculate memory usage by simply subtracting `MemFree` from `MemTotal` and labelling all cached memory as unavailable.

Use `MemAvailable` where supported.

## 9.1 Process monitoring

Enumerate processes through `/proc`.

For each process, collect accessible information:

- PID.
- Process name.
- Command identity, where safely available.
- CPU utilisation.
- Resident memory.
- Virtual memory.
- Memory percentage.
- Process state.
- User identity, where accessible.
- Thread count.
- Start time.

Do not read or expose process environment variables.

Do not collect passwords, tokens, command-line secrets or application file contents.

Command-line arguments can contain sensitive information. Avoid displaying full arguments by default.

If process information is inaccessible, show a permission limitation.

Do not use root to inspect protected processes.

## 9.2 Process memory ranking

Provide a ranked process list.

Users must be able to:

- Sort by memory usage.
- Sort by CPU usage.
- Search process names.
- Filter running and sleeping processes.
- Open detailed process information.
- View process memory and CPU history where available.

Do not add Kill, Terminate, Suspend or Change Priority buttons.

Sentinel is not a process controller.

## 9.3 Memory page

Display:

- Memory usage graph.
- Used, available and cached memory.
- Swap usage.
- Memory pressure.
- Top memory-consuming processes.
- Historical memory trends.

Provide clear distinctions between physical memory, virtual memory and swap.

# 10. BATTERY AND POWER MONITORING

Battery monitoring is a major feature of Sentinel.

Use Linux power-supply interfaces and other available read-only information sources.

Collect where available:

- Charging state.
- Discharging state.
- Full state.
- Battery percentage.
- Energy now.
- Energy full.
- Energy design.
- Charge now.
- Charge full.
- Voltage.
- Current.
- Power draw.
- Cycle count.
- Battery technology.
- Battery manufacturer.
- Battery model.
- Battery temperature, if explicitly exposed.
- AC adapter connection state.

## 10.1 Battery health

Where the necessary capacity information exists, calculate:

```text
Battery Health (%) =
(Full Charge Capacity / Design Capacity) × 100
```

Use compatible units.

If capacity information is incomplete or inconsistent, do not display a calculated health percentage.

Explain that the value is an estimate based on reported capacity, not a complete laboratory assessment of battery condition.

## 10.2 Remaining battery time

Estimate remaining runtime when discharging.

Use energy remaining and a measured discharge-rate estimate where reliable.

Conceptually:

```text
Estimated Runtime =
Remaining Energy / Average Discharge Power
```

Use a rolling average to reduce noise.

Requirements:

- Ignore invalid or zero discharge readings.
- Require sufficient valid samples before presenting an estimate.
- Handle changes in power consumption.
- Recalculate when new measurements arrive.
- Clearly label the result as an estimate.
- Show an uncertainty or confidence indicator.
- Avoid presenting an exact countdown when the data is unstable.

If there is insufficient information, display:

"Calculating remaining time..."

or:

"Insufficient battery data for a reliable estimate."

Do not invent a remaining runtime.

## 10.3 Battery temperature

Only display battery temperature if a reliable battery-specific sensor exists.

Do not assume that a nearby CPU or chassis sensor represents battery temperature.

If unavailable, state that clearly.

## 10.4 Battery page

Display:

- Battery percentage.
- Charging/discharging state.
- Remaining runtime estimate.
- Battery health estimate.
- Design capacity.
- Full charge capacity.
- Current energy.
- Power draw.
- Temperature, if available.
- Cycle count.
- AC adapter state.
- Battery history.

Use a clear battery visualisation.

# 11. THERMAL AND FAN MONITORING

Build a thermal collector using:

- `/sys/class/hwmon/`
- `/sys/class/thermal/`

Discover available:

- Temperature sensors.
- Sensor labels.
- Fan speeds.
- Voltage sensors.
- Thermal zones.

Associate a sensor with a component only when the association is supported by reliable metadata.

Do not assume every temperature sensor belongs to the CPU.

## 11.1 Temperature interpretation

Display:

- Sensor name.
- Temperature.
- Unit.
- Source.
- Component association confidence.
- Last measurement.

Do not invent universal safe temperature thresholds.

Thermal limits differ between components and manufacturers.

Use verified manufacturer or driver-reported limits where available.

If a reliable threshold is unavailable, allow the user to configure a warning threshold and clearly label it as user-defined.

## 11.2 Fan monitoring

Read fan speed where exposed.

Do not control fan speeds.

Do not add fan-control functionality.

If fan information is unavailable, show an unavailable state.

# 12. GPU MONITORING

Support integrated and dedicated GPUs.

Use available Linux interfaces and vendor-supported read-only telemetry sources.

Collect where supported:

- GPU model.
- GPU utilisation.
- GPU memory usage.
- Dedicated VRAM usage.
- Shared memory information, where available.
- GPU temperature.
- GPU power draw.
- GPU clock information.
- GPU driver.
- GPU activity.

Support multiple GPUs.

Distinguish:

- Dedicated VRAM.
- Shared system memory.
- Total GPU memory.
- Unavailable GPU memory information.

Do not misrepresent shared RAM as dedicated VRAM.

Use vendor-specific adapters where necessary.

Possible sources include:

- DRM sysfs.
- AMD GPU sysfs interfaces.
- NVIDIA read-only monitoring interfaces.
- Intel GPU telemetry interfaces.

Feature-detect vendor capabilities.

Do not require installing GPU drivers or utilities.

Do not run GPU benchmarks.

Do not change GPU configuration.

The application must work completely without a dedicated GPU.

Do not use the GPU to render the Sentinel interface as a requirement.

# 13. STORAGE MONITORING

Implement storage telemetry.

Collect:

- Device read activity.
- Device write activity.
- Read throughput.
- Write throughput.
- I/O operations.
- I/O time.
- Device capacity.
- Used space.
- Available space.
- Filesystem usage.
- Mount information.

Use appropriate sources such as:

- `/proc/diskstats`
- `/sys/block/`
- `/proc/mounts`
- `statvfs` or equivalent read-only filesystem interfaces.

Distinguish:

- Physical disk usage.
- Partition usage.
- Filesystem usage.
- Disk I/O activity.

Do not mistake disk I/O activity for disk health.

Do not mount, unmount or repair filesystems.

SMART and NVMe health analysis will be expanded in a later phase.

# 14. NETWORK MONITORING

Implement live network monitoring.

Collect:

- Interface name.
- Interface type.
- Link state.
- Bytes received.
- Bytes transmitted.
- Receive rate.
- Transmit rate.
- Packet counts.
- Error counts.
- Dropped packet counts.

Use `/sys/class/net/` and suitable kernel statistics interfaces.

Calculate throughput using counter differences and actual elapsed time.

Support multiple interfaces.

Do not collect packet contents.

Do not perform packet capture.

Do not monitor private application communications.

Do not modify network configuration.

Network monitoring should be local and aggregate by interface.

# 15. SYSTEM LOAD AND HEALTH INDICATORS

Create a system health aggregation layer.

It should combine relevant measurements into a clear overview.

Possible indicators:

- CPU pressure.
- Memory pressure.
- Swap pressure.
- Thermal condition.
- Battery condition.
- Storage capacity.
- Storage activity.
- Network activity.
- Sensor availability.

Use real evidence.

Do not generate a single arbitrary health score that hides important details.

If a combined score is introduced later, its calculation must be transparent, documented and configurable.

For Phase 3, prefer understandable statuses:

- Normal.
- Elevated.
- High.
- Critical.
- Unknown.

Thresholds must be based on the relevant metric and its context.

Do not label a system critical merely because CPU usage reaches 100% briefly.

Consider duration, repetition and metric-specific conditions.

# 16. ALERT ENGINE

Create a separate alert evaluation engine.

It must evaluate telemetry and detect meaningful conditions.

Possible alert categories:

## CPU

- Sustained high CPU utilisation.
- Unusually high system load.
- Unusual CPU temperature, when a reliable limit exists.

## Memory

- Low available memory.
- Sustained memory pressure.
- High swap activity.

## Battery

- Low battery percentage.
- Battery temperature warning, if reliably measured.
- Battery health below a user-configured warning level.
- Battery discharge rate unusually high.

## Storage

- Low available storage.
- Sustained high I/O activity.
- Storage health warning, when reliable health data becomes available in a later phase.

## Thermal

- Sustained temperature above a configured or verified warning limit.
- Repeated thermal warnings.

## Network

- Unusually high aggregate traffic, if the user enables such a threshold.

# 17. ALERT QUALITY AND FALSE-POSITIVE PREVENTION

Do not create an alert for every temporary spike.

Implement:

- Duration thresholds.
- Consecutive-sample requirements.
- Hysteresis.
- Cooldown periods.
- Duplicate suppression.
- Recovery events.
- Per-alert severity.
- User-configurable thresholds.

Example:

A CPU warning may require utilisation above a configured threshold for a sustained period rather than one isolated sample.

When the metric returns to normal, the alert should resolve.

Do not repeatedly notify the user about the same unresolved condition.

## 17.1 Alert model

Use a structured model:

```rust
struct Alert {
    id: String,
    category: AlertCategory,
    severity: AlertSeverity,
    title: String,
    explanation: String,
    recommendation: Option<String>,
    metric_id: MetricId,
    detected_at: i64,
    last_updated_at: i64,
    status: AlertStatus,
}
```

Include evidence and the relevant threshold.

Do not present an unverified diagnosis as fact.

Use language such as "possible cause" where appropriate.

## 17.2 Recommendations

Recommendations must be informational.

They must never contain executable buttons.

Do not provide one-click fixes.

Do not invoke shell commands from alert cards.

A recommendation can explain what the user may consider doing, but the user must independently perform any action.

# 18. DESKTOP NOTIFICATIONS

Integrate with the Linux desktop notification system.

Use the appropriate notification mechanism supported by the desktop environment.

Notifications must be generated only for meaningful alerts.

Examples:

**Sentinel — High Memory Pressure**

Available memory has remained below your configured threshold.

Review the processes consuming the most memory.

**Sentinel — High CPU Temperature**

A supported temperature sensor has exceeded its warning threshold for a sustained period.

Check ventilation and review current system activity.

**Sentinel — Low Battery**

Your battery has reached the configured low-battery threshold.

Consider connecting your charger.

Notifications must not include automatic fix actions.

Do not repeatedly send notifications for the same unresolved issue.

Respect the user's notification preference.

If the desktop notification service is unavailable, record the event locally and continue monitoring.

# 19. LOCAL TELEMETRY HISTORY

Persist monitoring history locally.

Use SQLite or the existing storage abstraction.

Store:

- Timestamp.
- Metric identifier.
- Metric value.
- Unit.
- Source.
- Quality.
- Associated device or process identifier where appropriate.

Also persist alert history.

## 19.1 Retention

Provide configurable history retention.

Suggested defaults:

- Detailed telemetry: 24 hours.
- Aggregated hourly data: 30 days.
- Alert history: 90 days.

These are starting defaults.

Make retention configurable.

Avoid excessive database writes.

Use batched writes where appropriate.

Do not persist every high-frequency sample indefinitely.

Do not allow the history database to grow without limits.

## 19.2 Privacy

All telemetry remains local.

Do not upload metrics.

Do not transmit process names or hardware information.

Do not add analytics.

# 20. IPC EXPANSION

Extend the existing IPC protocol.

Add operations such as:

```text
StartMonitoring
StopMonitoring
PauseMonitoring
ResumeMonitoring
GetMonitoringStatus

GetLatestMetrics
GetCpuMetrics
GetMemoryMetrics
GetProcessMetrics
GetBatteryMetrics
GetThermalMetrics
GetGpuMetrics
GetStorageMetrics
GetNetworkMetrics

GetActiveAlerts
GetAlertHistory
AcknowledgeAlert

GetSamplingProfile
UpdateSamplingProfile

GetTelemetryHistory
```

Validate every request.

Return structured, typed responses.

Do not expose arbitrary command execution.

The frontend must never directly read system telemetry files.

The daemon owns collection and aggregation.

# 21. DASHBOARD UI

Follow the approved Sentinel visual design.

Keep:

- Black/dark background.
- White text.
- Coloured diagrams.
- Cyan and blue accents.
- Orange and red warning indicators.
- Subtle borders.
- High information density.
- Consistent typography.
- Clear spacing.

Do not replace the established design.

## 21.1 Main overview

The main dashboard should now display actual live information.

Include:

- CPU usage.
- Memory usage.
- Battery status.
- Temperature summary.
- Storage activity.
- Network activity.
- Active alerts.
- Monitoring status.
- Selected power mode.

Show the last update time.

Every metric must indicate whether it is live, estimated, cached or unavailable.

## 21.2 CPU page

Display:

- Overall CPU utilisation.
- Individual core utilisation.
- CPU topology diagram.
- CPU frequency, if available.
- Temperature sensors.
- Load average.
- Historical graph.

Selecting a core should reveal its telemetry details.

## 21.3 Memory page

Display:

- Total RAM.
- Used RAM.
- Available RAM.
- Cache.
- Swap.
- Memory pressure.
- Process-level memory ranking.
- Memory usage history.

## 21.4 Process page

Display:

- Process name.
- PID.
- CPU utilisation.
- Memory consumption.
- Process state.
- User, where accessible.

Allow search, sorting and filtering.

Do not provide process termination controls.

## 21.5 Battery page

Display:

- Battery percentage.
- Charging/discharging state.
- Remaining runtime estimate.
- Battery health estimate.
- Temperature, if available.
- Power draw.
- Cycle count.
- Historical battery behaviour.

Clearly label estimated values.

## 21.6 Thermal page

Display:

- Available temperature sensors.
- Fan speeds.
- Sensor labels.
- Component associations.
- Historical temperature graphs.
- Configured warning thresholds.

## 21.7 GPU page

Display actual GPU telemetry when supported.

If the GPU is unsupported or unavailable, show a clear explanation.

Do not create fake graphs.

## 21.8 Storage page

Display:

- Physical drives.
- Partitions.
- Capacity.
- Used space.
- Read/write activity.
- Historical I/O graphs.

## 21.9 Network page

Display:

- Network interfaces.
- Receive rate.
- Transmit rate.
- Total received.
- Total transmitted.
- Packet errors and drops.

## 21.10 Alerts and Events

Create a central alert history.

Show:

- Alert severity.
- Category.
- Detected time.
- Current status.
- Supporting metric.
- Explanation.
- Recommendation.
- Recovery time, where applicable.

Allow users to inspect and acknowledge alerts.

Acknowledging an alert must not modify the underlying system or suppress future unrelated alerts.

# 22. CHARTS AND VISUALISATIONS

Use efficient chart rendering.

Do not redraw every component unnecessarily.

Use bounded data buffers.

Avoid storing unlimited points in frontend state.

Implement:

- CPU utilisation graphs.
- Per-core graphs.
- Memory history.
- Process resource graphs where practical.
- Battery percentage history.
- Power draw history.
- Temperature graphs.
- Storage throughput graphs.
- Network throughput graphs.

Use consistent time ranges.

Provide selectable time windows where appropriate.

Charts must use actual measurements.

Clearly indicate gaps when collection fails.

Do not connect missing values with misleading continuous lines.

# 23. ERROR HANDLING AND RECOVERY

Handle:

- Missing sensor files.
- Disappearing devices.
- CPU hotplug.
- Process termination during sampling.
- Permission errors.
- Unsupported GPU telemetry.
- Battery interface changes.
- Malformed metric values.
- IPC disconnection.
- Database failures.
- Notification service failure.
- Collector timeouts.

A failed collector must not crash the entire daemon.

Use independent collector error handling.

Apply retry backoff.

Never convert collection errors into normal metric values.

# 24. TESTING REQUIREMENTS

## Unit tests

Test:

- CPU utilisation calculations.
- Per-core utilisation.
- Counter-delta calculations.
- Memory calculations.
- Process CPU calculations.
- Process memory calculations.
- Battery health calculation.
- Battery runtime estimation.
- Temperature conversion.
- Fan-speed parsing.
- GPU metric parsing.
- Storage throughput.
- Network throughput.
- Alert thresholds.
- Alert hysteresis.
- Alert cooldown.
- Recovery events.
- Sampling profile selection.
- History retention.

## Integration tests

Test:

- Monitoring start and stop.
- Monitoring pause and resume.
- CLI monitoring controls.
- Desktop monitoring controls.
- Daemon restart.
- Collector failures.
- Database writes.
- History retrieval.
- Alert generation.
- Desktop notification delivery.
- Power-mode changes.
- Multiple simultaneous collectors.

## Safety tests

Verify:

- No process is terminated.
- No service is modified.
- No CPU or GPU setting is changed.
- No power setting is changed.
- No battery charging configuration is changed.
- No network configuration is changed.
- No packages are installed or removed.
- No repair command is executed.
- No root privileges are requested.
- No arbitrary shell command is executed.
- Monitoring OFF stops telemetry collection.
- No fabricated values are displayed.
- No private process environment data is collected.

Use synthetic fixtures and mocks where necessary.

Do not run stress tests on the user's actual machine without explicit permission.

# 25. RESOURCE AND POWER BENCHMARKING

Measure the real application overhead.

Test:

1. Daemon running, monitoring OFF.
2. Monitoring ON in Eco mode.
3. Monitoring ON in Balanced mode.
4. Monitoring ON in Performance mode.
5. Desktop window open.
6. Desktop window closed while monitoring remains ON.

Measure:

- CPU usage.
- Memory usage.
- Wake-ups.
- Collection duration.
- Database writes.
- Battery impact where measurable.

Document the results.

Do not claim a battery-life improvement without a controlled measurement.

Do not require a dedicated GPU.

# 26. DOCUMENTATION

Update:

- README.md
- ARCHITECTURE.md
- SECURITY.md
- SAFETY_POLICY.md
- DEVELOPMENT.md

Create:

- TELEMETRY_ENGINE.md
- METRIC_REFERENCE.md
- ALERT_ENGINE.md
- POWER_EFFICIENCY.md
- MONITORING_LIMITATIONS.md

Document:

- Metric sources.
- Metric units.
- Sampling intervals.
- Calculation methods.
- Unsupported hardware.
- Permission limitations.
- Alert thresholds.
- Retention policy.
- Resource benchmarks.
- Known accuracy limitations.

# 27. DEFINITION OF DONE

Phase 3 is complete only when:

1. Monitoring can be enabled and disabled from the desktop application.
2. Monitoring can be controlled from the CLI.
3. Monitoring state is accurately reported.
4. CPU utilisation is measured correctly.
5. Per-core utilisation is measured correctly.
6. Memory usage is measured correctly.
7. Process-level CPU and memory information is displayed.
8. Battery information is collected.
9. Remaining battery time is estimated only when sufficient data exists.
10. Battery temperature is displayed only when reliably available.
11. Thermal sensors are discovered and monitored.
12. Fan speeds are monitored where supported.
13. GPU telemetry works where supported.
14. Storage I/O is monitored.
15. Network throughput is monitored.
16. System health indicators are evidence-based.
17. Alerts are generated using sustained conditions and hysteresis.
18. Desktop notifications work where supported.
19. Telemetry history is stored locally.
20. History retention is enforced.
21. Eco, Balanced and Performance profiles work.
22. The dashboard displays real telemetry.
23. Monitoring OFF stops collection.
24. The application works without a dedicated GPU.
25. Resource consumption is measured and documented.
26. All safety tests pass.
27. All relevant unit and integration tests pass.

# 28. STRICT PHASE BOUNDARY

Do not implement Phase 4 or later features.

The following are out of scope:

- Automatic motherboard schematic searching.
- Downloading motherboard schematic PDFs.
- PDF schematic extraction.
- OCR.
- AI-based schematic analysis.
- Physical motherboard reconstruction.
- Interactive motherboard component overlays.
- Automatic USB port-to-physical-connector mapping.
- Advanced predictive hardware failure models.
- Automatic repair functionality.
- System configuration management.

Prepare clean interfaces for later phases, but do not implement these capabilities.

# 29. FINAL ENGINEERING REPORT

When Phase 3 is complete, stop.

Provide a detailed report containing:

1. Implemented telemetry collectors.
2. Actual hardware metrics successfully collected.
3. Metrics unavailable on the development machine.
4. CPU and memory calculation validation.
5. Battery estimation methodology and accuracy limitations.
6. GPU telemetry support.
7. Thermal and fan sensor support.
8. Alert rules and threshold behaviour.
9. Notification integration.
10. Sampling profiles.
11. Database and retention implementation.
12. Resource benchmarks for every monitoring mode.
13. Unit and integration test results.
14. Safety verification.
15. Known limitations.
16. Outstanding issues.
17. Commands for starting, stopping and inspecting monitoring.

Clearly distinguish actual measurements from synthetic test results.

Do not claim a feature works on all Linux systems because it works on one laptop.

Wait for the user's instruction before starting Phase 4.

---

# INITIAL EXECUTION INSTRUCTION

Read the Phase 1 and Phase 2 documentation and inspect the existing implementation.

Verify that hardware discovery works before implementing telemetry.

Start by designing the metric model and central scheduler.

Implement collectors independently.

Validate all calculations against known Linux interfaces and controlled test fixtures.

Then integrate the telemetry engine with the daemon, IPC, CLI and desktop application.

Prioritise accuracy, safety and low resource consumption.

Do not install packages or modify system settings without explicit permission.

Do not use sudo.

Do not implement future schematic functionality.

**Sentinel observes, analyses and recommends. It never fixes or modifies the user's system.**

Stop after Phase 3 and wait for the user's instruction.

#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================

# SENTINEL — PHASE 4: DESKTOP INTEGRATION & RELIABILITY

## ROLE

Act as a senior Linux systems engineer, Rust engineer, Tauri desktop application engineer, QA engineer and software reliability specialist.

You are working on Sentinel, an advanced Linux desktop system intelligence and hardware monitoring application.

Your task is to complete **Phase 4 — Desktop Integration & Reliability**.

This is an existing project. Phases 1, 2 and 3 have already been implemented. You MUST preserve their architecture and functionality.

Do not rebuild Sentinel from scratch.

---

# 1. PROJECT CONTEXT

Sentinel is a Linux-first desktop application designed to provide users with a detailed understanding of their computer's hardware, software, performance, health and internal architecture.

Its long-term direction includes:
- Detailed hardware discovery.
- Live system monitoring.
- Hardware architecture visualisation.
- Automatic motherboard schematic discovery.
- Schematic and boardview processing.
- Interactive motherboard visualisation.
- Hardware diagnostics.
- Evidence-based system recommendations.
- Unified Linux system intelligence.

Sentinel is built using:
- Rust backend.
- Tauri desktop framework.
- React frontend.
- TypeScript.
- Background daemon.
- CLI.
- Unix socket IPC.
- SQLite historical storage.

The original Sentinel idea/concept conversation is available in the project folder.

**Before implementation:**
1. Locate and read the original Sentinel idea documentation/conversation in the project folder.
2. Understand the original product vision and intended behaviour.
3. Inspect the existing repository and its documentation.
4. Read existing architecture decisions, configuration files and development notes.
5. Identify what has already been implemented.
6. Use the original concept and existing implementation together as the source of truth.

Do not assume that the roadmap alone describes every existing implementation detail.

---

# 2. COMPLETED DEVELOPMENT PHASES

These phases are considered complete.

## Phase 1 — Project Foundation

- Rust backend and Cargo workspace.
- Tauri desktop application.
- React + TypeScript frontend.
- Background daemon and CLI.
- Unix socket IPC.
- Configuration and logging.
- Safety architecture.
- Initial dark Sentinel interface.

## Phase 2 — Hardware & Software Discovery

- 12 hardware discovery modules.
- CPU, RAM, GPU, motherboard, storage, battery, network, USB, PCI and sensors.
- Installed application and utility discovery.
- Systemd service discovery.
- 64 tools across 15 categories.
- Scan queue, cancellation and caching.
- Hardware, utilities and service UI pages.
- 273 Rust tests and 41 frontend tests passed at phase completion.

## Phase 3 — Live Monitoring, Alerts & History

- 10 live monitoring collectors.
- Per-core CPU monitoring.
- Process-level resource monitoring.
- Eco, Balanced and Performance modes.
- 13 alert rules with sustained-condition checks.
- Desktop notification integration.
- SQLite historical data.
- Telemetry, alerts and settings pages.
- 342 Rust tests and 57 frontend tests passed at phase completion.
- Low background resource consumption verified.

These are historical completion figures, not permission to assume that all tests still pass.

You must independently establish the current state.

---

# 3. PHASE 4 OBJECTIVE

## Desktop Integration & Reliability

The objective is to ensure that all existing Sentinel components work correctly together in a real Linux desktop environment.

The application must be reliable during:

- Normal application startup.
- Daemon startup and shutdown.
- Desktop navigation.
- Hardware discovery.
- Continuous monitoring.
- Alert generation.
- Desktop notifications.
- Historical data collection.
- Settings changes.
- Application restarts.
- Daemon crashes and recovery.
- System sleep and resume, where supported.

The final result must be a stable application rather than a collection of individually working modules.

Do not introduce major new product features during this phase.

Focus on integration, correctness, lifecycle management, resource consumption and reliability.

---

# 4. IMPLEMENTATION WORKSTREAMS

## WORKSTREAM A — COMPLETE EXISTING APPLICATION AUDIT

Before changing code, inspect the complete repository.

Examine:

1. Rust workspace and crate dependencies.
2. Daemon lifecycle and process management.
3. CLI commands.
4. Tauri configuration and application lifecycle.
5. IPC protocol and connection handling.
6. React routing and page navigation.
7. Frontend state management.
8. Hardware discovery integration.
9. Monitoring collectors.
10. Monitoring mode implementation.
11. Alert engine.
12. Notification integration.
13. SQLite storage and migrations.
14. Historical data retrieval.
15. Configuration persistence.
16. Error handling and logging.
17. Existing automated tests.

Produce an audit identifying:

- Working functionality.
- Partially implemented functionality.
- Broken functionality.
- Missing integration.
- Potential race conditions.
- Resource leaks.
- Reliability issues.
- Unsupported hardware behaviour.
- Existing technical debt.

Create a Phase 4 issue register.

Do not rewrite working components simply to make the architecture look cleaner.

## WORKSTREAM B — DESKTOP APPLICATION INTEGRATION

Verify the complete desktop experience.

### Navigation

Test every existing page:

- Dashboard.
- Hardware.
- Utilities.
- Services.
- Telemetry.
- Alerts.
- History.
- Settings.

Requirements:

- Every navigation item opens the correct page.
- Navigation works repeatedly without refreshing the application.
- No page renders stale information after navigation.
- Loading states are correct.
- Empty states are handled.
- Errors are displayed clearly.
- No uncaught frontend exceptions.
- No broken IPC requests.
- No unnecessary duplicate data fetching.

### Application startup

Verify:

- Tauri application launches correctly.
- Daemon connection is established.
- Initial application state is loaded.
- Hardware information is available.
- Monitoring status is displayed accurately.
- Existing settings are loaded.
- Historical data is accessible.
- Startup failures are handled gracefully.

The frontend must not pretend that Sentinel is monitoring when the daemon is disconnected.

### Application shutdown

Verify:

- Frontend shutdown does not leave unnecessary processes behind.
- Daemon lifecycle follows the existing design.
- IPC connections are closed correctly.
- Database writes are not interrupted.
- Monitoring tasks are stopped or retained according to the intended daemon lifecycle.
- Restarting the UI does not create duplicate collectors.

Preserve the existing architecture's ownership of the daemon lifecycle.

## WORKSTREAM C — DAEMON RELIABILITY AND IPC

This is a critical workstream.

Test the following scenarios:

1. Start Sentinel normally.
2. Start the daemon independently.
3. Launch the UI while the daemon is already running.
4. Restart the UI without restarting the daemon.
5. Stop the daemon while the UI is open.
6. Restart the daemon while the UI is open.
7. Simulate an IPC connection failure.
8. Simulate a slow IPC response.
9. Simulate an invalid or incomplete response.
10. Test repeated connection and disconnection.
11. Test daemon recovery after an unexpected termination.

Requirements:

- No application crash due to daemon disconnection.
- No infinite reconnection loops.
- No unbounded background tasks.
- No duplicate monitoring collectors.
- No stale connection state.
- No permanently blocked frontend.
- Clear daemon connection status.
- Appropriate retry and backoff behaviour.
- Requests must have sensible timeout handling.
- Reconnection must restore the correct application state.

Do not introduce a second daemon or duplicate monitoring architecture.

Use the existing IPC protocol wherever possible.

If protocol changes are necessary, maintain backward compatibility where practical and document them.

## WORKSTREAM D — LIVE MONITORING VERIFICATION

Verify all existing monitoring collectors.

Test:

- CPU utilisation.
- Per-core CPU utilisation.
- RAM usage.
- Swap usage.
- GPU utilisation where supported.
- Storage utilisation and activity where available.
- Battery status.
- Battery charging/discharging state.
- Network activity.
- Temperature sensors.
- Process-level resource consumption.

### Accuracy requirements

Compare Sentinel's measurements against appropriate Linux system sources.

Possible reference sources include:

- /proc/stat
- /proc/meminfo
- /proc/diskstats
- /proc/net/dev
- /proc/[pid]/
- /sys/class/power_supply/
- /sys/class/thermal/
- /sys/class/hwmon/

Use only the sources relevant to each metric.

Do not assume that every device exposes every metric.

Investigate:

- Incorrect sampling intervals.
- Incorrect CPU calculations.
- Per-core aggregation mistakes.
- Missing sensor readings.
- Invalid zero values.
- Stale values.
- Incorrect units.
- Incorrect temperature conversions.
- Process disappearance during sampling.
- Counter resets.
- Unsupported GPU telemetry.
- Missing battery information.

### Data integrity

Every measurement should have appropriate metadata:

- Value.
- Unit.
- Timestamp.
- Source.
- Availability.
- Validity or reliability status where appropriate.

Never substitute fabricated values for unavailable measurements.

If GPU utilisation cannot be read, show it as unavailable rather than zero.

If a temperature sensor is unavailable, do not invent a temperature.

## WORKSTREAM E — MONITORING MODES AND SETTINGS

Verify the three existing modes:

- Eco.
- Balanced.
- Performance.

Important: These are Sentinel monitoring modes, not Linux CPU governor controls or system power-profile modifications.

The modes must affect Sentinel's own monitoring behaviour only.

Verify:

- Correct mode selection.
- Correct sampling intervals.
- Correct collector scheduling.
- Correct UI updates.
- Correct persistence.
- Correct behaviour after application restart.
- Correct behaviour after daemon restart.
- No duplicated collectors when switching modes.
- No background resource spikes.

Inspect all existing settings.

Verify that:

- Every functional setting actually changes the intended behaviour.
- Settings persist after restart.
- Invalid values are rejected safely.
- Default values are sensible.
- Settings do not conflict with one another.
- The frontend reflects the actual backend configuration.

Do not add fake settings or decorative controls.

Do not modify operating-system power settings.

## WORKSTREAM F — ALERTS AND DESKTOP NOTIFICATIONS

Verify all 13 existing alert rules.

Test:

- Alert threshold evaluation.
- Sustained-condition timing.
- Alert severity.
- Alert deduplication.
- Alert cooldown.
- Alert resolution.
- Repeated threshold crossings.
- Alert history.
- Alert persistence.
- Alert configuration.
- Behaviour during daemon restart.

### Desktop notifications

Verify actual desktop notification delivery.

Do not consider a notification system complete merely because an alert appears in Sentinel's React interface.

Test:

1. Generate a controlled test alert.
2. Confirm that the backend detects it.
3. Confirm that the alert is persisted.
4. Confirm that the notification integration is invoked.
5. Confirm that the Linux desktop actually displays the notification.
6. Confirm that notification failure does not crash the daemon.
7. Confirm that repeated alerts are not excessively noisy.

Investigate desktop notification support and permissions.

Use the existing notification implementation if it is correct.

Where notification services are unavailable, Sentinel must degrade gracefully and communicate the limitation.

Never generate a real high-temperature or other dangerous hardware condition just to test an alert.

Use controlled test fixtures or a safe test-only alert mechanism.

Ensure test mechanisms cannot accidentally trigger destructive operations.

## WORKSTREAM G — HISTORICAL DATA AND SQLITE RELIABILITY

Verify the existing SQLite history engine.

Test:

- Telemetry insertion.
- Telemetry retrieval.
- Timestamp ordering.
- Data retention.
- Historical filtering.
- Alert history.
- Application restart.
- Daemon restart.
- Database recovery.
- Database locking.
- Concurrent access.
- Migration behaviour.
- Missing or corrupt database handling.

Investigate:

- Unbounded database growth.
- Excessive write frequency.
- Database locking.
- Long-running queries.
- Incorrect timestamp conversion.
- Duplicate records.
- Missing records.
- Incorrect retention cleanup.
- Unnecessary database writes.

Preserve historical records during normal application upgrades.

Do not delete user data as part of reliability testing.

Use isolated temporary databases for destructive test scenarios.

Any retention mechanism must follow existing configuration and documented user expectations.

## WORKSTREAM H — BATTERY ACCURACY AND POWER IMPACT

Investigate battery telemetry and its reliability.

Verify:

- Battery presence detection.
- Battery capacity.
- Energy or charge values.
- Charging state.
- Discharging state.
- Full-charge state.
- Remaining capacity.
- Power consumption measurements where available.
- Battery health information where genuinely exposed by hardware.

### Battery runtime estimation

If Sentinel currently displays estimated remaining runtime, investigate its accuracy.

Check:

- Whether the estimate is based on instantaneous or averaged discharge rate.
- Whether the battery is charging.
- Whether the battery is discharging.
- Whether the discharge rate is stable.
- Whether the battery is connected to AC power.
- Whether the measurement is available.
- Whether the estimate becomes misleading during sudden workload changes.

Do not present unreliable runtime estimates as precise predictions.

Use an appropriate rolling average where justified.

Display unavailable or insufficient-data states honestly.

### Power impact testing

Measure Sentinel's own resource consumption.

Evaluate:

- Idle CPU usage.
- Active monitoring CPU usage.
- RAM consumption.
- Daemon overhead.
- Database write activity.
- Wakeups and unnecessary polling.
- Resource impact in Eco, Balanced and Performance modes.

Do not claim battery savings without measurements.

The purpose is to ensure Sentinel itself does not unnecessarily consume the resources it is monitoring.

## WORKSTREAM I — GPU HARDWARE VALIDATION

Test the existing GPU discovery and monitoring implementation.

Support should be capability-based.

Where available, inspect:

- GPU identification.
- Vendor and device information.
- Driver information.
- GPU utilisation.
- VRAM information.
- Temperature.
- Clock speeds.
- Power consumption.

Not all GPUs expose these values.

Test AMD, Intel and NVIDIA hardware where access to the respective hardware is available.

Use the appropriate existing Linux interfaces and vendor-supported read-only telemetry mechanisms.

Important:

- Do not install proprietary drivers.
- Do not change GPU drivers.
- Do not change GPU power profiles.
- Do not attempt GPU overclocking.
- Do not attempt stress testing that risks overheating hardware.
- Do not fabricate unavailable GPU metrics.

Where physical hardware is unavailable, implement or improve mock-based tests and clearly report that physical validation remains outstanding.

## WORKSTREAM J — SYSTEM SLEEP, RESUME AND LONG-RUN RELIABILITY

Investigate behaviour during:

- Screen lock.
- Display sleep.
- System suspend.
- System resume.
- Temporary network disconnection.
- Temporary hardware telemetry failure.
- Long-running monitoring sessions.

Verify:

- No stale telemetry after resume.
- No duplicate collectors after resume.
- Correct timestamp handling.
- No excessive CPU usage after resume.
- No unrecoverable IPC state.
- No database corruption.
- No repeated notification bursts.
- No resource leaks during extended operation.

Test a sustained monitoring session, preferably for at least 2 hours where the environment permits.

Record any remaining long-duration limitations.

---

# 5. TESTING STRATEGY

Do not rely solely on unit tests.

Phase 4 must include multiple testing levels.

## A. Unit testing

Test individual:

- Monitoring collectors.
- Metric conversion functions.
- Alert rules.
- Configuration logic.
- Battery calculations.
- Runtime estimators.
- Database operations.
- IPC serialization and validation.

## B. Integration testing

Test:

- Daemon + collectors.
- Daemon + SQLite.
- Daemon + IPC.
- Tauri + daemon.
- React + IPC.
- Alert engine + notification service.
- Settings + monitoring modes.
- Monitoring + historical storage.

## C. Frontend testing

Test:

- Navigation.
- Loading states.
- Error states.
- Disconnection states.
- Settings interaction.
- Monitoring mode changes.
- Alert display.
- Historical data display.

## D. End-to-end testing

Where feasible, run the actual Tauri application and verify the complete workflow.

Do not substitute mocked UI tests for real desktop integration testing.

## E. Regression testing

Run all existing tests from Phases 1–3.

Record:

- Previous baseline.
- Current test count.
- Passed tests.
- Failed tests.
- Skipped tests.
- New tests.
- Regression findings.

Do not delete or weaken existing tests to achieve a passing test count.

---

# 6. HARDWARE AND ENVIRONMENT TEST MATRIX

Use the following compatibility matrix.

| Environment | Validation |
|---|---|
| Ubuntu/Debian | Primary Linux compatibility |
| GNOME | Desktop navigation and notifications |
| KDE Plasma | Desktop notifications and UI behaviour |
| AMD GPU | Discovery and supported telemetry |
| Intel GPU | Discovery and supported telemetry |
| NVIDIA GPU | Discovery and supported telemetry |
| CPU-only systems | Graceful GPU fallback |
| 8 GB RAM systems | Resource consumption |
| 16 GB RAM systems | Normal desktop operation |
| Laptop with battery | Battery telemetry and power impact |
| Desktop without battery | Graceful battery fallback |

Do not claim that a test passed unless it was actually performed.

When hardware or desktop environments are unavailable, mark the test as:

- Not tested.
- Mock tested.
- Partially validated.
- Physically validated.

Clearly distinguish these statuses.

---

# 7. RESOURCE CONSUMPTION TARGETS

Sentinel must remain lightweight.

The application should be usable on ordinary Linux laptops, including systems with 8 GB RAM and integrated graphics.

Investigate these targets:

- Low idle CPU overhead.
- No uncontrolled RAM growth.
- No unnecessary background polling.
- No duplicate monitoring processes.
- No excessive SQLite writes.
- No unnecessary wakeups.
- No GPU dependency.
- No memory leaks during extended operation.

Establish actual measured baselines before setting strict numerical performance thresholds.

If an existing target is exceeded, investigate the cause before attempting optimisation.

Do not compromise telemetry correctness simply to reduce resource usage.

---

# 8. STRICT SAFETY AND PRIVACY REQUIREMENTS

These requirements are non-negotiable.

## Read-only operation

Sentinel must NEVER:

- Modify CPU governor settings.
- Change GPU power settings.
- Change system power profiles.
- Modify BIOS or UEFI settings.
- Modify kernel parameters.
- Change systemd services or enable/disable them.
- Install or remove packages.
- Update the operating system.
- Modify drivers.
- Change hardware configurations.
- Modify network configurations.
- Execute automatic fixes.
- Apply maintenance recommendations.

Sentinel is an inspection, monitoring, analysis and recommendation application.

It is NOT a system repair utility.

## Testing restrictions

Do not use destructive commands or tests.

Never:

- Delete system files.
- Modify system configuration to simulate failures.
- Stop critical operating-system services.
- Intentionally trigger hardware overheating.
- Exhaust available memory.
- Perform dangerous disk stress tests.
- Alter power-management settings.

Use test doubles, mocks, controlled fixtures and isolated temporary resources.

## Privacy

- Hardware information remains local.
- Telemetry history remains local.
- No private system information should be uploaded.
- No automatic telemetry collection by external services.
- Online access must not be introduced unnecessarily.
- Logging must not expose sensitive information.

---

# 9. IMPLEMENTATION ORDER

Follow this sequence.

### Stage 1 — Repository and architecture audit

- Inspect the original Sentinel concept.
- Inspect the repository.
- Understand the existing architecture.
- Establish test baselines.
- Produce the issue register.

Do not start feature implementation before this stage is complete.

### Stage 2 — Critical lifecycle and IPC reliability

- Application startup.
- Daemon lifecycle.
- IPC disconnection.
- Reconnection.
- Restart handling.
- Error propagation.

Resolve critical reliability issues first.

### Stage 3 — Desktop integration

- Navigation.
- Page loading.
- Frontend state.
- Backend connectivity.
- Application shutdown.
- Settings persistence.

### Stage 4 — Monitoring and alert verification

- Collector accuracy.
- Monitoring modes.
- Alert engine.
- Actual desktop notifications.
- Historical integration.

### Stage 5 — Battery, GPU and resource validation

- Battery telemetry.
- Runtime estimation.
- GPU capability detection.
- Power impact.
- Memory and CPU profiling.

### Stage 6 — Long-run reliability

- Suspend/resume.
- Extended monitoring.
- Database reliability.
- Recovery testing.
- Resource leak investigation.

### Stage 7 — Regression and acceptance testing

- Full Rust test suite.
- Full frontend test suite.
- Integration tests.
- End-to-end tests.
- Build verification.
- Safety audit.

### Stage 8 — Documentation and handover

Update the project documentation and prepare the Phase 4 completion report.

---

# 10. REQUIRED DOCUMENTATION

Create or update appropriate documentation in the existing documentation structure.

Do not create duplicate documents unnecessarily.

Required documentation:

1. Phase 4 implementation report.
2. Desktop integration test report.
3. Hardware compatibility matrix.
4. Resource consumption measurements.
5. Notification validation report.
6. Daemon and IPC reliability report.
7. Known issues and limitations.
8. Remaining physical hardware validation requirements.

Record:

- What was tested.
- How it was tested.
- The environment.
- The actual result.
- Whether the test was physical or simulated.
- Any unresolved problems.

---

# 11. DEFINITION OF DONE

Phase 4 is complete only when all applicable requirements below have been verified.

## Desktop integration

- [ ] All existing pages work.
- [ ] Navigation is reliable.
- [ ] UI correctly reflects daemon state.
- [ ] Loading and error states work.
- [ ] No critical frontend exceptions.
- [ ] Application startup and shutdown are reliable.

## Daemon and IPC

- [ ] Daemon starts and operates correctly.
- [ ] UI can connect to an existing daemon.
- [ ] UI survives daemon disconnection.
- [ ] Reconnection works.
- [ ] Daemon restart does not create duplicate collectors.
- [ ] IPC errors are handled correctly.

## Monitoring

- [ ] All existing collectors are tested.
- [ ] Measurement units are correct.
- [ ] Timestamps are correct.
- [ ] Unavailable metrics are handled honestly.
- [ ] Monitoring modes work.
- [ ] Settings persist.
- [ ] No unnecessary resource consumption.

## Alerts and history

- [ ] All 13 alert rules are verified.
- [ ] Sustained-condition checks work.
- [ ] Alert deduplication works.
- [ ] Desktop notifications are physically verified where supported.
- [ ] SQLite records remain consistent.
- [ ] Historical data survives restarts.

## Hardware

- [ ] Battery telemetry is verified where available.
- [ ] Runtime estimation is investigated.
- [ ] GPU discovery is verified.
- [ ] Unsupported hardware is handled gracefully.
- [ ] CPU-only systems remain supported.

## Reliability

- [ ] Sleep/resume behaviour is investigated.
- [ ] Extended monitoring is tested.
- [ ] No critical memory leaks.
- [ ] No uncontrolled database growth.
- [ ] No critical crash or recovery issues.

## Safety

- [ ] No operating-system modifications introduced.
- [ ] No automatic fixes introduced.
- [ ] No maintenance execution introduced.
- [ ] No private telemetry uploads introduced.
- [ ] All testing respects the read-only restriction.

## Regression

- [ ] Existing Rust tests pass.
- [ ] Existing frontend tests pass.
- [ ] New tests pass.
- [ ] Application builds successfully.
- [ ] No completed Phase 1–3 functionality has regressed.

A test that cannot be physically performed must remain explicitly marked as outstanding. Do not mark the phase complete by silently skipping it.

---

# 12. IMPORTANT DEVELOPMENT RULES

1. Inspect before implementing.
2. Preserve working architecture.
3. Reuse existing modules.
4. Do not duplicate collectors.
5. Do not introduce unnecessary dependencies.
6. Do not rewrite entire files without a technical reason.
7. Keep Rust code modular.
8. Keep TypeScript types consistent with backend contracts.
9. Maintain existing IPC compatibility wherever possible.
10. Do not remove working UI components.
11. Do not replace real data with mock data.
12. Do not hide errors.
13. Do not fabricate hardware measurements.
14. Do not claim unperformed tests passed.
15. Do not expand the scope into Phase 5.

If a problem requires a larger architectural change, explain the issue and propose the smallest safe solution before implementing it.

If you discover an issue belonging to a future phase, document it rather than prematurely implementing that phase.

---

# 13. FINAL HANDOVER REPORT

When implementation is finished, provide a detailed report with these sections:

### A. Executive summary
What Phase 4 achieved.

### B. Repository findings
Important problems discovered during the initial audit.

### C. Implementation changes
Files created, modified and removed, with reasons.

### D. Reliability improvements
Daemon, IPC, UI, monitoring, alerts and database improvements.

### E. Hardware validation
Actual hardware tested and unsupported capabilities.

### F. Resource measurements
CPU, RAM, database activity and power impact.

### G. Test results
Exact test counts and pass/fail results.

### H. Safety verification
Confirm which safety boundaries were audited and whether any violations were found.

### I. Outstanding issues
Anything not fully verified or still requiring attention.

### J. Phase completion status
Choose one:

- COMPLETE — All applicable acceptance criteria passed.
- PARTIALLY COMPLETE — Some criteria remain outstanding.
- BLOCKED — Critical problems prevent completion.

Never declare COMPLETE simply because the application compiles.

### K. Next development phase

Explain how Phase 4 prepares Sentinel for:

**Phase 5 — Hardware Explorer & Component Visualisation.**

Do not begin Phase 5.

---

# FINAL INSTRUCTION

Start with the original Sentinel idea documentation and repository inspection.

Then produce the audit and implementation plan.

After the audit, proceed through the implementation stages systematically.

Do not rush to change code before understanding the existing system.

The objective is not to add more features. It is to make everything already built dependable, accurate, lightweight and reliable on real Linux desktops.

**Sentinel must remain a read-only system intelligence platform throughout this entire phase.**

#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================

# SENTINEL — PHASE 5: HARDWARE EXPLORER & COMPONENT VISUALISATION

## ROLE

Act as a senior Linux hardware engineer, Rust systems programmer, frontend architect, computer architecture specialist and technical visualisation engineer.

You are working on Sentinel, an advanced Linux desktop system intelligence application.

Your task is to implement:

**PHASE 5 — HARDWARE EXPLORER & COMPONENT VISUALISATION**

This phase transforms Sentinel's existing hardware discovery information into an interactive hardware exploration experience.

You MUST build on the existing implementation from Phases 1–4.

Do not rebuild the application.
Do not replace working discovery modules.
Do not introduce fabricated hardware information.
Do not begin schematic discovery or motherboard boardview implementation prematurely.

---

# 1. PROJECT CONTEXT

Sentinel is a Linux-first desktop system intelligence application.

Its purpose is to help users understand their computer at multiple levels:

- Hardware identification.
- Hardware architecture.
- Component relationships.
- Live resource utilisation.
- Hardware health.
- Physical component documentation.
- Electrical connections.
- Diagnostics and recommendations.

The long-term objective is to create an application that can identify a user's actual laptop hardware, understand its architecture, discover matching technical documentation and eventually provide interactive motherboard visualisation.

The existing technology stack:

- Rust backend.
- Tauri desktop application.
- React frontend.
- TypeScript.
- SQLite.
- Background daemon.
- CLI.
- Unix socket IPC.

The original Sentinel idea documentation exists in the project folder.

Before implementation:

1. Locate and read the original Sentinel idea documentation.
2. Inspect the existing repository.
3. Review the hardware discovery implementation from Phase 2.
4. Review monitoring and telemetry from Phase 3.
5. Review integration and reliability improvements from Phase 4.
6. Understand the existing frontend design system.
7. Identify existing hardware data structures and IPC contracts.

Do not assume that the architecture described here perfectly matches the current codebase. Adapt to the actual implementation.

If Phase 4 has not been completed, do not assume its reliability requirements have been satisfied. Identify any blocking issues.

---

# 2. PHASE OBJECTIVE

Build a dedicated **Hardware Explorer** experience.

Users should be able to explore their actual computer through interactive component representations.

The experience should include:

1. CPU architecture and core topology.
2. CPU core and thread visualisation.
3. CPU cache hierarchy.
4. RAM modules and memory topology.
5. GPU architecture overview.
6. Storage device visualisation.
7. Battery information visualisation.
8. Network adapter visualisation.
9. USB controller and device hierarchy.
10. PCI device hierarchy.
11. Interactive component selection.
12. Live component information panels.
13. Hardware source and reliability indicators.

The visualisations should communicate actual technical relationships, not merely display decorative hardware illustrations.

---

# 3. CORE ARCHITECTURAL REQUIREMENTS

## A. Separate hardware data from visualisation

Create a clean architecture in which:

- Rust discovers and normalises hardware information.
- Existing collectors provide live telemetry.
- The frontend consumes structured hardware models.
- Visual components render those models.
- User interactions select and inspect components.

Do not place hardware discovery logic inside React components.

Do not duplicate existing hardware discovery functionality.

Do not make frontend components execute arbitrary shell commands.

## B. Hardware representation model

Inspect existing hardware types before creating new ones.

Where appropriate, establish or extend a normalised model containing:

- Stable component identifier.
- Component category.
- Display name.
- Manufacturer.
- Model.
- Hardware identifier.
- Parent component.
- Child components.
- Detected specifications.
- Supported capabilities.
- Current telemetry references.
- Data source.
- Detection timestamp.
- Availability status.
- Confidence or reliability metadata.

Not every field applies to every component.

Use optional fields where information is unavailable.

Avoid creating multiple competing representations of the same hardware.

## C. Distinguish physical and logical topology

This distinction is extremely important.

For example:

A CPU may expose:
- Physical package.
- Chiplets or dies, if detectable.
- Physical cores.
- Logical processors.
- Threads.
- Cache levels.

Linux CPU topology information does not necessarily reveal the exact physical arrangement of cores on the silicon.

Sentinel must distinguish:

- Confirmed hardware topology.
- Operating-system-reported topology.
- Inferred relationships.
- Unknown physical arrangements.

Never label a logical representation as a verified physical die layout.

---

# 4. WORKSTREAM A — HARDWARE EXPLORER FOUNDATION

Create a dedicated Hardware Explorer experience within the existing application.

Inspect the current navigation structure and integrate it without breaking existing pages.

The experience should provide:

### Hardware overview

A structured overview of detected components:

- Processor.
- Memory.
- Graphics.
- Storage.
- Battery.
- Network.
- USB.
- PCI devices.
- Motherboard.

Each category should display:

- Component identification.
- Important specifications.
- Availability.
- Data reliability.
- Relevant live telemetry, where available.

### Component navigation

Users should be able to:

1. Select a hardware category.
2. See its detected components.
3. Select an individual component.
4. Explore its technical representation.
5. View detailed specifications.
6. View associated live measurements.
7. Identify unavailable or unsupported information.

Use stable component IDs.

Selections should remain consistent when data refreshes.

Provide meaningful empty states for unsupported or unavailable hardware.

---

# 5. WORKSTREAM B — CPU ARCHITECTURE VISUALISATION

This is one of the most important components of Phase 5.

Create an interactive CPU architecture explorer.

## CPU overview

Display:

- CPU manufacturer.
- CPU model.
- Architecture.
- CPU family.
- Model identifier.
- Socket information where detectable.
- Number of physical cores.
- Number of logical processors.
- Maximum reported frequency.
- Current frequency where available.
- Cache information.
- Supported instruction sets where discoverable.

Do not invent specifications.

## Core topology

Create an interactive topology visualisation.

Represent:

- CPU package.
- Physical cores.
- Logical processors.
- Core-to-thread relationships.
- NUMA nodes where available.
- Die or chiplet relationships only when actually supported by detected data.

Users should be able to select an individual core or logical processor.

Selecting a core should reveal:

- Core identifier.
- Associated logical processors.
- Current utilisation.
- Current frequency where available.
- Relevant topology metadata.

## Live utilisation

Integrate the existing monitoring collectors.

Show per-core utilisation through:

- Visual utilisation indicators.
- Percentage values.
- Optional historical trend.
- Current sampling timestamp.

Do not create another CPU monitoring collector.

Use existing telemetry.

Ensure that the visualisation remains responsive during frequent updates.

## Cache hierarchy

Visualise the available cache levels:

- L1 Data.
- L1 Instruction.
- L2.
- L3.

Display, where discoverable:

- Cache size.
- Cache level.
- Cache type.
- Number of instances.
- Shared CPU list.
- Associated cores.

Linux cache topology information may be available through:

/sys/devices/system/cpu/cpu*/cache/

Use the existing discovery architecture and appropriate read-only interfaces.

If the CPU exposes a shared L3 cache, show the sharing relationship rather than displaying it as an independent cache for every core.

## CPU visualisation restrictions

Do not invent:

- Silicon floorplans.
- Core positions.
- Die boundaries.
- Chiplet positions.
- Internal transistor arrangements.

A logical topology diagram is acceptable.

A physically accurate CPU die diagram is outside Phase 5 unless actual verified documentation is available.

---

# 6. WORKSTREAM C — RAM AND MEMORY TOPOLOGY

Create a dedicated memory explorer.

Display:

- Total installed memory.
- Available memory.
- Used memory.
- Swap capacity.
- Swap utilisation.
- Memory type where detectable.
- Memory speed where detectable.
- Module manufacturer where detectable.
- Module capacity.
- Module identifiers.
- Memory channel information where detectable.
- NUMA memory topology where supported.

## Memory module visualisation

Where firmware information is available, represent detected memory modules individually.

For example:

- DIMM A1.
- DIMM A2.
- DIMM B1.
- DIMM B2.

These are illustrative identifiers only. Use actual firmware-reported identifiers.

Important:

Do not assume that every laptop exposes removable DIMM slots.

For soldered memory, distinguish detected memory devices from physical removable slots.

Do not invent:

- Empty RAM slots.
- Module placement.
- Channel configuration.
- Memory speed.
- Physical slot positions.

If slot information cannot be obtained, display the limitation explicitly.

## Live memory information

Integrate existing telemetry for:

- Memory utilisation.
- Available memory.
- Swap utilisation.
- Memory pressure where already supported.

Clearly distinguish installed capacity from currently usable memory.

---

# 7. WORKSTREAM D — GPU ARCHITECTURE OVERVIEW

Create an interactive GPU explorer.

Support capability-based detection for:

- AMD.
- Intel.
- NVIDIA.

Display available information:

- GPU manufacturer.
- GPU model.
- Device identifier.
- PCI identifier.
- Driver.
- VRAM capacity.
- GPU utilisation.
- Temperature.
- Clock speed.
- Power consumption.
- Shared memory information where applicable.

Not all hardware exposes every metric.

## GPU visualisation

Create a logical GPU architecture overview showing available functional information.

Possible elements:

- GPU device.
- Graphics processing resources.
- Memory.
- Driver.
- PCI connection.
- Available telemetry.

Only show architecture-specific units when the hardware interface or trusted metadata supports them.

Do not invent shader counts, compute units, execution units or physical GPU arrangements.

Integrated GPUs must be represented correctly.

Distinguish:

- Dedicated GPU memory.
- Shared system memory.
- Reported memory capacity.
- Unknown memory configuration.

Do not imply that shared memory is dedicated VRAM.

---

# 8. WORKSTREAM E — STORAGE VISUALISATION

Create an interactive storage explorer.

Display:

- Storage devices.
- Device model.
- Manufacturer.
- Device type.
- Capacity.
- Interface.
- Logical block devices.
- Partitions.
- Filesystem information where available.
- Mount points.
- Device health information where supported.

Distinguish:

- Physical storage devices.
- Logical block devices.
- Partitions.
- Filesystems.
- Mount points.

Represent their relationships visually.

For example:

Physical SSD
  └── Logical device
       ├── Partition 1
       └── Partition 2

This is a logical example, not a configuration to hardcode.

Use actual discovered relationships.

Integrate existing storage telemetry where supported.

Do not perform disk benchmarks or modify partitions.

Do not expose sensitive filesystem contents.

---

# 9. WORKSTREAM F — BATTERY VISUALISATION

Create a battery information explorer.

Display available information:

- Battery identifier.
- Manufacturer.
- Model.
- Technology.
- Design capacity.
- Current full-charge capacity.
- Current energy or charge.
- Voltage.
- Charging state.
- Cycle count.
- Health estimation where supported.
- Current power draw where available.
- Remaining runtime estimate where sufficiently reliable.

Visualise:

- Battery capacity.
- Current charge.
- Charging/discharging state.
- Health information.
- Historical capacity information where already stored.

Clearly distinguish firmware-reported values from calculated estimates.

Do not imply that a battery's reported full-charge capacity is a precise measurement of its actual chemical condition.

Do not invent battery cycle counts or health percentages.

---

# 10. WORKSTREAM G — NETWORK AND USB VISUALISATION

## Network explorer

Display detected network devices.

Where available, include:

- Adapter name.
- Manufacturer.
- Model.
- Interface name.
- MAC address, subject to appropriate privacy-conscious presentation.
- Driver.
- Connection status.
- Link speed.
- Interface type.
- IP configuration where already collected.
- Current network activity.

Do not expose unnecessary private network information in the overview.

Use the existing network collectors.

## USB explorer

Create an interactive USB device hierarchy.

Represent:

- USB host controllers.
- Root hubs.
- USB hubs.
- Connected devices.
- Vendor ID.
- Product ID.
- Manufacturer.
- Product name.
- Device class.
- Connection state where available.

Use actual discovered parent-child relationships.

Do not fabricate USB port positions.

Do not imply that a device's logical topology reveals the exact physical location of a USB port.

Where the relationship cannot be established, show an unassigned or unknown parent state.

---

# 11. WORKSTREAM H — PCI AND MOTHERBOARD DEVICE RELATIONSHIPS

Create a logical PCI device explorer.

Display:

- PCI device identifier.
- Vendor ID.
- Device ID.
- Class.
- Subclass.
- Driver.
- PCI address.
- Parent bridge relationships where available.
- Connected hardware devices.

Provide a logical hierarchy that helps users understand how devices connect through PCI bridges and controllers.

Important:

This is NOT a physical motherboard diagram.

Do not draw actual motherboard component positions.

Do not invent electrical traces or board connections.

Do not assume that a PCI device's position in a software hierarchy corresponds to its physical location on the motherboard.

Phase 8 will handle verified motherboard visualisation.

---

# 12. WORKSTREAM I — INTERACTIVE COMPONENT INSPECTOR

Build a reusable component inspection panel.

When a user selects any component, show a detailed information panel.

The panel should adapt to the component category.

Examples:

### Selecting a CPU core

Show:
- Core ID.
- Logical processors.
- Utilisation.
- Frequency.
- Topology relationships.

### Selecting a RAM module

Show:
- Module capacity.
- Manufacturer.
- Memory type.
- Speed.
- Channel information.
- Detection source.

### Selecting a GPU

Show:
- Model.
- Driver.
- VRAM.
- Utilisation.
- Temperature.
- Supported capabilities.

### Selecting a storage device

Show:
- Model.
- Capacity.
- Interface.
- Device hierarchy.
- Health information where available.

### Selecting a USB device

Show:
- Vendor ID.
- Product ID.
- Device class.
- Parent hub.
- Driver information.

The panel must include:

- Component title.
- Category.
- Available specifications.
- Live information where supported.
- Source information.
- Last updated timestamp.
- Unsupported or unavailable field indicators.

Do not show empty fields as fabricated defaults.

Use consistent component detail layouts.

---

# 13. WORKSTREAM J — HARDWARE DATA RELIABILITY AND SOURCE INDICATORS

This is a fundamental Sentinel requirement.

Every important hardware property must have traceable provenance.

Where practical, represent:

- Value.
- Source.
- Collection timestamp.
- Availability.
- Confidence or reliability classification.
- Whether the value is directly detected, derived or inferred.

Use a clear reliability vocabulary.

Suggested categories:

1. Directly detected.
2. Firmware reported.
3. Operating-system reported.
4. Derived from detected measurements.
5. Inferred.
6. Unavailable.
7. Unsupported.

Do not confuse confidence with measurement accuracy.

For example:

A CPU core count reported by Linux is operating-system-reported information.

A battery health percentage calculated from design capacity and full-charge capacity is a derived estimate.

A physical RAM slot layout inferred from incomplete firmware data must never be presented as directly detected.

Create reusable UI indicators for these distinctions.

The user should be able to understand where a displayed value came from.

---

# 14. WORKSTREAM K — VISUAL DESIGN AND USER EXPERIENCE

The Hardware Explorer should feel like a professional hardware engineering tool.

Maintain the existing Sentinel design language.

Do not redesign the entire application.

## Design direction

Use:

- Dark technical interface.
- Clear component hierarchy.
- Restrained accent colours.
- Precise typography.
- Clean technical diagrams.
- Subtle interactions.
- Consistent spacing.
- Readable technical information.
- Responsive layouts.

Avoid:

- Excessive gradients.
- Unnecessary animations.
- Decorative 3D hardware illustrations that communicate no actual information.
- Overcrowded specification cards.
- Fake hardware diagrams.
- Excessive glowing effects.
- Unreadable technical labels.

## Visual hierarchy

The user should be able to distinguish:

1. Hardware overview.
2. Component category.
3. Selected component.
4. Technical architecture.
5. Live telemetry.
6. Detailed specifications.
7. Source and reliability.

## Interaction

Support:

- Component selection.
- Hover information where appropriate.
- Expandable hierarchies.
- Zooming and panning for large logical topology diagrams where useful.
- Clear selection states.
- Keyboard accessibility.
- Responsive layouts.

Do not introduce unnecessary complex visualisation libraries.

Prefer lightweight SVG, CSS and React-based visualisation where practical.

Use existing dependencies before introducing new ones.

---

# 15. WORKSTREAM L — LIVE TELEMETRY INTEGRATION

Integrate the visualisations with existing Phase 3 monitoring.

Examples:

- CPU core diagrams update with per-core utilisation.
- Memory overview updates with RAM usage.
- GPU information updates with available GPU telemetry.
- Storage information updates with existing storage measurements.
- Network explorer updates with network activity.
- Battery view updates with battery state.

Important:

Do not create independent polling systems for every component.

Use the existing monitoring architecture.

Requirements:

- Efficient subscriptions.
- Stable component identifiers.
- Controlled rendering frequency.
- No unnecessary re-rendering.
- No duplicate telemetry collection.
- Correct stale-data handling.
- Clear unavailable-data states.

Hardware discovery and live telemetry must remain separate concerns.

Hardware topology should not be repeatedly rediscovered just because telemetry updates.

---

# 16. PERFORMANCE REQUIREMENTS

Sentinel must remain lightweight.

The primary target includes ordinary laptops with:

- 8 GB RAM.
- Integrated graphics.
- CPU-only hardware.
- No dedicated GPU.

Requirements:

- Avoid heavyweight 3D rendering engines.
- Avoid unnecessary background polling.
- Avoid generating large SVG trees unnecessarily.
- Avoid repeated hardware discovery.
- Use efficient React rendering.
- Use stable keys and component identifiers.
- Keep visualisation data structures reasonably compact.
- Avoid memory leaks from subscriptions.
- Avoid excessive animation.

Measure the actual resource impact.

Do not claim performance improvements without measurements.

---

# 17. TESTING REQUIREMENTS

Phase 5 requires proper automated and integration testing.

## A. Rust tests

Test:

- Hardware model normalisation.
- CPU topology extraction.
- Cache hierarchy relationships.
- Memory module extraction.
- GPU capability detection.
- Storage hierarchy construction.
- Battery data normalisation.
- USB hierarchy construction.
- PCI hierarchy construction.
- Source metadata.
- Missing information handling.
- Unsupported hardware behaviour.

## B. Frontend tests

Test:

- Hardware overview.
- Component selection.
- Component detail panels.
- CPU topology rendering.
- Cache hierarchy rendering.
- Memory visualisation.
- GPU visualisation.
- Storage hierarchy.
- Battery display.
- USB hierarchy.
- PCI hierarchy.
- Live telemetry updates.
- Missing-data states.
- Reliability indicators.
- Navigation and selection persistence.

## C. Integration tests

Verify:

- Rust hardware models reach the frontend correctly.
- Existing IPC contracts remain consistent.
- Component IDs remain stable.
- Telemetry maps to the correct component.
- Hardware discovery is not unnecessarily repeated.
- UI subscriptions are cleaned up correctly.

## D. Regression tests

Run all existing tests from Phases 1–4.

Do not delete or weaken existing tests.

Record actual results.

---

# 18. STRICT SAFETY REQUIREMENTS

Sentinel remains a read-only system intelligence application.

NEVER introduce functionality that:

- Changes hardware configurations.
- Modifies CPU settings.
- Modifies GPU settings.
- Changes memory settings.
- Changes storage partitions.
- Modifies drivers.
- Installs or removes packages.
- Modifies systemd services.
- Changes network configurations.
- Changes BIOS or UEFI settings.
- Applies automatic fixes.
- Executes maintenance operations.

Hardware Explorer is for inspection and understanding only.

All discovery operations must use appropriate read-only interfaces.

Do not introduce external data uploads.

Local hardware information remains local.

---

# 19. IMPLEMENTATION SEQUENCE

Follow this order.

### STAGE 1 — Architecture and repository audit

- Read original Sentinel documentation.
- Inspect existing discovery modules.
- Inspect existing monitoring models.
- Inspect IPC contracts.
- Inspect existing frontend design.
- Identify reusable code.
- Produce an implementation plan.

### STAGE 2 — Hardware data model

- Normalise existing hardware structures.
- Add missing topology relationships.
- Add provenance metadata.
- Introduce stable component IDs where needed.
- Maintain compatibility with existing modules.

### STAGE 3 — Hardware Explorer foundation

- Create the explorer experience.
- Integrate navigation.
- Build the component overview.
- Implement selection.
- Implement reusable component detail panels.

### STAGE 4 — CPU and memory visualisation

- CPU topology.
- Core and thread relationships.
- Cache hierarchy.
- Memory modules.
- Memory topology.

### STAGE 5 — GPU and storage visualisation

- GPU overview.
- GPU capability presentation.
- Storage device hierarchy.
- Logical block device relationships.

### STAGE 6 — Battery, network and USB visualisation

- Battery explorer.
- Network adapter explorer.
- USB device hierarchy.

### STAGE 7 — PCI explorer

- PCI device hierarchy.
- Parent-child relationships.
- Logical hardware connections.

### STAGE 8 — Live telemetry integration

- Integrate existing collectors.
- Add efficient subscriptions.
- Add stale-data handling.
- Validate component-to-telemetry mapping.

### STAGE 9 — Reliability and UI refinement

- Handle missing data.
- Improve selection behaviour.
- Optimise rendering.
- Validate accessibility.
- Test low-spec hardware.

### STAGE 10 — Final testing and documentation

- Full Rust test suite.
- Full frontend test suite.
- Integration testing.
- Regression testing.
- Documentation.
- Phase completion report.

Do not skip stages.

---

# 20. DEFINITION OF DONE

Phase 5 is complete only when the following applicable requirements are satisfied.

## Hardware Explorer

- [ ] Dedicated Hardware Explorer experience exists.
- [ ] Existing navigation remains functional.
- [ ] Hardware categories are displayed correctly.
- [ ] Component selection works.
- [ ] Detailed information panels work.

## CPU

- [ ] CPU identification works.
- [ ] Core topology is represented.
- [ ] Logical processors are represented.
- [ ] Cache hierarchy is represented.
- [ ] Per-core telemetry is integrated.
- [ ] Unsupported topology details are identified honestly.

## Memory

- [ ] Installed memory information is displayed.
- [ ] Memory module information is displayed where available.
- [ ] Channel information is displayed where available.
- [ ] Physical slot information is not fabricated.
- [ ] Live memory information is integrated.

## GPU

- [ ] GPU identification works.
- [ ] Supported capabilities are displayed.
- [ ] Integrated and dedicated GPU memory are distinguished.
- [ ] Unavailable metrics are handled correctly.

## Storage

- [ ] Physical devices are represented.
- [ ] Logical device relationships are represented.
- [ ] Partition relationships are represented.
- [ ] Filesystem information is handled correctly.

## Other components

- [ ] Battery explorer works.
- [ ] Network explorer works.
- [ ] USB hierarchy works.
- [ ] PCI hierarchy works.

## Reliability

- [ ] Hardware data has provenance metadata.
- [ ] Component IDs remain stable.
- [ ] Live telemetry updates correctly.
- [ ] No duplicate monitoring collectors.
- [ ] Missing data is handled honestly.
- [ ] No critical rendering performance issues.

## Regression and safety

- [ ] All existing Phase 1–4 functionality remains operational.
- [ ] Existing Rust tests pass.
- [ ] Existing frontend tests pass.
- [ ] New tests pass.
- [ ] Application builds successfully.
- [ ] No operating-system modifications are introduced.
- [ ] No automatic repair functionality is introduced.

A mocked hardware configuration is not equivalent to physical hardware validation.

Clearly report what has been tested on real hardware.

---

# 21. DOCUMENTATION

Create or update the existing documentation structure.

Required documentation:

1. Hardware Explorer architecture.
2. Normalised hardware data model.
3. CPU topology representation.
4. Memory topology representation.
5. GPU capability model.
6. Storage hierarchy model.
7. USB and PCI hierarchy model.
8. Hardware data provenance system.
9. Frontend visualisation architecture.
10. Testing and hardware compatibility report.

Do not create redundant documents if equivalent documentation already exists.

---

# 22. FINAL HANDOVER REPORT

At completion, provide:

### A. Executive summary
What Phase 5 achieved.

### B. Architecture changes
Explain the hardware data model and visualisation architecture.

### C. Implemented visualisations
List each hardware category implemented.

### D. Live telemetry integration
Explain how existing collectors feed the visualisations.

### E. Hardware validation
List actual devices tested and distinguish physical from simulated validation.

### F. UI and UX changes
Describe the Hardware Explorer experience.

### G. Performance measurements
Report actual CPU, RAM and rendering impact.

### H. Test results
Provide exact test counts and results.

### I. Safety verification
Confirm that the application remains read-only.

### J. Outstanding limitations
Identify unavailable hardware information and unfinished capabilities.

### K. Phase completion status

Choose:

- COMPLETE.
- PARTIALLY COMPLETE.
- BLOCKED.

Do not declare completion based solely on successful compilation.

### L. Next phase

Explain how Phase 5 prepares the application for:

**PHASE 6 — AUTOMATIC SCHEMATIC DISCOVERY & VERIFICATION.**

Do not begin Phase 6.

---

# FINAL INSTRUCTION

Start by reading the original Sentinel concept documentation and inspecting the existing repository.

Understand the existing implementation before modifying anything.

Build a technically accurate, interactive hardware explorer using the actual information Sentinel can discover.

Prioritise accuracy, explainability, performance and maintainability over visual spectacle.

Remember:

**Sentinel must show the hardware that actually exists, not the hardware it assumes exists.**

The long-term motherboard intelligence system depends on getting this foundation right.

#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================

# SENTINEL — PHASE 6: AUTOMATIC SCHEMATIC DISCOVERY & VERIFICATION

## ROLE

Act as a senior Linux systems engineer, hardware reverse-engineering researcher, Rust backend architect, technical-document discovery engineer, and security-conscious software developer.

You are working on **Sentinel**, a Linux-first desktop system intelligence and hardware documentation platform.

Your task is to implement **Phase 6 — Automatic Schematic Discovery & Verification**.

This is an engineering task. Inspect the existing project, understand its architecture, and implement the phase without unnecessarily replacing working code.

---

# 1. FIRST: UNDERSTAND THE EXISTING PROJECT

Before writing code:

1. Locate and read the original Sentinel idea/concept documentation available in this project folder.
2. Read the master roadmap and identify the exact scope of Phase 6.
3. Inspect the existing repository structure and architecture.
4. Review the implementation of:
   - Phase 1: Project Foundation.
   - Phase 2: Hardware and Software Discovery.
   - Phase 3: Live Monitoring, Alerts and History.
   - Phase 4: Desktop Integration and Reliability.
   - Phase 5: Hardware Explorer and Component Visualisation.
5. Identify the actual motherboard identification data currently exposed by Sentinel.
6. Understand how hardware metadata, source provenance, confidence, and reliability are represented.
7. Review existing networking, caching, configuration, logging, and error-handling infrastructure.
8. Inspect existing tests and documentation.

Do not assume a phase is complete simply because its code or prompt exists.

### Dependency gate

Phase 6 depends particularly on:

- Reliable motherboard manufacturer identification.
- Motherboard model and product identifiers.
- Revision information where detectable.
- Hardware source and reliability metadata.
- A stable hardware identity model from Phase 5.

If these are incomplete, document the missing prerequisites and implement only the portions that can be supported safely.

Do not invent hardware identifiers to make the discovery system appear functional.

### Before implementation

Produce a short audit containing:

- Existing motherboard identification capabilities.
- Available hardware identity fields.
- Missing or unreliable identification fields.
- Existing document/network infrastructure that can be reused.
- Architectural changes needed.
- Phase 6 implementation plan.

Then proceed with implementation in controlled stages.

---

# 2. PHASE 6 OBJECTIVE

Build an automatic technical-document discovery and verification system.

When Sentinel detects a motherboard, it should attempt to discover legitimate technical documents associated with that exact hardware.

The system should search for relevant documents such as:

- Motherboard schematics.
- Service manuals.
- Hardware maintenance manuals.
- Boardview files.
- Manufacturer technical documentation.
- Official motherboard specifications.
- Hardware revision documentation.
- Other legitimate repair and diagnostic documentation.

The system must determine how closely each discovered document matches the detected motherboard.

It must distinguish between:

1. Exact motherboard model and revision match.
2. Exact model but revision unknown.
3. Related motherboard model.
4. Similar motherboard family.
5. Unverified or ambiguous document.
6. Unrelated document.

**Finding a document is not the same as verifying that it belongs to the detected motherboard.**

The system must never automatically label a document as an exact match merely because its filename contains a similar model number.

---

# 3. MOTHERBOARD IDENTITY AND SEARCH INTELLIGENCE

Build on the hardware identity data already collected by Sentinel.

Potential identification fields include:

- Manufacturer.
- Product name.
- Product version.
- Board name.
- Board vendor.
- Board version.
- System manufacturer.
- System product name.
- System version.
- BIOS vendor.
- BIOS version.
- BIOS release date.
- DMI/SMBIOS identifiers.
- Relevant PCI identifiers.
- Hardware family identifiers.

Use existing discovery modules wherever possible.

On Linux, investigate appropriate read-only sources, such as:

- `/sys/class/dmi/id/`
- `/sys/devices/`
- `/proc/`
- Existing PCI discovery data.
- Existing firmware information.

Respect permissions. If a field cannot be accessed, mark it unavailable rather than attempting privileged access.

### Identity normalization

Implement a normalization layer that can:

- Remove irrelevant whitespace.
- Normalize case.
- Handle punctuation differences.
- Identify common manufacturer naming variations.
- Separate model identifiers from marketing names.
- Preserve the original detected value.
- Avoid merging two distinct motherboard models simply because their names look similar.

Maintain both:

- Original hardware identity.
- Normalized search identity.

Never discard the original detected identifiers.

### Search query generation

Generate multiple targeted search queries based on available information.

Examples:

- Exact motherboard model + schematic.
- Exact motherboard model + boardview.
- Manufacturer + model + service manual.
- Manufacturer + model + hardware revision.
- Board identifier + schematic.
- Exact model + official technical documentation.

Search queries must become more conservative when the detected identity is incomplete.

Do not generate broad searches that could easily associate documents from unrelated hardware.

---

# 4. LEGITIMATE DOCUMENT DISCOVERY

Implement a modular document-discovery architecture.

Do not tightly couple the application to one search provider or one website.

### Source categories

Prioritize sources in this order:

**Category A — Manufacturer sources**
- Official manufacturer websites.
- Manufacturer support portals.
- Official product documentation.
- Official service and maintenance manuals.
- Official technical bulletins.

**Category B — Authorized or legitimate technical sources**
- Authorized repair documentation providers.
- Legitimate technical-document repositories.
- Publicly accessible documentation with clear source attribution.
- Other sources whose access and usage terms permit discovery and retrieval.

**Category C — Community-maintained technical resources**
- Public technical communities.
- Community-maintained repair documentation.
- Public repositories containing documentation that is legitimately shared.
- Other sources with verifiable provenance and permitted access.

### Important legal and access restrictions

- Do not bypass authentication.
- Do not circumvent paywalls.
- Do not bypass access controls.
- Do not scrape websites in violation of their terms.
- Respect robots.txt and applicable website policies.
- Respect rate limits.
- Do not download pirated or unauthorized documents.
- Do not use stolen credentials or private repositories.
- Do not attempt to access restricted manufacturer portals.
- Do not treat a publicly accessible URL as automatic permission to redistribute its content.

When a document is discoverable but cannot legally or technically be retrieved, store its public reference and explain why it was not downloaded.

### Discovery architecture

Create an extensible source-provider interface.

Each provider should independently handle:

- Search query submission.
- Result retrieval.
- Source identification.
- Metadata extraction.
- URL validation.
- Document-type identification.
- Rate limiting.
- Error reporting.

The system should continue functioning when one provider is unavailable.

Do not make the entire discovery engine depend on a single external service.

### Search behavior

- Avoid excessive repeated requests.
- Cache search results.
- Avoid searching repeatedly for the same hardware identity.
- Allow manual rediscovery.
- Support cancellation.
- Provide clear search progress.
- Handle timeouts and unavailable sources.
- Never block the main UI while searching.

Online search must be optional and configurable.

Sentinel should remain useful when the machine is offline.

---

# 5. DOCUMENT TYPE IDENTIFICATION

Implement document classification.

Supported document categories should include:

| Document type | Description |
|---|---|
| Schematic | Electrical circuit diagram |
| Boardview | Board layout and component-location data |
| Service manual | Repair and maintenance instructions |
| Hardware manual | Technical hardware information |
| Specification sheet | Product specifications |
| Firmware documentation | BIOS/firmware-related documentation |
| Unknown | Unclassified document |

Identify likely formats, including:

- PDF.
- ZIP archives containing permitted documentation.
- Boardview formats, where their extensions can be identified.
- Other recognized technical-document formats.

At this stage, classify files and extract basic metadata only.

Do not implement schematic parsing, OCR-based circuit extraction, component extraction, or boardview coordinate processing. Those belong to Phase 7.

### File identification

For discovered documents, capture:

- Filename.
- Extension.
- MIME type, where available.
- Document category.
- File size, where known.
- Source URL.
- Original page URL.
- Discovery timestamp.
- Retrieval status.
- Basic document metadata, where available.

Do not trust filenames or HTTP Content-Type headers alone.

Use safe file-type validation where a document is downloaded.

---

# 6. DOCUMENT MATCHING AND VERIFICATION ENGINE

This is one of the most important parts of Phase 6.

Implement a matching engine that compares discovered documents with Sentinel's detected motherboard identity.

### Matching signals

Consider:

- Manufacturer match.
- Exact model match.
- Board identifier match.
- Hardware revision match.
- Product family match.
- System model match.
- Document title.
- Filename.
- Document metadata.
- Source website.
- Official manufacturer references.
- Other reliable identification evidence.

Not every field will be available.

Missing information must not be treated as a positive match.

### Match classifications

Implement explicit classifications:

**1. Exact match**
- Strong evidence that the document belongs to the detected motherboard.
- Model identifier matches.
- Revision matches when revision information is available.
- No contradictory identification evidence.

**2. High-confidence model match**
- Exact model match.
- Revision is unavailable or not specified.
- No conflicting evidence.

**3. Related model**
- Same product family or closely related board.
- Exact identity has not been established.

**4. Possible match**
- Some identifying fields match.
- Evidence is insufficient for a stronger classification.

**5. Unverified**
- Document exists, but there is insufficient evidence to establish its hardware identity.

**6. Mismatch**
- Available evidence indicates that the document belongs to different hardware.

### Confidence scoring

Build a transparent confidence model.

Requirements:

- Confidence must be derived from identifiable evidence.
- Each contributing signal must be inspectable.
- Contradictory evidence must reduce confidence.
- Missing fields must not increase confidence.
- A high numerical score must not override a critical identity mismatch.
- Manufacturer source authority and hardware identity match must be represented separately.

Do not create arbitrary percentages that have no meaningful relationship to the evidence.

Document the scoring methodology.

A document from an official manufacturer website is not automatically an exact match. Source trust and hardware identity are separate dimensions.

### Verification output

Every candidate should explain:

- What matched.
- What did not match.
- What information was missing.
- Why it received its classification.
- Whether revision compatibility is known.
- Whether the source is trustworthy.
- Whether manual verification is recommended.

The user should be able to understand why Sentinel considers a document relevant.

---

# 7. SOURCE TRUST AND PROVENANCE

Implement a source provenance model.

Each source should include, where available:

- Source name.
- Domain.
- Original page URL.
- Document URL.
- Source category.
- Official/authorized/community/unverified classification.
- Discovery timestamp.
- Last verification timestamp.
- Retrieval status.
- Source accessibility status.
- Relevant usage/access notes.

Keep source trust separate from hardware-match confidence.

For example:

A document can be:
- From an official source but for the wrong motherboard.
- From a community source but strongly matched to the correct board.
- From an unknown source with an uncertain hardware identity.

These distinctions must remain visible.

Never imply that a document is manufacturer-approved unless the evidence supports that statement.

---

# 8. SAFE DOWNLOAD AND LOCAL DOCUMENT STORAGE

Implement controlled local document storage.

### Storage requirements

- Store permitted downloaded documents inside Sentinel's designated application data directory.
- Keep document metadata in the existing persistence architecture where appropriate.
- Use unique internal identifiers.
- Preserve the original filename as metadata.
- Avoid duplicate downloads.
- Track file hashes where appropriate.
- Verify downloaded file integrity.
- Track document availability.
- Allow the user to remove cached documents through Sentinel's own application interface.

### Download security

Treat all remote documents as untrusted input.

Implement protections against:

- Path traversal.
- Malicious filenames.
- Unsafe archive extraction.
- Oversized downloads.
- Unexpected content types.
- Redirect abuse.
- Server-side request forgery (SSRF).
- Local file overwrite.
- Malformed URLs.
- Unexpected executable content.

Do not automatically execute downloaded files.

Do not automatically extract archives unless the extraction is explicitly implemented with strict path and size validation.

Do not open downloaded documents automatically.

Downloading must be subject to the user's configured permissions and the source's access conditions.

### Privacy

- Do not upload private system information.
- Do not transmit serial numbers or unique device identifiers unnecessarily.
- Do not send raw hardware inventory to external services.
- Generate search queries using only the minimum necessary model information.
- Make online discovery transparent to the user.
- Provide an offline mode.

If a unique hardware identifier is not needed for discovery, do not transmit it.

---

# 9. DOCUMENT DISCOVERY DATABASE AND CACHE

Integrate with Sentinel's existing persistence architecture.

Do not introduce a second database without a clear architectural reason.

Design appropriate records for:

### Hardware search identity
- Hardware identity reference.
- Original manufacturer/model.
- Normalized manufacturer/model.
- Board identifier.
- Revision.
- Search identity hash.
- Last search timestamp.

### Discovery session
- Session ID.
- Hardware identity.
- Search queries used.
- Providers queried.
- Start and completion timestamps.
- Session status.
- Error information.

### Document candidate
- Document ID.
- Title.
- Filename.
- Document type.
- Source reference.
- Document URL.
- Local cache path, if downloaded.
- File hash, if downloaded.
- Source trust classification.
- Hardware-match classification.
- Confidence/evidence data.
- Revision compatibility.
- Discovery timestamp.
- Last verification timestamp.

### Cache behavior

- Prevent duplicate candidates.
- Avoid repeated searches for unchanged hardware identities.
- Support cache expiry.
- Support manual refresh.
- Preserve useful discovery history.
- Handle interrupted sessions.
- Handle application restarts.
- Implement safe database migrations.

Do not store large binary documents directly inside the database unless the existing architecture has a compelling reason.

---

# 10. FRONTEND — SCHEMATIC DISCOVERY EXPERIENCE

Integrate the feature into Sentinel's existing design system.

Do not redesign the entire application.

Add a dedicated documentation discovery experience associated with the detected motherboard in Hardware Explorer.

### A. Motherboard documentation overview

Display:

- Detected motherboard manufacturer.
- Detected model.
- Board identifier, if available.
- Hardware revision, if available.
- Identification reliability.
- Last discovery time.
- Discovery status.

Clearly show when motherboard identification is incomplete.

### B. Discovery controls

Provide:

- Search for documentation.
- Cancel active discovery.
- Refresh search results.
- View previous discovery sessions.
- Offline-mode status.
- Clear discovery cache, where appropriate.

Avoid unnecessary controls.

### C. Search progress

Display meaningful stages:

1. Preparing motherboard identity.
2. Generating search queries.
3. Searching configured providers.
4. Collecting document candidates.
5. Identifying document types.
6. Verifying hardware matches.
7. Saving discovery results.

Do not fabricate progress percentages.

### D. Document results

Each candidate should show:

- Document title.
- Document category.
- Source name.
- Source trust classification.
- Hardware-match classification.
- Match confidence, where meaningful.
- Revision compatibility.
- Retrieval/download status.
- Last verification time.

Use clear visual differences between:

- Exact match.
- High-confidence match.
- Related model.
- Possible match.
- Unverified.
- Mismatch.

Do not make an unverified document look equivalent to an exact match.

### E. Document detail panel

When selecting a candidate, show:

- Full document metadata.
- Source information.
- Original source page.
- Document URL.
- Matching evidence.
- Contradictory evidence.
- Missing information.
- Revision information.
- Retrieval status.
- Local file availability.
- Verification explanation.

Allow the user to open the original source page through a safe external-link mechanism.

Allow opening a locally cached document only after validating its stored path and file type.

### F. Empty and failure states

Design useful states for:

- No motherboard detected.
- Incomplete motherboard identity.
- No documents found.
- No providers configured.
- Offline system.
- Provider unavailable.
- Search cancelled.
- No exact matches.
- Only related documents found.
- Download unavailable.
- Verification incomplete.

Never show fake documents or placeholder results as actual discoveries.

---

# 11. ARCHITECTURE AND CODE QUALITY

Maintain clear separation between:

1. Hardware identity.
2. Search-query generation.
3. Source-provider integrations.
4. Document candidate collection.
5. Document type identification.
6. Hardware-match verification.
7. Source provenance.
8. Download management.
9. Persistence and caching.
10. Frontend presentation.

### Rust backend

Use the existing Rust architecture and conventions.

Prefer:

- Strongly typed models.
- Explicit error types.
- Async network operations.
- Cancellation support.
- Bounded concurrency.
- Configurable timeouts.
- Rate limiting.
- Structured logging.
- Unit-testable matching logic.

Avoid putting network discovery logic directly into Tauri UI commands.

### Frontend

Use existing React and TypeScript conventions.

- Keep components modular.
- Reuse existing design tokens.
- Avoid duplicate state.
- Handle loading, cancellation and error states.
- Do not perform external searches directly from React.
- Do not expose API secrets or provider credentials to the frontend.

### Configuration

Add configuration for:

- Online discovery enabled/disabled.
- Configured source providers.
- Request timeouts.
- Rate limits.
- Cache duration.
- Maximum permitted download size.
- Automatic download behavior, defaulting to disabled.
- Local document storage location, following existing application conventions.

Never make automatic downloading the default behavior.

---

# 12. TESTING REQUIREMENTS

Testing is mandatory.

### A. Hardware identity tests

Test:

- Exact model identification.
- Missing manufacturer.
- Missing model.
- Missing revision.
- Naming variations.
- Conflicting identifiers.
- Incomplete DMI information.

### B. Search generation tests

Test:

- Exact model queries.
- Manufacturer and model combinations.
- Missing revision.
- Incomplete hardware identity.
- Duplicate query prevention.
- Query sanitization.

### C. Matching engine tests

Test:

- Exact model and revision match.
- Exact model with unknown revision.
- Same family but different model.
- Same manufacturer but unrelated model.
- Conflicting revision.
- Missing model identifiers.
- Misleading filenames.
- Misleading document titles.
- Official source but wrong hardware.
- Community source with strong model evidence.
- Unverified documents.

### D. Source provider tests

Use mocked providers.

Test:

- Successful search.
- Empty results.
- Timeout.
- HTTP errors.
- Rate limiting.
- Invalid response.
- Provider unavailable.
- Cancellation.
- Duplicate results.

Do not make routine unit tests depend on external websites.

### E. Download security tests

Test:

- Invalid URLs.
- Private and loopback network destinations.
- Redirects to prohibited destinations.
- Oversized files.
- Malformed documents.
- Unsafe filenames.
- Path traversal attempts.
- Invalid content types.
- Duplicate downloads.
- Interrupted downloads.
- Unsafe archive paths.

### F. Persistence tests

Test:

- Saving candidates.
- Updating verification results.
- Cache expiry.
- Duplicate prevention.
- Database migration.
- Application restart.
- Interrupted discovery recovery.

### G. Frontend tests

Test:

- Motherboard overview.
- Discovery progress.
- Cancellation.
- Document results.
- Match classifications.
- Detail panel.
- Empty states.
- Provider failures.
- Offline mode.

Do not weaken existing Phase 1–5 tests to make new tests pass.

---

# 13. PERFORMANCE AND RESOURCE REQUIREMENTS

Sentinel must remain usable on low-resource laptops.

Requirements:

- No continuous background web searching.
- No unnecessary repeated hardware discovery.
- No large in-memory document downloads.
- Bounded network concurrency.
- Configurable request timeouts.
- Efficient metadata caching.
- No heavy document-processing workloads in this phase.
- No GPU requirement.
- No unnecessary background polling.

The application must remain responsive while discovery is running.

Test on CPU-only hardware and systems with approximately 8 GB RAM.

---

# 14. STRICT SAFETY BOUNDARIES

Sentinel remains a read-only system intelligence application.

Phase 6 must not:

- Modify BIOS or firmware.
- Modify motherboard settings.
- Modify kernel parameters.
- Modify device configurations.
- Install drivers.
- Update firmware.
- Execute repair scripts.
- Execute downloaded files.
- Apply recommendations.
- Change system power settings.
- Modify operating-system services.
- Upload private system inventory.
- Access restricted documentation through unauthorized methods.

Document discovery is informational only.

No document should be interpreted as authorization to modify hardware or software.

---

# 15. IMPLEMENTATION STAGES

Implement Phase 6 in the following order.

## Stage 1 — Existing architecture audit
- Read original Sentinel documentation.
- Inspect repository and previous phases.
- Identify available motherboard identity fields.
- Identify blockers and missing prerequisites.
- Produce an implementation plan.

## Stage 2 — Hardware identity normalization
- Implement normalized motherboard identity.
- Preserve original hardware values.
- Add identity completeness and reliability information.
- Write tests.

## Stage 3 — Discovery architecture
- Create provider interfaces.
- Implement query generation.
- Add cancellation and bounded concurrency.
- Add provider configuration.

## Stage 4 — Source integrations
- Implement legitimate source discovery.
- Add source metadata and provenance.
- Add rate limiting and caching.
- Test provider failures.

## Stage 5 — Document classification
- Identify document types.
- Collect basic metadata.
- Validate candidate URLs and file information.
- Do not parse schematic contents.

## Stage 6 — Verification engine
- Implement hardware matching.
- Implement transparent confidence and evidence.
- Implement match classifications.
- Add contradictory-evidence handling.
- Test extensively.

## Stage 7 — Persistence and downloads
- Add database models and migrations.
- Implement candidate caching.
- Implement safe, opt-in downloading.
- Add integrity checks and security protections.

## Stage 8 — Frontend integration
- Add motherboard documentation overview.
- Add discovery controls and progress.
- Add document candidate list.
- Add document detail panel.
- Add empty and failure states.

## Stage 9 — Integration and reliability
- Integrate with Hardware Explorer.
- Test daemon and UI communication.
- Test offline operation.
- Test interrupted discovery and restarts.
- Verify low-resource performance.

## Stage 10 — Final validation and documentation
- Run all relevant tests.
- Run formatting and lint checks.
- Verify read-only guarantees.
- Document supported providers and limitations.
- Produce the Phase 6 completion report.

Do not start Phase 7 work.

---

# 16. DEFINITION OF DONE

Phase 6 is complete only when:

1. Sentinel can use its existing hardware discovery data to identify the motherboard.
2. Hardware identity normalization works without discarding original values.
3. Search queries are generated from available identity evidence.
4. At least one legitimate discovery provider is implemented and tested, with additional providers supported through the architecture.
5. Discovery results are stored with source provenance.
6. Document types are classified where possible.
7. Candidates are matched against motherboard identity.
8. Exact matches are distinguished from related and unverified documents.
9. Match explanations expose supporting and contradictory evidence.
10. Source trust and hardware-match confidence are separate.
11. Downloading is opt-in and securely implemented.
12. Discovery results are cached.
13. Users can initiate, monitor and cancel discovery.
14. Offline operation remains functional.
15. Database persistence and migrations work.
16. Existing application features remain functional.
17. All new and existing relevant tests pass.
18. No prohibited system modifications or unauthorized document access occur.
19. Documentation explains supported providers, limitations and verification behavior.

Compilation alone does not count as completion.

---

# 17. FINAL HANDOVER REPORT

At the end, provide a detailed report containing:

1. Original documentation and roadmap reviewed.
2. Existing architecture inspected.
3. Phase 5 dependencies and their actual status.
4. New modules and files created.
5. Existing modules modified.
6. Motherboard identity fields supported.
7. Discovery providers implemented.
8. Document formats recognized.
9. Verification and confidence methodology.
10. Source provenance implementation.
11. Download and storage security protections.
12. Database changes and migrations.
13. Frontend features added.
14. Tests executed and their results.
15. Performance observations.
16. Known limitations.
17. Unsupported manufacturers or document sources.
18. Remaining blockers.
19. Confirmation that read-only and privacy boundaries were preserved.
20. Confirmation that Phase 7 was not prematurely implemented.

Clearly distinguish:

- Fully implemented.
- Partially implemented.
- Not implemented.
- Blocked by missing hardware information.
- Blocked by external source availability.

Do not claim that schematic discovery works for every motherboard.

Be honest about coverage and limitations.

---

# FINAL INSTRUCTION

Build a trustworthy documentation discovery system, not a search-results scraper.

A document that looks relevant is not necessarily correct.

A document from an official source is not necessarily for the detected board.

A similar motherboard is not necessarily electrically compatible.

Sentinel must preserve these distinctions throughout the system.

**The objective is to find the right documentation for the actual detected hardware, establish how reliable that match is, and clearly communicate uncertainty to the user.**

Do not implement schematic interpretation, electrical connection extraction, OCR-based circuit understanding, or boardview coordinate parsing. Those are reserved for Phase 7.

#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================



#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================



#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================



#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================



#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================



#==========================================================================================================================================================
#==========================================================================================================================================================
#==========================================================================================================================================================


