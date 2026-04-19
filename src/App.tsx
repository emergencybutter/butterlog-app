import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Settings } from "./Settings";
import { FlightLogs } from "./FlightLogs";
import "./App.css";

interface FlightMetrics {
  latitude: number;
  longitude: number;
  alt_b: number;
  baro_a: number;
  alt_msl: number;
  oat: number;
  ias: number;
  gnd_spd: number;
  v_spd: number;
  pitch: number;
  roll: number;
  lat_ac: number;
  norm_ac: number;
  hdg: number;
  trk: number;
  volt1: number;
  volt2: number;
  amp1: number;
  f_qty_l: number;
  f_qty_r: number;
  e1_fflow: number;
  e1_oil_t: number;
  e1_oil_p: number;
  e1_map: number;
  e1_rpm: number;
  e1_pwr: number;
  e1_cht1: number;
  e1_cht2: number;
  e1_cht3: number;
  e1_cht4: number;
  e1_cht5: number;
  e1_cht6: number;
  e1_egt1: number;
  e1_egt2: number;
  e1_egt3: number;
  e1_egt4: number;
  e1_egt5: number;
  e1_egt6: number;
  e1_tit1: number;
  e1_tit2: number;
  alt_gps: number;
  tas: number;
  hsis: number;
  crs: number;
  nav1: number;
  nav2: number;
  com1: number;
  com2: number;
  hcdi: number;
  vcdi: number;
  wnd_spd: number;
  wnd_dr: number;
  wpt_dst: number;
  wpt_brg: number;
  mag_var: number;
  afcs_on: number;
  roll_m: number;
  pitch_m: number;
  roll_c: number;
  pitch_c: number;
  v_spd_g: number;
  gps_fix: number;
  hal: number;
  val: number;
  hpl_was: number;
  hpl_fd: number;
  vpl_was: number;
}

function App() {
  const [logs, setLogs] = useState<string[]>([]);
  const [metrics, setMetrics] = useState<FlightMetrics | null>(null);
  const [simConnected, setSimConnected] = useState(false);
  const [view, setView] = useState<"dashboard" | "settings" | "history">("dashboard");

  useEffect(() => {
    // Fetch existing logs on mount
    invoke<string[]>("get_logs").then(setLogs).catch(console.error);

    // Listen for new log events from the backend
    const unlisten = listen<string>("log-update", (event) => {
      setLogs((prevLogs) => [...prevLogs, event.payload]);
    });

    // Poll for metrics and connection status
    const interval = window.setInterval(async () => {
      try {
        const [m, connected] = await Promise.all([
          invoke<FlightMetrics>("get_metrics"),
          invoke<boolean>("is_sim_connected")
        ]);
        setMetrics(m);
        setSimConnected(connected);
      } catch (e) {
        // Silently handle if backend is not ready
      }
    }, 200);

    return () => {
      unlisten.then((f) => f());
      clearInterval(interval);
    };
  }, []);

  if (view === "settings") {
    return (
      <main className="container">
        <Settings onBack={() => setView("dashboard")} />
      </main>
    );
  }

  if (view === "history") {
    return (
      <main className="container">
        <FlightLogs onBack={() => setView("dashboard")} />
      </main>
    );
  }

  return (
    <main className="container">
      <div style={{ position: "absolute", top: "20px", right: "20px", display: "flex", gap: "10px" }}>
        <button onClick={() => setView("history")}>History</button>
        <button onClick={() => setView("settings")}>Settings</button>
      </div>

      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: "10px", marginBottom: "2rem" }}>
        <div style={{
          width: "12px",
          height: "12px",
          borderRadius: "50%",
          backgroundColor: simConnected ? "#4caf50" : "#f44336",
          boxShadow: simConnected ? "0 0 10px #4caf50" : "none"
        }} />
        <span style={{ fontWeight: "bold", color: simConnected ? "#4caf50" : "#f44336" }}>
          {simConnected ? "MSFS CONNECTED" : "MSFS DISCONNECTED"}
        </span>
      </div>

      {metrics && (
        <div className="metrics-display" style={{ textAlign: "left", marginBottom: "2rem" }}>
          <h3>Flight Metrics</h3>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))", gap: "10px", background: "#2a2a2a", padding: "1rem", borderRadius: "8px" }}>
            {Object.entries(metrics).map(([key, value]) => (
              <div key={key} style={{ borderBottom: "1px solid #444", padding: "5px" }}>
                <span style={{ fontWeight: "bold", fontSize: "0.8rem", color: "#888" }}>{key}:</span>
                <span style={{ float: "right", fontFamily: "monospace" }}>
                  {typeof value === "number" ? value.toFixed(2) : String(value)}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="logs-container" style={{ marginTop: "2rem", textAlign: "left" }}>
        <h3>Backend Logs</h3>
        <div style={{ background: "#1a1a1a", padding: "1rem", borderRadius: "8px", maxHeight: "200px", overflowY: "auto" }}>
          {logs.length === 0 ? <p style={{ color: "#888" }}>No logs yet...</p> : null}
          {logs.map((log, index) => (
            <div key={index} style={{ fontFamily: "monospace", fontSize: "0.9rem", color: "#4caf50" }}>{log}</div>
          ))}
        </div>
      </div>
    </main>
  );
}

export default App;
