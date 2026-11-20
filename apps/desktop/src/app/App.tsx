import { useEffect, useState } from "react";

import { Header } from "../components/Header";
import { Sidebar } from "../components/Sidebar";
import { useInventory } from "../hooks/useInventory";
import { useTelemetry } from "../hooks/useTelemetry";
import { AlertsPage } from "../pages/AlertsPage";
import { TelemetryPage } from "../pages/telemetry/TelemetryPage";
import { useSentinel } from "../hooks/useSentinel";
import { AppShell } from "../layouts/AppShell";
import { HardwarePage, type ExplorerCategory, type ExplorerSelection } from "../pages/HardwarePage";
import { MotherboardPage } from "../pages/MotherboardPage";
import { OverviewPage } from "../pages/OverviewPage";
import { PlaceholderPage } from "../pages/PlaceholderPage";
import { SchematicLibraryPage } from "../pages/SchematicLibraryPage";
import { useDocuments } from "../hooks/useDocuments";
import { ServicesPage } from "../pages/ServicesPage";
import { SettingsPage } from "../pages/SettingsPage";
import { UtilitiesPage } from "../pages/UtilitiesPage";
import type { DiscoveryJob, InventoryView } from "../types/inventory";
import { value } from "./inventoryView";

function deviceIdentity(view: InventoryView | null): { name: string | null; cached: boolean } | null {
  const system = view?.inventory?.system;
  if (!view?.inventory) return null;
  const name = system ? [value(system.manufacturer), value(system.product_name)].filter(Boolean).join(" ") : "";
  return { name: name || null, cached: view.origin === "cache" };
}
import { NAV_ITEMS } from "./catalog";
import { monitoringIndicator, serviceIndicator } from "./viewModel";

export function App() {
  const sentinel = useSentinel();
  const [activeId, setActiveId] = useState("overview");
  // Kept here so the explorer's place survives leaving the page.
  const [explorerCategory, setExplorerCategory] = useState<ExplorerCategory>("overview");
  const [explorerSelection, setExplorerSelection] = useState<ExplorerSelection>({});
  const active = NAV_ITEMS.find((item) => item.id === activeId) ?? NAV_ITEMS[0]!;
  // null: not known yet, or the daemon is there but not answering. Only an
  // answer, or a clean "not running", decides.
  const unreachable = sentinel.phase === "ready" && sentinel.overview?.ipc === "error";
  const serviceRunning = sentinel.phase === "ready" && !unreachable ? sentinel.overview?.running === true : null;
  const inv = useInventory(serviceRunning);
  const tel = useTelemetry(serviceRunning);
  // Loaded only while the documentation views are open; never polled otherwise.
  const docs = useDocuments(serviceRunning, active.id === "schematics" || (active.id === "hardware" && explorerCategory === "motherboard"));
  const monitoringOn = !!tel.report && tel.report.phase !== "off" && tel.report.phase !== "error";
  const running = serviceRunning === true;
  const scanHandlers = (job: DiscoveryJob) => ({
    progress: inv.status?.[job] ?? (job === "hardware" ? inv.inventory?.progress : inv.utilities?.progress) ?? null,
    serviceRunning: running,
    onScan: (force: boolean) => void inv.scan([job], force),
    onCancel: () => void inv.cancel(job),
  });

  // The theme is the one setting that takes visible effect today.
  const theme = sentinel.config?.config.theme ?? "dark";
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  const lifecycleBusy = sentinel.busy === "starting" || sentinel.busy === "stopping" ? sentinel.busy : null;
  const saving = sentinel.busy === "saving";

  const notice =
    sentinel.phase === "unavailable"
      ? "Preview mode: this page is open outside the Sentinel desktop app, so the daemon's state is unknown and controls are disabled."
      : sentinel.overview?.daemon?.recovered_from_unclean_shutdown
        ? "The previous Sentinel session did not shut down cleanly. This session started normally."
        : sentinel.overview?.daemon && !sentinel.overview.daemon.storage_available
          ? "Sentinel's local database could not be opened. The service is running without session history."
          : null;

  return (
    <AppShell
      header={
        <Header
          version={sentinel.overview?.client_version ?? null}
          device={deviceIdentity(inv.inventory)}
          service={serviceIndicator(sentinel.phase, sentinel.overview)}
          monitoring={monitoringIndicator(sentinel.phase, sentinel.overview)}
          serviceOn={sentinel.phase === "ready" && sentinel.overview?.running === true}
          serviceBusy={lifecycleBusy}
          serviceControllable={sentinel.phase === "ready" && !unreachable}
          onServiceToggle={(next) => void sentinel.setServiceRunning(next)}
          monitoringOn={monitoringOn}
          monitoringControllable={serviceRunning === true && tel.busy === null}
          onMonitoringToggle={(on) => void tel.command(on ? "start" : "stop").then(() => sentinel.refresh())}
        />
      }
      sidebar={<Sidebar activeId={active.id} onNavigate={setActiveId} />}
      notice={inv.launched ? `Opened ${inv.launched}.` : notice}
      error={sentinel.error ?? inv.error ?? tel.error ?? docs.error}
      onDismissError={() => {
        sentinel.dismissError();
        inv.dismissError();
        tel.dismissError();
        docs.dismissError();
      }}
    >
      {active.id === "overview" ? (
        <OverviewPage
          phase={sentinel.phase}
          overview={sentinel.overview}
          config={sentinel.config}
          saving={saving}
          onPowerMode={(power_mode) => void sentinel.updateConfiguration({ power_mode })}
          inventory={inv.inventory}
          progress={scanHandlers("hardware").progress}
          onScan={(force) => void inv.scan(["hardware", "utilities"], force)}
          onCancel={() => void inv.cancel("hardware")}
          onOpenHardware={() => setActiveId("hardware")}
          telemetry={tel}
          onMonitoring={(c) => void tel.command(c).then(() => sentinel.refresh())}
          onProfile={(p) => void tel.setSampling(p, null).then(() => sentinel.refresh())}
          onOpenAlerts={() => setActiveId("alerts")}
        />
      ) : active.id === "hardware" ? (
        <HardwarePage
          inventory={inv.inventory}
          {...scanHandlers("hardware")}
          telemetry={{ snapshot: tel.snapshot, report: tel.report, stale: tel.stale || unreachable }}
          category={explorerCategory}
          onCategory={setExplorerCategory}
          selection={explorerSelection}
          onSelection={setExplorerSelection}
          documents={docs.view}
          onOpenDocuments={() => setActiveId("schematics")}
        />
      ) : active.id === "schematics" ? (
        <SchematicLibraryPage
          controls={{
            view: docs.view,
            busy: docs.busy,
            search: (force) => void docs.search(force),
            cancel: () => void docs.cancel(),
            download: (id) => void docs.download(id),
            removeFile: (id) => void docs.removeFile(id),
            clearCache: () => void docs.clearCache(),
            open: (id, target) => void docs.open(id, target),
          }}
          serviceRunning={running}
          onOpenSettings={() => setActiveId("settings")}
        />
      ) : active.id === "motherboard" ? (
        <MotherboardPage inventory={inv.inventory} {...scanHandlers("hardware")} />
      ) : active.id === "utilities" ? (
        <UtilitiesPage utilities={inv.utilities} launching={inv.launching} onLaunch={(id) => void inv.launch(id)} {...scanHandlers("utilities")} />
      ) : active.id === "telemetry" ? (
        <TelemetryPage
          report={tel.report}
          snapshot={tel.snapshot}
          processor={inv.inventory?.inventory?.processors[0] ?? null}
          thresholds={sentinel.config?.config.alerts ?? null}
          serviceRunning={running}
          unreachable={unreachable}
          stale={tel.stale}
          onStart={() => void tel.command("start").then(() => sentinel.refresh())}
        />
      ) : active.id === "alerts" ? (
        <AlertsPage active={tel.alerts} onAcknowledge={(id) => void tel.acknowledge(id)} serviceRunning={running} />
      ) : active.id === "services" ? (
        <ServicesPage utilities={inv.utilities} {...scanHandlers("utilities")} />
      ) : active.id === "settings" ? (
        <SettingsPage
          config={sentinel.config}
          overview={sentinel.overview}
          saving={saving}
          onChange={(patch) => void sentinel.updateConfiguration(patch)}
        />
      ) : (
        <PlaceholderPage item={active} />
      )}
    </AppShell>
  );
}
