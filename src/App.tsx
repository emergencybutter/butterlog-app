import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Settings } from "./Settings";
import { ServicePage, linkText } from "./pages/ServicePage";
import { TelemetryPage } from "./pages/TelemetryPage";
import { ScreenshotsPage } from "./pages/ScreenshotsPage";
import { MultiplayerPage } from "./pages/MultiplayerPage";
import { LogsPage } from "./pages/LogsPage";
import { ServiceStatus } from "./models";
import "./App.css";

// Cap the in-memory log buffer; the app often runs for days in the tray.
const MAX_UI_LOG_LINES = 2000;

type Page = "service" | "telemetry" | "screenshots" | "multiplayer" | "logs" | "settings";

const PAGES: { id: Page; label: string }[] = [
  { id: "telemetry", label: "Simulator telemetry" },
  { id: "screenshots", label: "Screenshots" },
  { id: "multiplayer", label: "Multiplayer debugging" },
  { id: "logs", label: "Backend logs" },
];

const SIMS: Record<string, string> = { msfs: "MSFS", xplane: "X-Plane" };

function App() {
  const [page, setPage] = useState<Page>("service");
  const [logs, setLogs] = useState<string[]>([]);
  const [status, setStatus] = useState<ServiceStatus | null>(null);
  const [sims, setSims] = useState<string[]>([]);

  useEffect(() => {
    invoke<string[]>("get_logs").then(setLogs).catch(console.error);
    invoke<ServiceStatus>("get_service_status").then(setStatus).catch(console.error);

    const unlisten = [
      listen<string>("log-update", (event) => {
        setLogs((prev) => {
          const next = [...prev, event.payload];
          return next.length > MAX_UI_LOG_LINES ? next.slice(-MAX_UI_LOG_LINES) : next;
        });
      }),
      listen<ServiceStatus>("service-status", (event) => setStatus(event.payload)),
      // A flight just done: the Service page says so, with its debrief.
      listen("flight-done", () => setPage("service")),
    ];

    // Which sims are connected, for the status line: every 2 s while shown.
    const poll = () => {
      if (document.hidden) return;
      invoke<string[]>("get_connected_sims").then(setSims).catch(() => {});
    };
    poll();
    const t = window.setInterval(poll, 2000);

    return () => {
      unlisten.forEach((u) => u.then((f) => f()));
      clearInterval(t);
    };
  }, []);

  const link = linkText(status);
  const signedIn = !!status && status.link !== "signed_out";

  const content = () => {
    switch (page) {
      case "service": return <ServicePage status={status} />;
      case "telemetry": return <TelemetryPage signedIn={signedIn} />;
      case "screenshots": return <ScreenshotsPage signedIn={signedIn} onSettings={() => setPage("settings")} />;
      case "multiplayer": return <MultiplayerPage />;
      case "logs": return <LogsPage logs={logs} />;
      case "settings": return <Settings />;
    }
  };

  return (
    <div className="app">
      <div className="body">
        <nav className="nav">
          <div className="nav-brand"><img src="/icon.png" alt="" />ButterLog</div>

          <button className={`nav-item nav-service ${page === "service" ? "active" : ""}`} onClick={() => setPage("service")}>
            <span style={{ display: "flex", flexDirection: "column", gap: 2, flex: 1, minWidth: 0 }}>
              <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span className={`dot ${link.dot}`} />
                <span className="label">Service</span>
                {status?.flightDone && <span className="badge" title="A flight is done">1</span>}
              </span>
              <span className="who">{status?.me?.displayName ?? (signedIn ? status?.serviceUrl.replace(/^https?:\/\//, "") : "Not signed in")}</span>
            </span>
          </button>

          <div className="nav-sep" />

          {PAGES.map((p) => (
            <button key={p.id} className={`nav-item ${page === p.id ? "active" : ""}`} onClick={() => setPage(p.id)}>
              <span className="label">{p.label}</span>
            </button>
          ))}

          <div className="nav-spacer" />
          <button className={`nav-item ${page === "settings" ? "active" : ""}`} onClick={() => setPage("settings")}>
            <span className="label">Settings</span>
          </button>
        </nav>

        <main className="main">{content()}</main>
      </div>

      <footer className="statusbar">
        <button className="item link" onClick={() => setPage("service")} title={status?.error ?? status?.serviceUrl ?? ""}>
          <span className={`dot ${link.dot}`} />
          {link.text}
        </button>
        {status?.recording && <span className="item"><span className="dot accent" />Recording</span>}
        {!!status?.unsentKb && <span className="item warn">{status.unsentKb.toLocaleString()} kB to send</span>}
        {status?.uploadError && <span className="item bad" title={status.uploadError}>Upload paused</span>}
        <span className="item">{sims.length ? sims.map((s) => SIMS[s] ?? s).join(" + ") + " connected" : "No simulator"}</span>
        <span className="item push faint" title="The window draws without the GPU, and is freed entirely when closed to the tray">No GPU · closes to tray</span>
      </footer>
    </div>
  );
}

export default App;
