import { formatBytes } from "../../app/inventoryView";
import { celsius, pct, rate, reading, val, watts } from "../../app/telemetryView";
import { EmptyState, Panel, Tag } from "../../components/primitives";
import type { AlertThresholdsConfig } from "../../types/sentinel";
import { ChartPanel, Metric, type TabProps } from "./TelemetryTabs";

export function ThermalTab({ snapshot, live, refreshMs, thresholds }: TabProps & { thresholds: AlertThresholdsConfig | null }) {
  const temps = snapshot.thermal?.temperatures ?? [];
  const fans = snapshot.fans?.fans ?? [];
  const user = thresholds?.temperature_warning_c ?? null;
  return (
    <div className="grid grid-cols-12 gap-3">
      <Panel title={`Temperature sensors · ${temps.length}`} className="col-span-8" bodyClassName="p-0">
        {temps.length === 0 ? (
          <EmptyState title="No temperature sensors are exposed" />
        ) : (
          <table className="w-full text-left text-[11px]">
            <thead>
              <tr className="border-b border-line">
                {["Sensor", "Temperature", "Component", "Association", "Warning threshold", "Source"].map((h) => (
                  <th key={h} className="label px-2 py-1 font-normal">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {temps.map((t) => {
                const warn = user ?? t.max_c;
                return (
                  <tr key={t.id} className="border-b border-line">
                    <td className="px-2 py-1 text-fg">{`${t.chip} ${t.label ?? ""}`}</td>
                    <td className={`px-2 py-1 font-mono ${val(t.value_c) !== null && warn !== null && val(t.value_c)! >= warn ? "text-orange" : ""}`}>{reading(t.value_c, celsius).text}</td>
                    <td className="px-2 py-1 text-muted">{t.association.component}</td>
                    <td className="px-2 py-1 text-[10px] text-dim" title={t.association.basis}>{t.association.confidence} · {t.association.basis}</td>
                    <td className="px-2 py-1 font-mono text-[10px] text-muted">
                      {user !== null ? `${user} °C (your setting)` : t.max_c !== null ? `${t.max_c} °C (driver)` : "none"}
                      {t.critical_c !== null ? ` · critical ${t.critical_c} °C (driver)` : ""}
                    </td>
                    <td className="selectable max-w-[180px] truncate px-2 py-1 font-mono text-[10px] text-dim" title={t.source}>{t.source}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        <p className="p-2 text-[10px] text-dim">
          Sentinel uses no universal "safe" temperature. Warnings use limits the driver reports, or a threshold you set in Settings.
        </p>
      </Panel>
      <Panel title={`Fans · ${fans.length}`} className="col-span-4">
        {fans.length === 0 ? (
          <EmptyState title="No fan speeds are exposed">Fan information is not available on this system.</EmptyState>
        ) : (
          fans.map((f) => (
            <div key={f.id} className="flex justify-between border-b border-line py-1 text-xs">
              <span className="text-muted">{`${f.chip} ${f.label ?? ""}`}</span>
              <span className="font-mono">{reading(f.rpm, (v) => `${v} RPM`).text}</span>
            </div>
          ))
        )}
        <p className="mt-2 text-[10px] text-dim">Read only. Sentinel never controls fans.</p>
      </Panel>
      <div className="col-span-12">
        <ChartPanel title="Temperature history" keys={temps.filter((t) => t.value_c.state === "ok").slice(0, 6).map((t) => `temp.${t.id}`)} labels={Object.fromEntries(temps.map((t) => [`temp.${t.id}`, `${t.chip} ${t.label ?? ""}`]))} live={live} refreshMs={refreshMs} height={150} />
      </div>
    </div>
  );
}

export function GpuTab({ snapshot, live, refreshMs }: TabProps) {
  const gpus = snapshot.gpu?.gpus ?? [];
  if (gpus.length === 0) {
    return (
      <Panel title="GPU">
        <EmptyState title="No GPU telemetry">No PCI graphics device was found. Sentinel works without a GPU.</EmptyState>
      </Panel>
    );
  }
  return (
    <div className="grid grid-cols-12 gap-3">
      {gpus.map((g) => (
        <div key={g.pci_address} className="col-span-12 grid grid-cols-12 gap-3">
          <Panel title={`GPU ${g.pci_address} · ${g.driver ?? "no driver"}`} className="col-span-12" bodyClassName="grid grid-cols-6 gap-4 p-3" aside={<Tag kind="stored">{g.memory_model === "shared" ? "Integrated · shared memory" : g.memory_model === "dedicated" ? "Dedicated memory" : "Memory type unknown"}</Tag>}>
            <Metric label="Busy" r={g.busy_percent} render={pct} big />
            <Metric label="Temperature" r={g.temperature_c} render={celsius} />
            <Metric label="Power" r={g.power_w} render={watts} />
            <Metric label="Core clock" r={g.core_clock_mhz} render={(v) => `${v} MHz`} />
            <Metric label={g.memory_model === "shared" ? "Reserved system RAM in use" : "VRAM in use"} r={g.vram_used_bytes} render={(v) => `${formatBytes(v)} of ${reading(g.vram_total_bytes, formatBytes).text}`} />
            <Metric label="System memory mapped (GTT)" r={g.gtt_used_bytes} render={formatBytes} />
            {g.note ? <p className="col-span-6 text-[11px] text-orange">{g.note}</p> : null}
          </Panel>
          {g.busy_percent.state !== "unsupported" ? (
            <div className="col-span-12">
              <ChartPanel title="GPU history" keys={[`gpu.${g.pci_address}.busy`]} labels={{ [`gpu.${g.pci_address}.busy`]: "busy %" }} live={live} refreshMs={refreshMs} />
            </div>
          ) : null}
        </div>
      ))}
    </div>
  );
}

export function StorageTab({ snapshot, live, refreshMs }: TabProps) {
  const st = snapshot.storage;
  return (
    <div className="grid grid-cols-12 gap-3">
      <Panel title="Drive activity" className="col-span-6" bodyClassName="p-0" aside={<span className="label">Activity is not health</span>}>
        {(st?.devices ?? []).map((d) => (
          <div key={d.name} className="grid grid-cols-4 gap-3 border-b border-line p-2">
            <span className="font-mono text-xs text-fg">{d.name}</span>
            <Metric label="Read" r={d.read_bps} render={rate} />
            <Metric label="Write" r={d.write_bps} render={rate} />
            <Metric label="Busy" r={d.busy_percent} render={pct} />
          </div>
        ))}
      </Panel>
      <Panel title="Filesystems" className="col-span-6" bodyClassName="p-0">
        {(st?.filesystems ?? []).map((f) => (
          <div key={f.mount_point} className="border-b border-line p-2">
            <div className="flex justify-between text-xs">
              <span className="font-mono text-fg">{f.mount_point}</span>
              <span className="text-dim">{`${f.device} · ${f.fs_type}${f.read_only ? " · read-only" : ""}`}</span>
            </div>
            <div className="mt-1 h-1.5 bg-raised">
              {val(f.used_percent) !== null ? <div className="h-full bg-blue" style={{ width: `${Math.min(100, val(f.used_percent)!)}%` }} /> : null}
            </div>
            <div className="mt-1 font-mono text-[11px] text-muted">
              {reading(f.used_percent, pct).text} used · {reading(f.available_bytes, formatBytes).text} free of {reading(f.total_bytes, formatBytes).text}
            </div>
          </div>
        ))}
      </Panel>
      <div className="col-span-12">
        <ChartPanel title="Throughput history" keys={(st?.devices ?? []).flatMap((d) => [`disk.${d.name}.read`, `disk.${d.name}.write`])} live={live} refreshMs={refreshMs} />
      </div>
    </div>
  );
}

export function NetworkTab({ snapshot, live, refreshMs }: TabProps) {
  const ifaces = snapshot.network?.interfaces.filter((i) => !i.loopback) ?? [];
  return (
    <div className="grid grid-cols-12 gap-3">
      <Panel title="Interfaces" className="col-span-12" bodyClassName="p-0" aside={<span className="label">Counters only · no traffic is inspected</span>}>
        <table className="w-full text-left text-[11px]">
          <thead>
            <tr className="border-b border-line">
              {["Interface", "State", "Receiving", "Sending", "Received total", "Sent total", "Errors (rx/tx)", "Dropped (rx/tx)"].map((h) => (
                <th key={h} className="label px-2 py-1 font-normal">{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {ifaces.map((i) => (
              <tr key={i.name} className="border-b border-line">
                <td className="px-2 py-1 font-mono text-fg">{i.name}</td>
                <td className="px-2 py-1 text-muted">{i.operational_state ?? "unknown"}</td>
                <td className="px-2 py-1 font-mono">{reading(i.rx_bps, rate).text}</td>
                <td className="px-2 py-1 font-mono">{reading(i.tx_bps, rate).text}</td>
                <td className="px-2 py-1 font-mono text-muted">{formatBytes(i.rx_total_bytes)}</td>
                <td className="px-2 py-1 font-mono text-muted">{formatBytes(i.tx_total_bytes)}</td>
                <td className="px-2 py-1 font-mono text-muted">{`${i.rx_errors} / ${i.tx_errors}`}</td>
                <td className="px-2 py-1 font-mono text-muted">{`${i.rx_dropped} / ${i.tx_dropped}`}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Panel>
      <div className="col-span-12">
        <ChartPanel title="Throughput history" keys={ifaces.flatMap((i) => [`net.${i.name}.rx`, `net.${i.name}.tx`])} live={live} refreshMs={refreshMs} />
      </div>
    </div>
  );
}
