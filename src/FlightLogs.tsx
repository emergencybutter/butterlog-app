import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";

interface FlightSummary {
    filename: string;
    startIcao: string;
    startAirportName: string;
    endIcao: string;
    endAirportName: string;
    startTime: string;
    endTime: string;
    durationMinutes: number;
    fileSizeBytes: number;
    aircraftTitle: string;
    maxAltitude: number;
    maxGroundSpeed: number;
    fuelConsumed: number;
    events: any[];
}

interface ImportProgress {
    state: 'parsing' | 'saving' | 'finalizing';
    current: number;
    total: number;
}

export function FlightLogs({ onViewDetails }: { onViewDetails: (flight: FlightSummary) => void }) {
    const [summaries, setSummaries] = useState<FlightSummary[]>([]);
    const [loading, setLoading] = useState(true);
    const [importing, setImporting] = useState(false);
    const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);
    const [expandedIndex, setExpandedIndex] = useState<number | null>(null);
    const [showIncomplete, setShowIncomplete] = useState(false);

    const loadSummaries = () => {
        setLoading(true);
        invoke<FlightSummary[]>("get_flight_summaries")
            .then(setSummaries)
            .catch(console.error)
            .finally(() => setLoading(false));
    };

    useEffect(() => {
        loadSummaries();

        const unlistenUpdated = listen("flight-logs-updated", () => {
            loadSummaries();
            setImporting(false);
            setImportProgress(null);
        });

        const unlistenProgress = listen<ImportProgress>("import-progress", (event) => {
            setImportProgress(event.payload);
        });

        return () => {
            unlistenUpdated.then(f => f());
            unlistenProgress.then(f => f());
        };
    }, []);

    const handleImport = async () => {
        try {
            const selected = await open({
                multiple: true,
                filters: [{ name: 'CSV', extensions: ['csv'] }]
            });

            if (selected && Array.isArray(selected)) {
                setImporting(true);
                for (let i = 0; i < selected.length; i++) {
                    const path = selected[i];
                    setImportProgress(null); // Reset for each file
                    // We can add a "bulk" progress state if we want, but per-file is good
                    await invoke("import_flight_from_csv", { path });
                }
                // Explicitly refresh after all success
                loadSummaries();
                setImporting(false);
                setImportProgress(null);
            } else if (selected && typeof selected === 'string') {
                // Fallback for single selection if multiple: true still returns a string in some envs
                setImporting(true);
                setImportProgress(null);
                await invoke("import_flight_from_csv", { path: selected });
                loadSummaries();
                setImporting(false);
                setImportProgress(null);
            }

        } catch (e) {
            alert(`Import failed: ${e}`);
            setImporting(false);
            setImportProgress(null);
        }
    };

    if (loading && !importing) return <div>Scanning logs...</div>;

    const filteredSummaries = summaries.filter(s => {
        if (showIncomplete) return true;
        return s.startIcao !== "Airborne" && s.endIcao !== "Airborne";
    });

    return (
        <div className="logs-view" style={{ textAlign: "left", padding: "1rem", maxWidth: "800px", margin: "0 auto" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "1rem" }}>
                <h2>Flight History</h2>
                <button 
                    onClick={handleImport} 
                    disabled={importing}
                    style={{ backgroundColor: "#2196f3" }}
                >
                    {importing ? "Importing..." : "Import G1000 Log (CSV)"}
                </button>
            </div>

            <div style={{ marginBottom: "1.5rem", display: "flex", alignItems: "center", gap: "8px" }}>
                <input 
                    type="checkbox" 
                    id="showIncomplete" 
                    checked={showIncomplete} 
                    onChange={(e) => setShowIncomplete(e.target.checked)} 
                />
                <label htmlFor="showIncomplete" style={{ fontSize: "0.9rem", color: "#aaa", cursor: "pointer" }}>
                    Show incomplete flights (airborne start/end)
                </label>
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
                    <span className="import-spinner">↻</span> 
                    {importProgress?.state === 'saving' ? 'Saving to disk...' : 
                     importProgress?.state === 'finalizing' ? 'Analyzing flight data...' : 
                     'Ingesting CSV data...'} 
                    {importProgress ? ` (${importProgress.current.toLocaleString()} / ${importProgress.total.toLocaleString()} rows)` : " (calculating...)"}
                </div>
            )}

            {filteredSummaries.length === 0 ? (
                <p style={{ textAlign: "center", color: "#888" }}>No flight logs found.</p>
            ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
                    {filteredSummaries.map((s, i) => (
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
                                        <div style={{ fontWeight: "bold", fontSize: "1.1rem", color: "#4caf50" }}>
                                            {s.startIcao} → {s.endIcao}
                                        </div>
                                        <div style={{ fontSize: "0.7rem", color: "#aaa" }}>
                                            {s.startAirportName} to {s.endAirportName}
                                        </div>
                                    </div>
                                </div>
                                <div style={{ display: "flex", gap: "20px", alignItems: "center" }}>
                                    <div style={{ display: "flex", flexDirection: "column", gap: "2px", textAlign: "right" }}>
                                        <div style={{ color: "#aaa", fontSize: "0.9rem" }}>{s.startTime.split(' ')[0]} {s.startTime.split(' ')[1].substring(0, 5)}</div>
                                        <div style={{ fontWeight: "bold", color: "#888", fontSize: "0.8rem" }}>{Math.floor(s.durationMinutes / 60)}h {s.durationMinutes % 60}m</div>
                                    </div>
                                    <span>{expandedIndex === i ? "▲" : "▼"}</span>
                                </div>
                            </div>
                            
                            {expandedIndex === i && (
                                <div style={{ padding: "1rem", borderTop: "1px solid #444", background: "#1a1a1a", fontSize: "0.9rem" }}>
                                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px" }}>
                                        <div>
                                            <p><span style={{ color: "#888" }}>Departure:</span> {s.startTime}</p>
                                            <p><span style={{ color: "#888" }}>Arrival:</span> {s.endTime}</p>
                                            <p><span style={{ color: "#888" }}>Aircraft:</span> {s.aircraftTitle}</p>
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
