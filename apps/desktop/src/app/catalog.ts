// Static navigation and labels. Nothing here is a measurement: every
// hardware value the UI shows comes from the discovered inventory.

import type { IconName } from "../components/Icon";
import type { ModuleId } from "../types/inventory";

export type Accent = "cyan" | "blue" | "orange" | "purple" | "green";

export interface NavItem {
  id: string;
  label: string;
  icon: IconName;
  /** False for sections that are development placeholders in this release. */
  implemented: boolean;
  /** What the section will contain, shown on its placeholder page. */
  planned: string;
}

export const NAV_ITEMS: readonly NavItem[] = [
  { id: "overview", label: "Overview", icon: "overview", implemented: true, planned: "" },
  {
    id: "hardware",
    label: "Hardware",
    icon: "cpu",
    implemented: true,
    planned: "A full inventory of the processor, memory, storage, graphics and peripherals in this computer.",
  },
  {
    id: "motherboard",
    label: "Motherboard",
    icon: "board",
    implemented: true,
    planned: "Board identity now; an interactive view once schematic analysis exists.",
  },
  {
    id: "telemetry",
    label: "Telemetry",
    icon: "activity",
    implemented: true,
    planned: "Live and historical readings: load, memory pressure, temperatures, power and network.",
  },
  {
    id: "schematics",
    label: "Schematic Library",
    icon: "library",
    implemented: true,
    planned: "Schematics and board documentation located for the hardware Sentinel has identified.",
  },
  {
    id: "diagnostics",
    label: "Diagnostics",
    icon: "diagnostics",
    implemented: false,
    planned: "Read-only health analysis with evidence and recommendations. Sentinel will never apply a fix.",
  },
  {
    id: "utilities",
    label: "Installed Utilities",
    icon: "utilities",
    implemented: true,
    planned: "System tools already installed on this computer, with a way to open them.",
  },
  {
    id: "services",
    label: "Services and Startup",
    icon: "services",
    implemented: true,
    planned: "A read-only view of system services and what starts with the computer.",
  },
  {
    id: "alerts",
    label: "Alerts and Events",
    icon: "bell",
    implemented: true,
    planned: "A history of notable system events and the alerts Sentinel has raised.",
  },
  { id: "settings", label: "Settings", icon: "settings", implemented: true, planned: "" },
];

/** Display names of discovery modules (mirrors ModuleId::label in Rust). */
export const MODULE_LABEL: Record<ModuleId, string> = {
  system_identity: "System identity",
  firmware: "Firmware",
  cpu: "Processor",
  memory: "Memory",
  motherboard: "Motherboard",
  gpu: "Graphics",
  storage: "Storage",
  battery: "Battery and power",
  network: "Network",
  usb: "USB",
  pci: "PCI",
  sensors: "Sensors",
  desktop_entries: "Desktop applications",
  packages: "Package databases",
  executables: "Executables on PATH",
  services: "System services",
};

export const ACCENT_HEX: Record<Accent, string> = {
  cyan: "#22d3ee",
  blue: "#4c8dff",
  orange: "#ff9f43",
  purple: "#a78bfa",
  green: "#3ddc97",
};
