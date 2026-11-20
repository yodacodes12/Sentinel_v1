import { type Indicator } from "../app/viewModel";
import { windowControls } from "../services/bridge";
import { Icon, type IconName } from "./Icon";
import { StatusDot, Switch } from "./primitives";

export function Header({
  version,
  device,
  service,
  monitoring,
  serviceOn,
  serviceBusy,
  serviceControllable,
  onServiceToggle,
  monitoringOn,
  monitoringControllable,
  onMonitoringToggle,
}: {
  version: string | null;
  /** Detected identity, e.g. "Example Corp Book 14", and whether it is cached. */
  device: { name: string | null; cached: boolean } | null;
  service: Indicator;
  monitoring: Indicator;
  serviceOn: boolean;
  serviceBusy: "starting" | "stopping" | null;
  serviceControllable: boolean;
  onServiceToggle: (next: boolean) => void;
  monitoringOn?: boolean;
  monitoringControllable?: boolean;
  onMonitoringToggle?: (next: boolean) => void;
}) {
  return (
    <header className="flex h-12 shrink-0 items-stretch border-b border-line bg-panel">
      <div className="flex w-[220px] shrink-0 items-center gap-2.5 border-r border-line px-4" data-tauri-drag-region>
        <Icon name="logo" size={20} className="text-cyan" />
        <span className="font-mono text-sm font-semibold tracking-[0.28em] text-fg">SENTINEL</span>
        {version ? <span className="font-mono text-[10px] text-dim">v{version}</span> : null}
      </div>

      {/* Device identity, as firmware reported it in the last scan. */}
      <div className="flex min-w-0 items-center gap-3 px-4" data-tauri-drag-region>
        <span className="label">Device</span>
        <span
          className={`selectable truncate font-mono text-xs ${device?.name ? "text-fg" : "text-dim"}`}
          title={device?.name ? `From firmware (DMI)${device.cached ? ", cached scan" : ""}` : "Run a hardware scan to identify this computer."}
        >
          {device === null ? "Not scanned" : (device.name ?? "Not identified by firmware")}
        </span>
        {device?.cached ? <span className="label">cached</span> : null}
      </div>

      <div className="flex-1" data-tauri-drag-region />

      <div className="flex items-center gap-3 border-l border-line px-4" title={monitoring.detail}>
        <span className="label">Monitoring</span>
        {onMonitoringToggle ? (
          <Switch
            checked={!!monitoringOn}
            onChange={onMonitoringToggle}
            disabled={!monitoringControllable}
            label={monitoringOn ? "Turn monitoring off" : "Turn monitoring on"}
          />
        ) : null}
        <StatusDot tone={monitoring.tone} label={monitoring.label} />
      </div>

      <div className="flex items-center gap-3 border-l border-line px-4" title={service.detail}>
        <span className="label">Sentinel service</span>
        <Switch
          checked={serviceOn}
          onChange={onServiceToggle}
          disabled={!serviceControllable || serviceBusy !== null}
          label={serviceOn ? "Stop the Sentinel service" : "Start the Sentinel service"}
        />
        {/* role=status, not live: readable by screen readers, never announced on every refresh. */}
        <span role="status" aria-live="off" className={`w-16 font-mono text-xs ${serviceOn ? "text-cyan" : service.tone === "error" ? "text-red" : "text-muted"}`}>
          {serviceBusy === "starting" ? "STARTING" : serviceBusy === "stopping" ? "STOPPING" : service.tone === "error" ? "NO REPLY" : serviceOn ? "ON" : "OFF"}
        </span>
      </div>

      <div className="flex items-stretch border-l border-line">
        <WindowButton icon="minimize" label="Minimise" onClick={windowControls.minimize} />
        <WindowButton icon="maximize" label="Maximise" onClick={windowControls.toggleMaximize} />
        <WindowButton icon="close" label="Close window (Sentinel keeps running if it is on)" onClick={windowControls.close} danger />
      </div>
    </header>
  );
}

function WindowButton({
  icon,
  label,
  onClick,
  danger,
}: {
  icon: IconName;
  label: string;
  onClick: () => Promise<void>;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={() => void onClick()}
      className={`flex w-11 items-center justify-center text-muted hover:text-fg ${danger ? "hover:bg-red/80" : "hover:bg-raised"}`}
    >
      <Icon name={icon} size={14} />
    </button>
  );
}
