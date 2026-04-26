import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Settings } from "./Settings";
import { FlightLogs } from "./FlightLogs";
import { FlightDetails } from "./FlightDetails";
import "./App.css";

interface FlightEvent {
    timestamp: string;
    eventType: 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent';
    latitude: number;
    longitude: number;
}

interface FlightSummary {
    filename: string;
    startIcao: string;
    endIcao: string;
    startTime: string;
    endTime: string;
    durationMinutes: number;
    aircraftTitle: string;
    aircraftType: string;
    aircraftModel: string;
    maxAltitude: number;
    maxGroundSpeed: number;
    fuelConsumed: number;
    events: FlightEvent[];
}

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
  sim_on_ground: number;
}

const METRIC_LABELS: Record<string, string> = {
  latitude: "Latitude",
  longitude: "Longitude",
  alt_b: "Indicated Altitude (ft)",
  baro_a: "Altimeter Setting (inHg)",
  alt_msl: "Altitude MSL (ft)",
  oat: "Outside Air Temp (°C)",
  ias: "Indicated Airspeed (kt)",
  gnd_spd: "Groundspeed (kt)",
  v_spd: "Vertical Speed (fpm)",
  pitch: "Pitch Angle (deg)",
  roll: "Bank Angle (deg)",
  lat_ac: "Lateral Acceleration (G)",
  norm_ac: "Normal Acceleration (G)",
  hdg: "Heading (deg)",
  trk: "Track (deg)",
  volt1: "Bus Voltage 1",
  volt2: "Bus Voltage 2",
  amp1: "Bus Amperes 1",
  f_qty_l: "Fuel Left (gal)",
  f_qty_r: "Fuel Right (gal)",
  e1_fflow: "E1 Fuel Flow (gph)",
  e1_oil_t: "E1 Oil Temp (°F)",
  e1_oil_p: "E1 Oil Pressure (psi)",
  e1_map: "E1 Manifold Press (inHg)",
  e1_rpm: "E1 RPM",
  e1_pwr: "E1 Power (%)",
  e1_cht1: "E1 CHT 1",
  e1_cht2: "E1 CHT 2",
  e1_cht3: "E1 CHT 3",
  e1_cht4: "E1 CHT 4",
  e1_cht5: "E1 CHT 5",
  e1_cht6: "E1 CHT 6",
  e1_egt1: "E1 EGT 1",
  e1_egt2: "E1 EGT 2",
  e1_egt3: "E1 EGT 3",
  e1_egt4: "E1 EGT 4",
  e1_egt5: "E1 EGT 5",
  e1_egt6: "E1 EGT 6",
  e1_tit1: "E1 TIT 1",
  e1_tit2: "E1 TIT 2",
  alt_gps: "GPS Altitude (ft)",
  tas: "True Airspeed (kt)",
  hsis: "HSI Source",
  crs: "Selected Course (deg)",
  nav1: "NAV 1 Freq (MHz)",
  nav2: "NAV 2 Freq (MHz)",
  com1: "COM 1 Freq (MHz)",
  com2: "COM 2 Freq (MHz)",
  hcdi: "Horizontal CDI (fsd)",
  vcdi: "Vertical CDI (fsd)",
  wnd_spd: "Wind Speed (kt)",
  wnd_dr: "Wind Direction (deg)",
  wpt_dst: "Waypoint Distance (nm)",
  wpt_brg: "Waypoint Bearing (deg)",
  mag_var: "Magnetic Variation (deg)",
  afcs_on: "Autopilot Active",
  roll_m: "AP Roll Mode",
  pitch_m: "AP Pitch Mode",
  roll_c: "Roll Command (deg)",
  pitch_c: "Pitch Command (deg)",
  v_spd_g: "VS Target (fpm)",
  gps_fix: "GPS Fix Type",
  hal: "H-Alarm Limit (m)",
  val: "V-Alarm Limit (m)",
  hpl_was: "HPL WAAS (m)",
  hpl_fd: "HPL FD (m)",
  vpl_was: "VPL WAAS (m)",
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
  const [view, setView] = useState<"status" | "history" | "settings" | "details">("history");
  const [selectedFlight, setSelectedFlight] = useState<FlightSummary | null>(null);
  const [currentPhase, setCurrentPhase] = useState<string>("Parked");

  useEffect(() => {
    invoke<string[]>("get_logs").then(setLogs).catch(console.error);

    const unlistenLogs = listen<string>("log-update", (event) => {
      setLogs((prevLogs) => [...prevLogs, event.payload]);
    });

    const unlistenPhase = listen<string>("flight-phase-change", (event) => {
      setCurrentPhase(event.payload);
    });

    const interval = window.setInterval(async () => {
      try {
        const [m, connected] = await Promise.all([
          invoke<FlightMetrics>("get_metrics"),
          invoke<boolean>("is_sim_connected")
        ]);
        setMetrics(m);
        setSimConnected(connected);
      } catch (e) { }
    }, 200);

    return () => {
      unlistenLogs.then((f) => f());
      unlistenPhase.then((f) => f());
      clearInterval(interval);
    };
  }, []);

  const renderContent = () => {
    switch (view) {
      case "history":
        return <FlightLogs 
          onViewDetails={(flight) => {
            setSelectedFlight(flight);
            setView("details");
          }}
        />;
      case "details":
        return selectedFlight ? (
          <FlightDetails flight={selectedFlight} onBack={() => setView("history")} />
        ) : (
          <div>No flight selected</div>
        );
      case "settings":
        return <Settings onBack={() => setView("status")} />;
      case "status":
      default:
        return (
          <div className="status-view">
            {metrics && (
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
              MSFS CONNECTED
            </span>
          </div>
          {metrics && (
            <div className="status-bar-item" style={{ borderLeft: "1px solid rgba(255,255,255,0.1)", paddingLeft: "12px" }}>
              <span style={{ fontSize: "0.75rem", color: "rgba(255,255,255,0.8)" }}>
                <span style={{ color: metrics.sim_on_ground > 0.5 ? "#4caf50" : "#ffeb3b", fontWeight: "bold", marginRight: "8px" }}>
                  {metrics.sim_on_ground > 0.5 ? "GND" : "AIR"}
                </span>
                | IAS {metrics.ias.toFixed(0)} kt | {getWindComponent(metrics.wnd_spd, metrics.wnd_dr, metrics.hdg)} | GS {metrics.gnd_spd.toFixed(0)} kt | ALT {metrics.alt_msl.toFixed(0)} ft | VS {metrics.v_spd.toFixed(0)} fpm | OAT {metrics.oat.toFixed(0)}°C
              </span>
            </div>
          )}
        </footer>
      )}
    </div>
  );
}

export default App;
