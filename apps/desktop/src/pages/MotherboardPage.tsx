import { DiscoveryPanel, OriginBadge } from "../components/DiscoveryPanel";
import { FieldRow } from "../components/FieldRow";
import { EmptyState, Panel, StatusDot } from "../components/primitives";
import type { InventoryView, JobProgress } from "../types/inventory";

export const MOTHERBOARD_STATE = "Motherboard identity detected — schematic analysis not yet available.";

/**
 * The board's identity as firmware reports it. No board drawing is shown:
 * Sentinel has no verified layout for this board, and an invented one would
 * be presented as the user's hardware.
 */
export function MotherboardPage({
  inventory,
  progress,
  serviceRunning,
  onScan,
  onCancel,
}: {
  inventory: InventoryView | null;
  progress: JobProgress | null;
  serviceRunning: boolean;
  onScan: (force: boolean) => void;
  onCancel: () => void;
}) {
  const inv = inventory?.inventory ?? null;
  const board = inv?.motherboard ?? null;
  const identified = board !== null && board.identification.level !== "unidentified";
  return (
    <div className="grid min-w-[980px] grid-cols-12 gap-3">
      <Panel
        title="Motherboard"
        className="col-span-8"
        aside={inventory && inv ? <OriginBadge origin={inventory.origin} source={inventory.source} scannedAt={inv.discovered_at} /> : undefined}
      >
        {board ? (
          <>
            <div className="mb-3 border border-line bg-canvas p-3">
              <StatusDot tone={identified ? "ok" : "idle"} label={identified ? MOTHERBOARD_STATE : "Motherboard not identified — schematic analysis not yet available."} />
              <p className="mt-1 text-[11px] text-dim">{board.identification.explanation}</p>
            </div>
            <FieldRow label="System model" field={board.system_model} />
            <FieldRow label="System manufacturer" field={board.system_manufacturer} />
            <FieldRow label="Board manufacturer" field={board.board_manufacturer} />
            <FieldRow label="Board name" field={board.board_name} />
            <FieldRow label="Board revision" field={board.board_revision} />
            <FieldRow label="BIOS vendor" field={board.bios_vendor} />
            <FieldRow label="BIOS version" field={board.bios_version} />
            <FieldRow label="BIOS date" field={board.bios_release_date} />
            <div className="mt-3 grid grid-cols-3 gap-2 text-[11px]">
              <Fact label="Identification" value={board.identification.level.replace(/_/g, " ")} />
              <Fact label="Confidence" value="Firmware-reported" />
              <Fact label="Externally verified" value={board.identification.externally_verified ? "Yes" : "No"} />
            </div>
          </>
        ) : (
          <EmptyState title={inv ? "No board information" : "Not scanned yet"}>
            {inv ? "Firmware on this system did not report board identifiers." : "Run a hardware scan to read the board's identity from firmware."}
          </EmptyState>
        )}
      </Panel>
      <div className="col-span-4 flex flex-col gap-3">
        <Panel title="Hardware scan">
          <DiscoveryPanel job="hardware" progress={progress} serviceRunning={serviceRunning} hasResult={inv !== null} onScan={onScan} onCancel={onCancel} />
        </Panel>
        <Panel title="About this view">
          <p className="text-[11px] leading-relaxed text-dim">
            The laptop model and the motherboard model are different things, and a revision reported by firmware is not verified until it
            is matched against manufacturer documentation. Schematic search and an interactive board view come in a later phase; no board
            diagram is drawn until then.
          </p>
        </Panel>
      </div>
    </div>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="border border-line p-2">
      <div className="label">{label}</div>
      <div className="mt-1 font-mono text-xs capitalize text-fg">{value}</div>
    </div>
  );
}
