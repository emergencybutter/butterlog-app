import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { MultiplayerDebugInfo } from "../models";

/// Multiplayer debugging: what it was as a Status tab, polled once a second while shown.
export function MultiplayerPage() {
  const [multiplayerInfo, setMultiplayerInfo] = useState<MultiplayerDebugInfo | null>(null);

  useEffect(() => {
    let busy = false;
    const poll = async () => {
      if (busy || document.hidden) return;
      busy = true;
      try { setMultiplayerInfo(await invoke<MultiplayerDebugInfo>("get_multiplayer_status")); }
      catch (e) { console.error("Multiplayer status failed:", e); }
      finally { busy = false; }
    };
    poll();
    const t = window.setInterval(poll, 1000);
    return () => clearInterval(t);
  }, []);

  return (
    <div className="page">
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
    </div>
  );
}
