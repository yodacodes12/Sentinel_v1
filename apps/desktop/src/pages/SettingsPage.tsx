import { useEffect, useState } from "react";

import { Fact, Panel, Segmented, Switch } from "../components/primitives";
import type { AlertThresholdsConfig, ConfigPatch, ConfigView, Overview, RetentionConfig, Theme } from "../types/sentinel";
import type { DocumentSettings } from "../types/documents";

type NumericKey = { [K in keyof AlertThresholdsConfig]-?: AlertThresholdsConfig[K] extends number ? K : never }[keyof AlertThresholdsConfig];
type OptionalKey = "temperature_warning_c" | "battery_temperature_warning_c" | "battery_discharge_high_w" | "network_high_mbps";

const THRESHOLDS: readonly { key: NumericKey; label: string; unit: string }[] = [
  { key: "cpu_high_percent", label: "Sustained CPU usage", unit: "%" },
  { key: "cpu_high_duration_s", label: "…for at least", unit: "s" },
  { key: "memory_available_low_percent", label: "Available memory warning below", unit: "%" },
  { key: "memory_available_critical_percent", label: "Available memory critical below", unit: "%" },
  { key: "battery_low_percent", label: "Low battery", unit: "%" },
  { key: "battery_critical_percent", label: "Critical battery", unit: "%" },
  { key: "battery_health_warning_percent", label: "Battery health below", unit: "%" },
  { key: "storage_free_low_percent", label: "Free disk space warning below", unit: "%" },
  { key: "storage_free_critical_percent", label: "Free disk space critical below", unit: "%" },
  { key: "disk_busy_high_percent", label: "Disk busy for 5 minutes above", unit: "%" },
  { key: "sustain_s", label: "Conditions must last", unit: "s" },
  { key: "cooldown_s", label: "Do not re-notify within", unit: "s" },
];

const OPTIONAL: readonly { key: OptionalKey; label: string; unit: string }[] = [
  { key: "temperature_warning_c", label: "Temperature warning (all sensors)", unit: "°C" },
  { key: "battery_temperature_warning_c", label: "Battery temperature warning", unit: "°C" },
  { key: "battery_discharge_high_w", label: "Battery drain warning above", unit: "W" },
  { key: "network_high_mbps", label: "Network traffic above", unit: "Mbit/s" },
];

const THEMES: readonly { value: Theme; label: string }[] = [
  { value: "dark", label: "Dark" },
  { value: "black", label: "Black" },
];

export function SettingsPage({
  config,
  overview,
  saving,
  onChange,
}: {
  config: ConfigView | null;
  overview: Overview | null;
  saving: boolean;
  onChange: (patch: ConfigPatch) => void;
}) {
  if (config === null) {
    return (
      <Panel title="Settings" className="h-full">
        <p className="text-xs text-muted">Settings are unavailable because the desktop bridge is not connected.</p>
      </Panel>
    );
  }
  const { config: values } = config;

  return (
    <div className="grid max-w-5xl grid-cols-2 gap-3">
      <Panel title="Appearance">
        <Setting title="Theme" description="Dark uses charcoal panels. Black uses a pure black canvas.">
          <div className="w-44">
            <Segmented label="Theme" options={THEMES} value={values.theme} disabled={saving} onChange={(theme) => onChange({ theme })} />
          </div>
        </Setting>
      </Panel>

      <Panel title="Monitoring and notifications">
        <Setting title="Monitor at start-up" description="Whether monitoring resumes when the Sentinel service starts. Turning monitoring on or off elsewhere updates this too.">
          <Switch label="Monitor at start-up" checked={values.monitoring_enabled} disabled={saving} onChange={(monitoring_enabled) => onChange({ monitoring_enabled })} />
        </Setting>
        <Setting title="Desktop notifications" description="Notify you when an alert is raised. Alerts are recorded either way.">
          <Switch label="Desktop notifications" checked={values.notifications_enabled} disabled={saving} onChange={(notifications_enabled) => onChange({ notifications_enabled })} />
        </Setting>
        <Setting title="Power-aware sampling" description="Use the Eco profile automatically while running on battery. Shown on the dashboard whenever it applies.">
          <Switch label="Power-aware sampling" checked={values.power_aware_sampling} disabled={saving} onChange={(power_aware_sampling) => onChange({ power_aware_sampling })} />
        </Setting>
      </Panel>

      <ThresholdsPanel thresholds={values.alerts} saving={saving} onSave={(alerts) => onChange({ alerts })} />
      <RetentionPanel retention={values.retention} saving={saving} onSave={(retention) => onChange({ retention })} />
      <DocumentsPanel settings={values.documents ?? DOCUMENT_DEFAULTS} saving={saving} onSave={(documents) => onChange({ documents })} />

      <Panel title="Files" className="col-span-2">
        {config.warning ? <p className="mb-2 text-xs text-orange">{config.warning}</p> : null}
        <Fact label="Configuration file" value={config.path} />
        <Fact label="Settings read from" value={config.source === "daemon" ? "Running daemon" : "File on disk (daemon stopped)"} mono={false} />
        <Fact label="Configuration version" value={String(values.version)} />
        <Fact label="Log file" value={overview?.log_path ?? null} />
        <Fact label="IPC socket" value={overview?.socket_path ?? null} />
        <p className="mt-2 text-xs text-dim">
          Sentinel writes only to its own files, listed above and its local database. All data stays on this computer.
        </p>
      </Panel>
    </div>
  );
}

/**
 * An editable copy of `saved`. It is replaced only when the saved values
 * themselves change, not whenever the configuration is re-read (every few
 * seconds), so typing is never interrupted.
 */
function useDraft<T>(saved: T): [T, (next: T) => void, boolean] {
  const savedKey = JSON.stringify(saved);
  const [draft, setDraft] = useState<T>(saved);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => setDraft(saved), [savedKey]);
  return [draft, setDraft, JSON.stringify(draft) !== savedKey];
}

/** An empty or non-numeric field is NaN: shown empty, and blocks saving. */
function numberOf(text: string): number {
  return text.trim() === "" ? Number.NaN : Number(text);
}

function Setting({ title, description, children }: { title: string; description: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 border-b border-line py-2.5 last:border-b-0">
      <div className="min-w-0">
        <div className="text-[13px] text-fg">{title}</div>
        <div className="text-xs text-muted">{description}</div>
      </div>
      {children}
    </div>
  );
}

function ThresholdsPanel({ thresholds, saving, onSave }: { thresholds: AlertThresholdsConfig; saving: boolean; onSave: (t: AlertThresholdsConfig) => void }) {
  const [draft, setDraft, changed] = useDraft(thresholds);
  const incomplete = THRESHOLDS.some((t) => !Number.isFinite(draft[t.key]));
  return (
    <Panel title="Alert thresholds" className="col-span-2">
      <div className="grid grid-cols-2 gap-x-6">
        {THRESHOLDS.map((t) => (
          <label key={t.key} className="flex items-center justify-between border-b border-line py-1 text-xs">
            <span className="text-muted">{t.label}</span>
            <span className="flex items-center gap-1">
              <input type="number" value={Number.isFinite(draft[t.key]) ? draft[t.key] : ""} onChange={(e) => setDraft({ ...draft, [t.key]: numberOf(e.target.value) })} className="w-20 border border-line-strong bg-canvas px-1 py-0.5 text-right font-mono text-xs text-fg" />
              <span className="w-12 text-dim">{t.unit}</span>
            </span>
          </label>
        ))}
        {OPTIONAL.map((t) => (
          <label key={t.key} className="flex items-center justify-between border-b border-line py-1 text-xs">
            <span className="text-muted">{t.label}</span>
            <span className="flex items-center gap-1">
              <input
                type="number"
                placeholder="off"
                value={draft[t.key] ?? ""}
                onChange={(e) => setDraft({ ...draft, [t.key]: e.target.value.trim() === "" ? null : numberOf(e.target.value) })}
                className="w-20 border border-line-strong bg-canvas px-1 py-0.5 text-right font-mono text-xs text-fg placeholder:text-dim"
              />
              <span className="w-12 text-dim">{t.unit}</span>
            </span>
          </label>
        ))}
      </div>
      <div className="mt-2 flex items-center justify-between">
        <p className="text-[10px] text-dim">
          {incomplete
            ? "Every threshold above the optional ones needs a number before the change can be saved."
            : `Temperature warnings use limits reported by the hardware driver unless you set your own; Sentinel invents no universal "safe" temperature. Empty optional fields are off. The service rejects values out of range (percentages 0–100, critical levels above warning levels) and keeps the current ones.`}
        </p>
        <button type="button" disabled={!changed || incomplete || saving} onClick={() => onSave(draft)} className="border border-cyan/60 px-2 py-1 font-mono text-[11px] uppercase text-cyan disabled:border-line-strong disabled:text-dim">
          Save thresholds
        </button>
      </div>
    </Panel>
  );
}

function RetentionPanel({ retention, saving, onSave }: { retention: RetentionConfig; saving: boolean; onSave: (r: RetentionConfig) => void }) {
  const [draft, setDraft, changed] = useDraft(retention);
  const fields: { key: keyof RetentionConfig; label: string; unit: string }[] = [
    { key: "detailed_hours", label: "Detailed history (30 s)", unit: "hours" },
    { key: "hourly_days", label: "Hourly history", unit: "days" },
    { key: "alert_days", label: "Resolved alerts", unit: "days" },
  ];
  return (
    <Panel title="History retention" className="col-span-2">
      <div className="flex items-end gap-6">
        {fields.map((f) => (
          <label key={f.key} className="flex flex-col text-xs">
            <span className="text-muted">{f.label}</span>
            <span className="flex items-center gap-1">
              <input type="number" value={Number.isFinite(draft[f.key]) ? draft[f.key] : ""} onChange={(e) => setDraft({ ...draft, [f.key]: numberOf(e.target.value) })} className="w-20 border border-line-strong bg-canvas px-1 py-0.5 text-right font-mono text-xs text-fg" />
              <span className="text-dim">{f.unit}</span>
            </span>
          </label>
        ))}
        <button type="button" disabled={!changed || fields.some((f) => !Number.isFinite(draft[f.key])) || saving} onClick={() => onSave(draft)} className="ml-auto border border-cyan/60 px-2 py-1 font-mono text-[11px] uppercase text-cyan disabled:border-line-strong disabled:text-dim">
          Save retention
        </button>
      </div>
      <p className="mt-2 text-[10px] text-dim">History is stored only on this computer, in Sentinel's own database. Older data is deleted automatically. Process data is never stored.</p>
    </Panel>
  );
}

/** The daemon's defaults (sentinel-config `DocumentSettings::default`). */
export const DOCUMENT_DEFAULTS: DocumentSettings = {
  online_discovery: false,
  internet_archive: true,
  catalogue_urls: [],
  request_timeout_s: 20,
  min_request_interval_ms: 1500,
  cache_days: 14,
  max_download_mb: 100,
  auto_download: false,
};

/** Catalogue lines a user typed: trimmed, blank lines dropped. */
export function catalogueLines(text: string): string[] {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
}

function DocumentsPanel({ settings, saving, onSave }: { settings: DocumentSettings; saving: boolean; onSave: (d: DocumentSettings) => void }) {
  const [draft, setDraft, changed] = useDraft(settings);
  const [urls, setUrls] = useState(settings.catalogue_urls.join("\n"));
  const lines = catalogueLines(urls);
  const badUrl = lines.find((l) => !/^https:\/\/[^\s/@]+\.[^\s/@]+/.test(l));
  const urlsChanged = lines.join("\n") !== settings.catalogue_urls.join("\n");
  const fields: { key: "request_timeout_s" | "min_request_interval_ms" | "cache_days" | "max_download_mb"; label: string; unit: string }[] = [
    { key: "request_timeout_s", label: "Request timeout", unit: "s" },
    { key: "min_request_interval_ms", label: "Between requests to a site", unit: "ms" },
    { key: "cache_days", label: "Reuse results for", unit: "days" },
    { key: "max_download_mb", label: "Largest download", unit: "MB" },
  ];
  const incomplete = fields.some((f) => !Number.isFinite(draft[f.key])) || !!badUrl || lines.length > 8;
  return (
    <Panel title="Documentation discovery" className="col-span-2">
      <div className="grid grid-cols-2 gap-x-6 gap-y-2">
        <Switch label="Online discovery" checked={draft.online_discovery} disabled={saving} onChange={(online_discovery) => setDraft({ ...draft, online_discovery })} />
        <Switch label="Search the Internet Archive" checked={draft.internet_archive} disabled={saving || !draft.online_discovery} onChange={(internet_archive) => setDraft({ ...draft, internet_archive })} />
        <Switch label="Download official matches automatically" checked={draft.auto_download} disabled={saving || !draft.online_discovery} onChange={(auto_download) => setDraft({ ...draft, auto_download })} />
      </div>
      <p className="mt-1 text-[10px] text-dim">
        Off by default. When on, a search sends only the manufacturer, model and board identifier, never serial numbers or the inventory. Automatic downloads apply only to exact or high-confidence matches from the manufacturer&apos;s own site.
      </p>
      <label className="mt-3 flex flex-col text-xs">
        <span className="text-muted">Documentation catalogues (https addresses, one per line, at most 8)</span>
        <textarea value={urls} onChange={(e) => setUrls(e.target.value)} rows={3} spellCheck={false} className="mt-1 border border-line-strong bg-canvas px-1 py-0.5 font-mono text-[11px] text-fg" />
        {badUrl ? <span className="text-[10px] text-orange">Not a usable https address: {badUrl}</span> : null}
      </label>
      <div className="mt-3 flex items-end gap-6">
        {fields.map((f) => (
          <label key={f.key} className="flex flex-col text-xs">
            <span className="text-muted">{f.label}</span>
            <span className="flex items-center gap-1">
              <input type="number" value={Number.isFinite(draft[f.key]) ? draft[f.key] : ""} onChange={(e) => setDraft({ ...draft, [f.key]: numberOf(e.target.value) })} className="w-20 border border-line-strong bg-canvas px-1 py-0.5 text-right font-mono text-xs text-fg" />
              <span className="text-dim">{f.unit}</span>
            </span>
          </label>
        ))}
        <button
          type="button"
          disabled={(!changed && !urlsChanged) || incomplete || saving}
          onClick={() => onSave({ ...draft, catalogue_urls: lines })}
          className="ml-auto border border-cyan/60 px-2 py-1 font-mono text-[11px] uppercase text-cyan disabled:border-line-strong disabled:text-dim"
        >
          Save discovery settings
        </button>
      </div>
    </Panel>
  );
}
