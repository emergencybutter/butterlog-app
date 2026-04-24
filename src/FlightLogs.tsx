import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";

interface FlightSummary {
    filename: string;
    startIcao: string;
    endIcao: string;
    startTime: string;
    endTime: string;
    durationMinutes: number;
    fileSizeBytes: number;
    aircraftTitle: string;
    aircraftType: string;
    aircraftModel: string;
    maxAltitude: number;
    maxGroundSpeed: number;
    fuelConsumed: number;
}

export function FlightLogs({ onViewDetails }: { onViewDetails: (flight: FlightSummary) => void }) {
    const [summaries, setSummaries] = useState<FlightSummary[]>([]);
    const [loading, setLoading] = useState(true);
    const [importing, setImporting] = useState(false);
    const [expandedIndex, setExpandedIndex] = useState<number | null>(null);

    const loadSummaries = () => {
        setLoading(true);
        invoke<FlightSummary[]>("get_flight_summaries")
            .then(setSummaries)
            .catch(console.error)
            .finally(() => setLoading(false));
    };

    useEffect(() => {
        loadSummaries();

        const unlisten = listen("flight-logs-updated", () => {
            loadSummaries();
        });

        return () => {
            unlisten.then(f => f());
        };
    }, []);

    const handleImport = async () => {
        try {
            const selected = await open({
                multiple: false,
                filters: [{ name: 'CSV', extensions: ['csv'] }]
            });

            if (selected && typeof selected === 'string') {
                setImporting(true);
                await invoke("import_flight_from_csv", { path: selected });
                // We don't call loadSummaries() here because the event listener will do it
            }

        } catch (e) {
            alert(`Import failed: ${e}`);
        } finally {
            setImporting(false);
        }
    };

    if (loading && !importing) return <div>Scanning logs...</div>;

    return (
        <div className="logs-view" style={{ textAlign: "left", padding: "1rem", maxWidth: "800px", margin: "0 auto" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
                <h2>Flight History</h2>
                <button 
                    onClick={handleImport} 
                    disabled={importing}
                    style={{ backgroundColor: "#2196f3" }}
                >
                    {importing ? "Importing..." : "Import G1000 Log (CSV)"}
                </button>
            </div>

            {importing && (
                <div style={{ 
                    background: "#2196f3", 
                    color: "white", 
                    padding: "10px 20px", 
                    borderRadius: "8px", 
                    marginBottom: "20px",
                    fontWeight: "bold",
                    display: "flex",
                    alignItems: "center",
                    gap: "10px"
                }}>
                    <span className="import-spinner">↻</span> Importing flight data...
                </div>
            )}

            {summaries.length === 0 ? (
                <p style={{ textAlign: "center", color: "#888" }}>No flight logs found.</p>
            ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
                    {summaries.map((s, i) => (
                        <div key={s.filename} style={{ background: "#2a2a2a", borderRadius: "8px", overflow: "hidden", border: "1px solid #444" }}>
                            <div 
                                onClick={() => setExpandedIndex(expandedIndex === i ? null : i)}
                                style={{ 
                                    padding: "1rem", 
                                    cursor: "pointer", 
                                    display: "flex", 
                                    justifyContent: "space-between",
                                    alignItems: "center",
                                    background: expandedIndex === i ? "#333" : "transparent"
                                }}
                            >
                                <div style={{ display: "flex", gap: "20px", alignItems: "center" }}>
                                    <div>
                                        <div style={{ fontSize: "0.7rem", color: "#888", marginBottom: "2px" }}>{s.aircraftTitle}</div>
                                        <span style={{ fontWeight: "bold", fontSize: "1.1rem", color: "#4caf50" }}>
                                            {s.startIcao} → {s.endIcao}
                                        </span>
                                    </div>
                                    <span style={{ color: "#aaa", fontSize: "0.9rem" }}>{s.startTime.split(' ')[0]}</span>
                                </div>
                                <div style={{ display: "flex", gap: "20px", alignItems: "center" }}>
                                    <span style={{ fontWeight: "bold" }}>{Math.floor(s.durationMinutes / 60)}h {s.durationMinutes % 60}m</span>
                                    <span>{expandedIndex === i ? "▲" : "▼"}</span>
                                </div>
                            </div>
                            
                            {expandedIndex === i && (
                                <div style={{ padding: "1rem", borderTop: "1px solid #444", background: "#1a1a1a", fontSize: "0.9rem" }}>
                                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px" }}>
                                        <div>
                                            <p><span style={{ color: "#888" }}>Departure:</span> {s.startTime}</p>
                                            <p><span style={{ color: "#888" }}>Arrival:</span> {s.endTime}</p>
                                            <p><span style={{ color: "#888" }}>Aircraft:</span> {s.aircraftTitle} ({s.aircraftModel})</p>
                                        </div>
                                        <div>
                                            <p><span style={{ color: "#888" }}>Max Altitude:</span> {s.maxAltitude.toFixed(0)} ft</p>
                                            <p><span style={{ color: "#888" }}>Max Speed:</span> {s.maxGroundSpeed.toFixed(0)} kt (GS)</p>
                                            <p><span style={{ color: "#888" }}>Fuel Consumed:</span> {s.fuelConsumed.toFixed(1)} gal</p>
                                        </div>
                                    </div>
                                    <div style={{ marginTop: "1rem", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                                        <span style={{ color: "#555", fontSize: "0.7rem" }}>
                                            {s.filename} ({(s.fileSizeBytes / 1024).toFixed(1)} KB)
                                        </span>
                                        <button 
                                            onClick={() => onViewDetails(s)}
                                            style={{ fontSize: "0.8rem" }}
                                        >
                                            View Detailed Log
                                        </button>
                                    </div>
                                </div>
                            )}
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
}
