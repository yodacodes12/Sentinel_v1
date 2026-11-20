# Utility and service discovery

Code: `crates/sentinel-utilities`. Read-only, like hardware discovery. It builds a catalogue of
system utilities already installed and a read-only list of services. It never installs, removes,
updates or runs anything it finds, and never recommends installation commands.

## Sources

| Module | What is read | How |
|---|---|---|
| Desktop applications | `.desktop` files in `$XDG_DATA_HOME/applications` and each `$XDG_DATA_DIRS/*/applications` (up to 3 levels deep) | Parsed for name, comment, categories, icon, `Terminal`, `NoDisplay`, `Hidden`, `TryExec`. **`Exec` is never parsed or stored.** Earlier directories win; `Hidden=true` hides an ID everywhere (XDG rules) |
| Package databases | dpkg `/var/lib/dpkg/status`; pacman `/var/lib/pacman/local/`; RPM `rpmdb.sqlite`; Flatpak `/var/lib/flatpak/app` and `~/.local/share/flatpak/app`; Snap `/snap/*/current/meta/snap.yaml` | dpkg is streamed stanza by stanza; only packages in the catalogue are kept. RPM is opened read-only and `immutable=1` (no lock or journal created); only names are read. No package-manager program is run |
| Executables on `PATH` | The catalogue's executable names in each absolute `PATH` directory and the standard bin directories | `stat` only: a regular file with an execute bit. Never executed (verified by `utility_discovery_never_runs_what_it_finds`) |
| System services | `systemctl list-units --type=service --all --output=json` and `list-unit-files`, system and `--user` | The only programs Sentinel's discovery runs. Arguments are fixed in `services/query.rs` and pinned by a safety test; environment cleared; 5 second limit, after which the query is abandoned (never killed). Unit-file locations come from the standard unit directories |

Services are only queried on the real machine; with a test root the module reports "unavailable".

## Catalogue

`catalogue.rs` lists 64 well-known tools in fifteen categories (system monitoring,
hardware information, disk management, storage health, battery and power, sensors, network
management, network diagnostics, process management, system logs, service management, package
management, graphics and drivers, backup and recovery, system configuration) with their executable
names, desktop-file IDs and package names. It is data used for matching; nothing in it is run.

A known utility is **installed** when it was found by at least one source; otherwise it is listed as
"not installed" with source `catalogue`. Desktop entries that are not in the catalogue are added when
their freedesktop categories mark them as system tools (`System`, `Settings`, `Monitor`,
`HardwareSettings`, `PackageManager`, …), excluding terminal emulators, file managers, development
tools and games.

Each `SystemUtility` records: id, name, description, category, interface (graphical, terminal,
command line), installed, version (from the package database), executable path, desktop entry
reference, package, icon, the sources that found it, and whether it can be opened.

`RELEVANT_SERVICES` marks services related to networking, power, storage, sensors, logging, packages
and graphics; the UI and CLI show those by default.

## Opening a utility

Only on an explicit user action (`sentinel utilities open <id>` or the Open button):

1. The ID is validated (letters, digits, `.-_:+`, at most 160 characters).
2. It is looked up in the catalogue the daemon discovered (or the cache when it is stopped).
3. It must be installed and have a desktop entry. Command-line tools are never run; the user is
   told to run them in a terminal.
4. The desktop file's path must be absolute, contain no `..`, end in `.desktop`, sit inside an
   `applications` directory, and still exist.
5. `gio launch <path>` (or `gtk-launch <id>` if `gio` is missing) opens it, exactly as the desktop's
   own menu would. Sentinel never interprets the `Exec` line itself.

The daemon never launches anything; the CLI or desktop process does, as the user.

## Distribution differences

- Debian/Ubuntu: dpkg (versions shown); Snap on Ubuntu.
- Fedora/openSUSE: `rpmdb.sqlite` (names only). Older Berkeley-DB RPM databases are reported as
  unsupported.
- Arch: pacman `local/` directory names give name and version.
- Flatpak and Snap applications appear through their exported desktop entries as well.
- Systems without systemd: services are reported unavailable.

## Tested

On the development machine (Ubuntu 26.04): 136 desktop files, dpkg (2,414 packages) and Snap (23)
read; 72 utilities listed (64 from the built-in catalogue plus 8 further system tools found
through desktop entries), 36 of them installed; 432 services listed (system and user). Pacman,
RPM and Flatpak adapters are tested only with synthetic fixtures.
