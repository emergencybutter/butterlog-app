import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { check } from "@tauri-apps/plugin-updater";
import { ask } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { Settings } from "./Settings";
import { FlightLogs } from "./FlightLogs";
import { FlightDetails } from "./FlightDetails";
import { AircraftStats } from "./AircraftStats";
import { FlightMetrics, FlightSummary } from "./models";
import "./App.css";

const METRIC_LABELS: Record<string, string> = {
  Latitude: "Latitude",
  Longitude: "Longitude",
  AltB: "Indicated Altitude (ft)",
  BaroA: "Altimeter Setting (inHg)",
  AltMSL: "Altitude MSL (ft)",
  OAT: "Outside Air Temp (°C)",
  IAS: "Indicated Airspeed (kt)",
  GndSpd: "Groundspeed (kt)",
  VSpd: "Vertical Speed (fpm)",
  Pitch: "Pitch Angle (deg)",
  Roll: "Bank Angle (deg)",
  LatAc: "Lateral Acceleration (G)",
  NormAc: "Normal Acceleration (G)",
  HDG: "Heading (deg)",
  TRK: "Track (deg)",
  volt1: "Bus Voltage 1",
  volt2: "Bus Voltage 2",
  amp1: "Bus Amperes 1",
  FQtyL: "Fuel Left (gal)",
  FQtyR: "Fuel Right (gal)",
  "E1 FFlow": "E1 Fuel Flow (gph)",
  "E1 OilT": "E1 Oil Temp (°F)",
  "E1 OilP": "E1 Oil Pressure (psi)",
  "E1 MAP": "E1 Manifold Press (inHg)",
  "E1 RPM": "E1 RPM",
  "E1 %Pwr": "E1 Power (%)",
  "E1 CHT1": "E1 CHT 1",
  "E1 CHT2": "E1 CHT 2",
  "E1 CHT3": "E1 CHT 3",
  "E1 CHT4": "E1 CHT 4",
  "E1 CHT5": "E1 CHT 5",
  "E1 CHT6": "E1 CHT 6",
  "E1 EGT1": "E1 EGT 1",
  "E1 EGT2": "E1 EGT 2",
  "E1 EGT3": "E1 EGT 3",
  "E1 EGT4": "E1 EGT 4",
  "E1 EGT5": "E1 EGT 5",
  "E1 EGT6": "E1 EGT 6",
  "E1 TIT1": "E1 TIT 1",
  "E1 TIT2": "E1 TIT 2",
  AltGPS: "GPS Altitude (ft)",
  TAS: "True Airspeed (kt)",
  HSIS: "HSI Source",
  CRS: "Selected Course (deg)",
  NAV1: "NAV 1 Freq (MHz)",
  NAV2: "NAV 2 Freq (MHz)",
  COM1: "COM 1 Freq (MHz)",
  COM2: "COM 2 Freq (MHz)",
  HCDI: "Horizontal CDI (fsd)",
  VCDI: "Vertical CDI (fsd)",
  WndSpd: "Wind Speed (kt)",
  WndDr: "Wind Direction (deg)",
  WptDst: "Waypoint Distance (nm)",
  WptBrg: "Waypoint Bearing (deg)",
  MagVar: "Magnetic Variation (deg)",
  AfcsOn: "Autopilot Active",
  RollM: "AP Roll Mode",
  PitchM: "AP Pitch Mode",
  RollC: "Roll Command (deg)",
  PichC: "Pitch Command (deg)",
  VSpdG: "VS Target (fpm)",
  GPSfix: "GPS Fix Type",
  HAL: "H-Alarm Limit (m)",
  VAL: "V-Alarm Limit (m)",
  HPLwas: "HPL WAAS (m)",
  HPLfd: "HPL FD (m)",
  VPLwas: "VPL WAAS (m)",
  sim_on_ground: "On Ground",
};

const getWindComponent = (speed: number, dir: number, hdg: number) => {
  if (speed < 0.5) return "WND CALM";
  const headwind = speed * Math.cos((dir - hdg) * Math.PI / 180);
  return `${headwind >= 0 ? "H" : "T"} ${Math.abs(Math.round(headwind))} kt`;
};

const Icons = {
  Logs: () => (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
      <polyline points="14 2 14 8 20 8"></polyline>
      <line x1="16" y1="13" x2="8" y2="13"></line>
      <line x1="16" y1="17" x2="8" y2="17"></line>
      <polyline points="10 9 9 9 8 9"></polyline>
    </svg>
  ),
  Status: () => (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="18" y1="20" x2="18" y2="10"></line>
      <line x1="12" y1="20" x2="12" y2="4"></line>
      <line x1="6" y1="20" x2="6" y2="14"></line>
    </svg>
  ),
  Aircraft: () => (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M17.8 19.2L16 11l3.5-3.5C21 6 21.5 4 21 3c-1-.5-3 0-4.5 1.5L13 8 4.8 6.2c-.5-.1-1.1.1-1.5.5l-.3.3c-.4.4-.5 1-.1 1.5l7.5 4.5-4.5 4.5-2.5-.5c-.5-.1-1.1.1-1.5.5l-.3.3c-.4.4-.5 1-.1 1.5l2 2 2 2c.5.4 1.1.3 1.5-.1l.3-.3c.4-.4.6-1 .5-1.5l-.5-2.5 4.5-4.5 4.5 7.5c.5.4 1.1.3 1.5-.1l.3-.3c.4-.4.6-1 .5-1.5z"></path>
    </svg>
  ),
  Settings: () => (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3"></circle>
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
    </svg>
  )
};

function App() {
  const [logs, setLogs] = useState<string[]>([]);
  const [metrics, setMetrics] = useState<FlightMetrics | null>(null);
  const [simConnected, setSimConnected] = useState(false);
  const [connectedSims, setConnectedSims] = useState<string[]>([]);
  const [view, setView] = useState<"status" | "history" | "settings" | "details" | "aircraft">("history");
  const [selectedFlight, setSelectedFlight] = useState<FlightSummary | null>(null);
  const [currentPhase, setCurrentPhase] = useState<string>("Parked");
  const [flightOngoing, setFlightOngoing] = useState(false);
  const [currentFlightId, setCurrentFlightId] = useState<string>("");

  useEffect(() => {
    // Check for updates on startup
    const checkForUpdates = async () => {
      try {
        const update = await check();
        if (update) {
          console.log(`Update available: ${update.version}`);
          const yes = await ask(`A new version (${update.version}) is available. Would you like to install it now?\n\nRelease notes: ${update.body}`, {
            title: 'Update Available',
            kind: 'info'
          });
          
          if (yes) {
            await update.downloadAndInstall();
            await relaunch();
          }
        }
      } catch (e) {
        console.error("Failed to check for updates:", e);
      }
    };
    checkForUpdates();

    invoke<string[]>("get_logs").then(setLogs).catch(console.error);

    const unlistenLogs = listen<string>("log-update", (event) => {
      setLogs((prevLogs) => [...prevLogs, event.payload]);
    });

    const unlistenPhase = listen<string>("flight-phase-change", (event) => {
      setCurrentPhase(event.payload);
    });

    const interval = window.setInterval(async () => {
      try {
        const [m, connected, ongoing, sims, fid] = await Promise.all([
          invoke<FlightMetrics>("get_metrics"),
          invoke<boolean>("is_sim_connected"),
          invoke<boolean>("is_flight_ongoing"),
          invoke<string[]>("get_connected_sims"),
          invoke<string>("get_current_flight_id")
        ]);
        setMetrics(m);
        setSimConnected(connected);
        setFlightOngoing(ongoing);
        setConnectedSims(sims);
        setCurrentFlightId(fid);
      } catch (e) { }
    }, 200);

    return () => {
      unlistenLogs.then((f) => f());
      unlistenPhase.then((f) => f());
      clearInterval(interval);
    };
  }, []);

  const getSimNameDisplay = () => {
    if (connectedSims.length === 0) return "SIM";
    return connectedSims.map(s => s.toUpperCase()).join(" + ");
  };

  const renderContent = () => {
    switch (view) {
      case "history":
        return <FlightLogs 
          currentFlightId={currentFlightId}
          onViewDetails={(flight) => {
            setSelectedFlight(flight);
            setView("details");
          }}
        />;
      case "details":
        return selectedFlight ? (
          <FlightDetails flight={selectedFlight} currentFlightId={currentFlightId} onBack={() => setView("history")} />
        ) : (
          <div>No flight selected</div>
        );
      case "aircraft":
        return <AircraftStats onViewDetails={(flight) => {
            setSelectedFlight(flight);
            setView("details");
        }} />;
      case "settings":
        return <Settings onBack={() => setView("status")} />;
      case "status":
      default:
        return (
          <div className="status-view">
            {metrics && flightOngoing && (
              <div className="metrics-display" style={{ textAlign: "left", marginBottom: "2rem" }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
                    <h3>Flight Metrics</h3>
                    <div style={{ background: "#4caf50", color: "white", padding: "4px 12px", borderRadius: "20px", fontSize: "0.8rem", fontWeight: "bold" }}>
                        PHASE: {currentPhase.toUpperCase()}
                    </div>
                </div>
                <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))", gap: "10px", background: "#2a2a2a", padding: "1rem", borderRadius: "8px" }}>
                  {Object.entries(metrics).map(([key, value]) => (
                    <div key={key} style={{ borderBottom: "1px solid #444", padding: "5px" }}>
                      <span style={{ fontWeight: "bold", fontSize: "0.8rem", color: "#888" }}>{METRIC_LABELS[key] || key}:</span>
                      <span style={{ float: "right", fontFamily: "monospace" }}>
                        {typeof value === "number" ? value.toFixed(2) : String(value)}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {!flightOngoing && simConnected && (
              <div style={{ background: "#2a2a2a", padding: "2rem", borderRadius: "8px", textAlign: "center", marginBottom: "2rem" }}>
                <div style={{ fontSize: "1.2rem", color: "#4caf50", fontWeight: "bold", marginBottom: "0.5rem" }}>{getSimNameDisplay()} CONNECTED</div>
                <div style={{ color: "#888" }}>Waiting for flight movement to start logging...</div>
              </div>
            )}

            {!simConnected && (
              <div style={{ background: "#2a2a2a", padding: "2rem", borderRadius: "8px", textAlign: "center", marginBottom: "2rem" }}>
                <div style={{ fontSize: "1.2rem", color: "#f44336", fontWeight: "bold", marginBottom: "0.5rem" }}>DISCONNECTED</div>
                <div style={{ color: "#888" }}>Start your flight simulator to begin logging.</div>
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
          </div>
        );
    }
  };

  return (
    <div className="app-container">
      <div className="app-layout">
        <nav className="sidebar">
          <div className="sidebar-top">
            <div 
              className={`sidebar-item ${view === 'history' || view === 'details' ? 'active' : ''}`} 
              onClick={() => {
                setView('history');
                setSelectedFlight(null);
              }}
              title="Logs"
            >
              <span className="icon"><Icons.Logs /></span>
            </div>
            <div 
              className={`sidebar-item ${view === 'aircraft' ? 'active' : ''}`} 
              onClick={() => setView('aircraft')}
              title="Aircraft Stats"
            >
              <span className="icon"><Icons.Aircraft /></span>
            </div>
            <div 
              className={`sidebar-item ${view === 'status' ? 'active' : ''}`} 
              onClick={() => setView('status')}
              title="Status"
            >
              <span className="icon"><Icons.Status /></span>
            </div>
          </div>
          <div className="sidebar-bottom">
            <div 
              className={`sidebar-item ${view === 'settings' ? 'active' : ''}`} 
              onClick={() => setView('settings')}
              title="Settings"
            >
              <span className="icon"><Icons.Settings /></span>
            </div>
          </div>
        </nav>
        <main className="main-content">
          {renderContent()}
        </main>
      </div>
      {simConnected && (
        <footer className="status-bar" style={{ backgroundColor: "#007acc" }}>
          <div className="status-bar-item">
            <div style={{
              width: "8px",
              height: "8px",
              borderRadius: "50%",
              backgroundColor: "#ffffff",
              marginRight: "8px"
            }} />
            <span style={{ fontSize: "0.75rem", fontWeight: "bold" }}>
              {getSimNameDisplay()} CONNECTED
            </span>
          </div>
          {metrics && (
            <div className="status-bar-item" style={{ borderLeft: "1px solid rgba(255,255,255,0.1)", paddingLeft: "12px" }}>
              <span style={{ fontSize: "0.75rem", color: "rgba(255,255,255,0.8)" }}>
                <span style={{ color: metrics.sim_on_ground > 0.5 ? "#4caf50" : "#ffeb3b", fontWeight: "bold", marginRight: "8px" }}>
                  {metrics.sim_on_ground > 0.5 ? "GND" : "AIR"}
                </span>
                | IAS {metrics.IAS.toFixed(0)} kt | {getWindComponent(metrics.WndSpd, metrics.WndDr, metrics.HDG)} | GS {metrics.GndSpd.toFixed(0)} kt | ALT {metrics.AltMSL.toFixed(0)} ft | VS {metrics.VSpd.toFixed(0)} fpm | OAT {metrics.OAT.toFixed(0)}°C
              </span>
            </div>
          )}
        </footer>
      )}
    </div>
  );
}

export default App;
