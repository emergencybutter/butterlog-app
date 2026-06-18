import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { check } from "@tauri-apps/plugin-updater";
import { ask } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { Settings } from "./Settings";
import { FlightLogs } from "./FlightLogs";
import { FlightDetails } from "./FlightDetails";
import { AircraftStats } from "./AircraftStats";
import { FlightMetrics, FlightSummary, MultiplayerDebugInfo } from "./models";
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

// Cap the in-memory log buffer; the app often runs for days in the tray.
const MAX_UI_LOG_LINES = 2000;

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
  Activity: () => (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="22 12 18 12 15 21 9 3 6 12 2 12"></polyline>
    </svg>
  ),
  Settings: () => (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3"></circle>
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
    </svg>
  ),
  Copy: () => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
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
  const [authNotice, setAuthNotice] = useState<string | null>(null);
  const [statusTab, setStatusTab] = useState<"simulator" | "multiplayer" | "logs">("simulator");
  const [multiplayerInfo, setMultiplayerInfo] = useState<MultiplayerDebugInfo | null>(null);
  const [copiedLogs, setCopiedLogs] = useState(false);
  // Refs read inside the poll interval so the closure sees current values.
  const flightOngoingRef = useRef(false);
  const pollFailedRef = useRef(false);
  const pollInFlightRef = useRef(false);

  const handleBackToHistory = useCallback(() => {
    setView("history");
  }, []);

  const handleViewDetails = useCallback((flight: FlightSummary) => {
    setSelectedFlight(flight);
    setView("details");
  }, []);

  const handleCopyLogs = useCallback(() => {
    const text = logs.join("\n");
    navigator.clipboard.writeText(text)
      .then(() => {
        setCopiedLogs(true);
        setTimeout(() => setCopiedLogs(false), 2000);
      })
      .catch((err) => console.error("Failed to copy logs:", err));
  }, [logs]);

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
      setLogs((prevLogs) => {
        const next = [...prevLogs, event.payload];
        return next.length > MAX_UI_LOG_LINES ? next.slice(-MAX_UI_LOG_LINES) : next;
      });
    });

    const unlistenPhase = listen<string>("flight-phase-change", (event) => {
      setCurrentPhase(event.payload);
    });

    // Backend forces a logout when the service rejects our token (HTTP 401).
    const unlistenAuth = listen<string>("auth-logout", () => {
      setAuthNotice("Your ButterLog session expired. Reconnect with Discord in Settings to resume syncing and traffic injection.");
    });

    // Poll backend status: 200ms while a flight is being logged, 1s otherwise,
    // and not at all while the window is hidden in the tray.
    let pollTick = 0;
    const interval = window.setInterval(async () => {
      pollTick++;
      if (document.hidden) return;
      if (!flightOngoingRef.current && pollTick % 5 !== 0) return;
      // Skip if the previous poll is still in flight so slow backend calls don't
      // pile up overlapping invocations and saturate the command worker pool.
      if (pollInFlightRef.current) return;
      pollInFlightRef.current = true;
      try {
        const [m, connected, ongoing, sims, fid, mpInfo] = await Promise.all([
          invoke<FlightMetrics>("get_metrics"),
          invoke<boolean>("is_sim_connected"),
          invoke<boolean>("is_flight_ongoing"),
          invoke<string[]>("get_connected_sims"),
          invoke<string>("get_current_flight_id"),
          invoke<MultiplayerDebugInfo>("get_multiplayer_status")
        ]);
        setMetrics(m);
        setSimConnected(connected);
        setFlightOngoing(ongoing);
        setConnectedSims(sims);
        setCurrentFlightId(fid);
        setMultiplayerInfo(mpInfo);
        flightOngoingRef.current = ongoing;
        pollFailedRef.current = false;
      } catch (e) {
        // Log the first failure instead of spamming on every tick.
        if (!pollFailedRef.current) {
          pollFailedRef.current = true;
          console.error("Backend status poll failed:", e);
        }
      } finally {
        pollInFlightRef.current = false;
      }
    }, 200);

    return () => {
      unlistenLogs.then((f) => f());
      unlistenPhase.then((f) => f());
      unlistenAuth.then((f) => f());
      clearInterval(interval);
    };
  }, []);

  const getSimNameDisplay = () => {
    if (connectedSims.length === 0) return "SIM";
    return connectedSims.map(s => s.toUpperCase()).join(" + ");
  };

  const handleTrackClick = async () => {
    try {
      if (currentFlightId) {
        const summaries = await invoke<FlightSummary[]>("get_flight_summaries");
        const ongoing = summaries.find(s => s.filename.replace(".db", "") === currentFlightId);
        if (ongoing) {
          setSelectedFlight(ongoing);
          setView("details");
        }
      }
    } catch (e) {
      console.error("Failed to track flight:", e);
    }
  };

  const renderContent = () => {
    switch (view) {
      case "history":
        return <FlightLogs 
          currentFlightId={currentFlightId}
          onViewDetails={handleViewDetails}
        />;
      case "details":
        return selectedFlight ? (
          <FlightDetails flight={selectedFlight} currentFlightId={currentFlightId} onBack={handleBackToHistory} />
        ) : (
          <div>No flight selected</div>
        );
      case "aircraft":
        return <AircraftStats onViewDetails={handleViewDetails} />;
      case "settings":
        return <Settings />;
      case "status":
      default:
        return (
          <div className="status-view">
            <div className="status-tabs">
              <button 
                className={`status-tab ${statusTab === "simulator" ? "active" : ""}`}
                onClick={() => setStatusTab("simulator")}
              >
                Simulator Telemetry
              </button>
              <button 
                className={`status-tab ${statusTab === "multiplayer" ? "active" : ""}`}
                onClick={() => setStatusTab("multiplayer")}
              >
                Multiplayer Debugging
              </button>
              <button 
                className={`status-tab ${statusTab === "logs" ? "active" : ""}`}
                onClick={() => setStatusTab("logs")}
              >
                Backend Logs
              </button>
            </div>

            {statusTab === "simulator" && (
              <>
                {metrics && flightOngoing && (
                  <div className="metrics-display" style={{ textAlign: "left", marginBottom: "2rem" }}>
                    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
                        <h3>Flight Metrics</h3>
                        <div style={{ background: "#4caf50", color: "white", padding: "4px 12px", borderRadius: "20px", fontSize: "0.8rem", fontWeight: "bold" }}>
                            PHASE: {currentPhase.toUpperCase()}
                        </div>
                    </div>
                    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(200px, 1fr))", gap: "10px", background: "#14181a", padding: "1rem", borderRadius: "8px" }}>
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
                  <div className="status-box">
                    <div className="status-box-title" style={{ color: "#4caf50" }}>{getSimNameDisplay()} CONNECTED</div>
                    <div style={{ color: "#888" }}>Waiting for flight movement to start logging...</div>
                  </div>
                )}

                {!simConnected && (
                  <div className="status-box">
                    <div className="status-box-title" style={{ color: "#f44336" }}>DISCONNECTED</div>
                    <div style={{ color: "#888" }}>Start your flight simulator to begin logging.</div>
                  </div>
                )}
              </>
            )}

            {statusTab === "multiplayer" && (
              <div className="multiplayer-status" style={{ textAlign: "left" }}>
                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px", marginBottom: "2rem" }}>
                  <div className="panel" style={{ border: "1px solid #444" }}>
                    <h4 style={{ color: "#888", marginBottom: "0.5rem", fontSize: "0.9rem" }}>YOUR PUBLIC UDP ADDRESS (STUN)</h4>
                    <div style={{ fontSize: "1.4rem", fontFamily: "monospace", fontWeight: "bold" }}>
                      {multiplayerInfo?.publicAddress || "Discovering..."}
                    </div>
                  </div>
                  <div className="panel" style={{ border: "1px solid #444" }}>
                    <h4 style={{ color: "#888", marginBottom: "0.5rem", fontSize: "0.9rem" }}>ACTIVE MULTIPLAYER PEERS</h4>
                    <div style={{ fontSize: "1.4rem", fontWeight: "bold" }}>
                      {multiplayerInfo?.peers.length || 0} active
                    </div>
                  </div>
                </div>

                {multiplayerInfo && multiplayerInfo.peers.length > 0 && (
                  <div style={{ marginBottom: "2rem" }}>
                    <h4 style={{ color: "#888", borderBottom: "1px solid #333", paddingBottom: "4px", marginBottom: "12px" }}>Peer Connections</h4>
                    <div style={{ background: "#0e1113", padding: "1rem", borderRadius: "8px", display: "flex", flexWrap: "wrap", gap: "10px" }}>
                      {multiplayerInfo.peers.map((peer, idx) => (
                        <div key={idx} style={{ background: "#333", padding: "4px 10px", borderRadius: "4px", fontFamily: "monospace", fontSize: "0.9rem" }}>
                          🔗 {peer}
                        </div>
                      ))}
                    </div>
                  </div>
                )}

                <div>
                  <h3 style={{ marginBottom: "1rem" }}>Tracked Players & Traffic ({multiplayerInfo?.trackedAircrafts.length || 0})</h3>
                  {!multiplayerInfo || multiplayerInfo.trackedAircrafts.length === 0 ? (
                    <div style={{ background: "#14181a", padding: "2.5rem", borderRadius: "8px", textAlign: "center", border: "1px dashed #444" }}>
                      <span style={{ fontSize: "2rem" }}>✈️</span>
                      <p style={{ color: "#888", marginTop: "10px", marginBottom: 0 }}>
                        No multiplayer traffic detected. Other players flying with ButterLog will appear here as we receive their live UDP telemetry.
                      </p>
                    </div>
                  ) : (
                    <div className="mp-grid">
                      {multiplayerInfo.trackedAircrafts.map((ac) => {
                        const isStale = ac.lastSeenSecondsAgo > 5;
                        const isCritical = ac.lastSeenSecondsAgo > 10;
                        const lastSeenClass = isCritical ? "critical" : isStale ? "warn" : "";
                        
                        return (
                          <div key={ac.id} className="mp-card">
                            <div className="mp-card-header">
                              <span className="mp-callsign" title={ac.id}>{ac.username || ac.id}</span>
                              <span className={`mp-last-seen ${lastSeenClass}`}>
                                {ac.lastSeenSecondsAgo === 0 ? "just now" : `${ac.lastSeenSecondsAgo}s ago`}
                              </span>
                            </div>
                            <div className="mp-spec-grid">
                              <div style={{ gridColumn: "span 3" }}>
                                <span className="mp-label">Aircraft:</span>
                                <div style={{ fontWeight: "600", textOverflow: "ellipsis", overflow: "hidden", whiteSpace: "nowrap" }} title={ac.aircraft}>{ac.aircraft}</div>
                              </div>
                              <div style={{ gridColumn: "span 3" }}>
                                <span className="mp-label">Address:</span>
                                <div style={{ color: "#aaa", fontFamily: "monospace", fontSize: "0.85rem", textOverflow: "ellipsis", overflow: "hidden", whiteSpace: "nowrap" }} title={ac.id}>
                                  {ac.address || ac.id}{ac.localAddress ? ` · LAN ${ac.localAddress}` : ""}
                                </div>
                              </div>
                              <div>
                                <span className="mp-label">Deduced ICAO:</span>
                                <div style={{ color: "#aaa" }} title={ac.atcModel ? `sim ATC MODEL: ${ac.atcModel}` : undefined}>
                                  {ac.resolvedIcao || ac.atcModel || "—"}
                                </div>
                              </div>
                              <div>
                                <span className="mp-label">Airline:</span>
                                <div style={{ color: "#aaa", textOverflow: "ellipsis", overflow: "hidden", whiteSpace: "nowrap" }} title={ac.resolvedAirline}>
                                  {ac.resolvedAirlineIcao
                                    ? `${ac.resolvedAirlineIcao}${ac.resolvedAirline ? ` (${ac.resolvedAirline})` : ""}`
                                    : "—"}
                                </div>
                              </div>
                              <div>
                                <span className="mp-label">Engine/Class:</span>
                                <div style={{ color: "#aaa" }}>
                                  {ac.numEngines}x {ac.engineType} {ac.category}
                                </div>
                              </div>
                              <div style={{ gridColumn: "span 3" }}>
                                <span className="mp-label">Livery:</span>
                                <div style={{ color: "#aaa", textOverflow: "ellipsis", overflow: "hidden", whiteSpace: "nowrap" }} title={ac.livery}>
                                  {ac.livery || "—"}
                                </div>
                              </div>
                              <div style={{ gridColumn: "span 3" }}>
                                <span className="mp-label">Chosen model:</span>
                                <div style={{ color: "#7fd1b9", fontWeight: "600", textOverflow: "ellipsis", overflow: "hidden", whiteSpace: "nowrap" }} title={ac.chosenModel}>
                                  {ac.chosenModel || "— (deferred to sim plugin)"}
                                </div>
                              </div>
                              <div style={{ gridColumn: "span 3" }}>
                                <span className="mp-label">State:</span>
                                <div style={{ color: "#aaa" }}>
                                  {ac.onGround ? "On ground" : "Airborne"} · Gear {Math.round(ac.gearRatio * 100)}% · Lights: {
                                    [
                                      ac.navLights && "NAV",
                                      ac.beaconLights && "BCN",
                                      ac.strobeLights && "STRB",
                                      ac.taxiLights && "TAXI",
                                      ac.landingLights && "LDG",
                                    ].filter(Boolean).join(" ") || "—"
                                  }
                                </div>
                              </div>
                            </div>
                            <div className="mp-telemetry-box">
                              <span className="mp-label">LAT:</span>
                              <span className="mp-value">{ac.latitude.toFixed(5)}</span>
                              
                              <span className="mp-label">LON:</span>
                              <span className="mp-value">{ac.longitude.toFixed(5)}</span>
                              
                              <span className="mp-label">ALT MSL:</span>
                              <span className="mp-value">{Math.round(ac.gpsAltitudeMsl)} ft</span>
                              
                              <span className="mp-label">ALT AGL:</span>
                              <span className="mp-value">{Math.round(ac.indicatedAltitude)} ft</span>
                              
                              <span className="mp-label">SPEED:</span>
                              <span className="mp-value">{Math.round(ac.groundSpeed)} kt</span>
                              
                              <span className="mp-label">HDG/TRK:</span>
                              <span className="mp-value">{Math.round(ac.heading)}° / {Math.round(ac.track)}°</span>
                              
                              <span className="mp-label">PITCH/ROLL:</span>
                              <span className="mp-value">{ac.pitchAngle.toFixed(1)}° / {ac.rollAngle.toFixed(1)}°</span>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              </div>
            )}

            {statusTab === "logs" && (
              <div className="logs-container" style={{ textAlign: "left" }}>
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "1rem" }}>
                  <h3 style={{ margin: 0 }}>Backend Logs</h3>
                  <button
                    onClick={handleCopyLogs}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: "6px",
                      background: copiedLogs ? "#4caf50" : "#14181a",
                      color: copiedLogs ? "white" : "#eee",
                      border: "1px solid #444",
                      padding: "6px 12px",
                      borderRadius: "6px",
                      fontSize: "0.85rem",
                      fontWeight: "bold",
                      cursor: "pointer",
                      transition: "all 0.25s ease"
                    }}
                  >
                    {copiedLogs ? (
                      <>
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                          <polyline points="20 6 9 17 4 12"></polyline>
                        </svg>
                        Copied!
                      </>
                    ) : (
                      <>
                        <Icons.Copy />
                        Copy Logs
                      </>
                    )}
                  </button>
                </div>
                <div style={{ background: "#0e1113", padding: "1rem", borderRadius: "8px", maxHeight: "400px", overflowY: "auto" }}>
                  {logs.length === 0 ? <p style={{ color: "#888" }}>No logs yet...</p> : null}
                  {logs.map((log, index) => (
                    <div key={index} style={{ fontFamily: "monospace", fontSize: "0.9rem", color: "#4caf50", marginBottom: "4px" }}>{log}</div>
                  ))}
                </div>
              </div>
            )}
          </div>
        );
    }
  };

  return (
    <div className="app-container">
      {authNotice && (
        <div style={{
          display: "flex", alignItems: "center", justifyContent: "space-between", gap: "1rem",
          background: "rgba(243,139,168,0.12)", color: "#f38ba8",
          borderBottom: "1px solid rgba(243,139,168,0.3)", padding: "0.6rem 1rem", fontSize: "0.9rem"
        }}>
          <span>{authNotice}</span>
          <div style={{ display: "flex", gap: "0.5rem", flexShrink: 0 }}>
            <button
              onClick={() => { setView("settings"); setAuthNotice(null); }}
              style={{ background: "#f38ba8", color: "#11111b", border: "none", borderRadius: "6px", padding: "0.3rem 0.8rem", cursor: "pointer", fontWeight: "bold" }}
            >Open Settings</button>
            <button
              onClick={() => setAuthNotice(null)}
              style={{ background: "transparent", color: "#f38ba8", border: "1px solid rgba(243,139,168,0.4)", borderRadius: "6px", padding: "0.3rem 0.6rem", cursor: "pointer" }}
            >Dismiss</button>
          </div>
        </div>
      )}
      <div className="app-layout">
        <nav className="sidebar">
          <div className="sidebar-top">
            <div style={{ width: "100%", height: "60px", display: "flex", alignItems: "center", justifyContent: "center", marginBottom: "5px", borderBottom: "1px solid #444" }}>
              <img src="/icon.png" alt="Butterlog" style={{ width: "32px", height: "32px" }} />
            </div>
            <div 
              className={`sidebar-item ${view === 'history' || (view === 'details' && selectedFlight?.filename.replace(".db", "") !== currentFlightId) ? 'active' : ''}`} 
              onClick={() => {
                setView('history');
                setSelectedFlight(null);
              }}
              title="Logs"
            >
              <span className="icon"><Icons.Logs /></span>
            </div>
            <div 
              className={`sidebar-item ${view === 'details' && selectedFlight?.filename.replace(".db", "") === currentFlightId ? 'active' : ''} ${!currentFlightId ? 'disabled' : ''}`} 
              onClick={handleTrackClick}
              title="Track Active Flight"
              style={{ opacity: currentFlightId ? 1 : 0.4, cursor: currentFlightId ? 'pointer' : 'default' }}
            >
              <span className="icon"><Icons.Activity /></span>
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
