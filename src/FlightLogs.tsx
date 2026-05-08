import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { FlightSummary } from "./models";

interface ImportProgress {
    state: 'parsing' | 'saving' | 'finalizing';
    current: number;
    total: number;
}

interface BatchImportStatus {
    totalFiles: number;
    completedFiles: number;
    currentFileName: string;
}

export function FlightLogs({ onViewDetails, currentFlightId }: { onViewDetails: (flight: FlightSummary) => void, currentFlightId?: string }) {
    const [summaries, setSummaries] = useState<FlightSummary[]>([]);
    const [loading, setLoading] = useState(true);
    const [importing, setImporting] = useState(false);
    const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);
    const [batchStatus, setBatchStatus] = useState<BatchImportStatus | null>(null);
    const [expandedIndex, setExpandedIndex] = useState<number | null>(null);
    const [showIncomplete, setShowIncomplete] = useState(false);

    const loadSummaries = () => {
        setLoading(true);
        invoke<FlightSummary[]>("get_flight_summaries")
            .then((res) => {
                console.log("[Debug] Summaries received from backend:", res.map(s => s.filename));
                console.log("[Debug] Current flight ID being logged:", currentFlightId);
                setSummaries(res);
            })
            .catch(console.error)
            .finally(() => {
                setLoading(false);
            });
    };

    useEffect(() => {
        loadSummaries();

        const unlistenUpdated = listen("flight-logs-updated", () => {
            if (!importing) {
                loadSummaries();
            }
        });

        const unlistenProgress = listen<ImportProgress>("import-progress", (event) => {
            setImportProgress(event.payload);
        });

        return () => {
            unlistenUpdated.then(f => f());
            unlistenProgress.then(f => f());
        };
    }, [importing]);

    const handleImport = async () => {
        try {
            const selected = await open({
                multiple: true,
                filters: [{ name: 'CSV', extensions: ['csv'] }]
            });

            if (selected) {
                const paths = Array.isArray(selected) ? selected : [selected];
                setImporting(true);
                setBatchStatus({
                    totalFiles: paths.length,
                    completedFiles: 0,
                    currentFileName: ""
                });

                for (let i = 0; i < paths.length; i++) {
                    const path = paths[i];
                    const fileName = path.split(/[\\/]/).pop() || "Unknown";
                    
                    setBatchStatus(prev => prev ? { ...prev, currentFileName: fileName } : null);
                    setImportProgress(null);
                    
                    await invoke("import_flight_from_csv", { path });
                    
                    setBatchStatus(prev => prev ? { ...prev, completedFiles: i + 1 } : null);
                }

                loadSummaries();
                setImporting(false);
                setBatchStatus(null);
                setImportProgress(null);
            }
        } catch (e) {
            alert(`Import failed: ${e}`);
            setImporting(false);
            setBatchStatus(null);
            setImportProgress(null);
        }
    };

    const filteredSummaries = summaries.filter(s => {
        if (showIncomplete) return true;
        return s.startIcao !== "Airborne" && s.endIcao !== "Airborne";
    });

    const renderProgressBar = (current: number, total: number, color: string = "#4caf50", height: string = "8px") => (
        <div style={{ width: "100%", height, background: "#1a1a1a", borderRadius: "4px", overflow: "hidden" }}>
            <div style={{ 
                width: `${Math.min(100, (current / (total || 1)) * 100)}%`, 
                height: "100%", 
                background: color,
                transition: "width 0.3s ease"
            }} />
        </div>
    );

    const isProcessing = importing;

    return (
        <div className="logs-view" style={{ textAlign: "left", padding: "1rem", maxWidth: "800px", margin: "0 auto" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "1rem" }}>
                <h2>Flight History</h2>
                <button 
                    onClick={handleImport} 
                    disabled={importing}
                    style={{ backgroundColor: importing ? "#444" : "#2196f3" }}
                >
                    {importing ? "Importing..." : "Import G1000 Log (CSV)"}
                </button>
            </div>

            {isProcessing ? (
                <div style={{ 
                    marginTop: "2rem",
                    padding: "3rem",
                    background: "#2a2a2a",
                    borderRadius: "12px",
                    border: "1px solid #444",
                    textAlign: "center",
                    boxShadow: "0 10px 30px rgba(0,0,0,0.5)"
                }}>
                    <div style={{ marginBottom: "2rem" }}>
                        <h3 style={{ margin: "0 0 10px 0" }}>Batch Import in Progress</h3>
                        <p style={{ color: "#aaa", fontSize: "0.9rem" }}>
                            File {batchStatus?.completedFiles} of {batchStatus?.totalFiles} processed
                        </p>
                    </div>

                    <div style={{ marginBottom: "2rem" }}>
                        <div style={{ display: "flex", justifyContent: "space-between", fontSize: "0.8rem", color: "#888", marginBottom: "8px" }}>
                            <span>Total Progress</span>
                            <span>{Math.round(((batchStatus?.completedFiles || 0) / (batchStatus?.totalFiles || 1)) * 100)}%</span>
                        </div>
                        {renderProgressBar(batchStatus?.completedFiles || 0, batchStatus?.totalFiles || 1)}
                    </div>

                    <div style={{ padding: "1.5rem", background: "#1a1a1a", borderRadius: "8px", border: "1px solid #333", textAlign: "left" }}>
                        <div style={{ fontSize: "0.8rem", color: "#4db8ff", fontWeight: "bold", marginBottom: "10px", textOverflow: "ellipsis", overflow: "hidden", whiteSpace: "nowrap" }}>
                            CURRENT: {batchStatus?.currentFileName}
                        </div>
                        
                        <div style={{ fontSize: "0.85rem", color: "#eee", marginBottom: "12px" }}>
                            {importProgress?.state === 'saving' ? '💾 Saving to flight database...' : 
                                importProgress?.state === 'finalizing' ? '📊 Analyzing flight dynamics...' : 
                                '📂 Parsing CSV data points...'}
                        </div>

                        {importProgress && renderProgressBar(importProgress.current, importProgress.total, "#2196f3", "4px")}
                        <div style={{ marginTop: "8px", textAlign: "right", fontSize: "0.75rem", color: "#666" }}>
                            {importProgress ? `${importProgress.current.toLocaleString()} / ${importProgress.total.toLocaleString()} rows` : "Initializing..."}
                        </div>
                    </div>
                </div>
            ) : loading ? (
                <div style={{ padding: "4rem", textAlign: "center", color: "#888" }}>
                    <div className="import-spinner" style={{ fontSize: "2rem", marginBottom: "1rem" }}>🔄</div>
                    <div>Scanning logs...</div>
                </div>
            ) : (
                <>
                    <div style={{ marginBottom: "1.5rem" }}>
                        <label style={{ display: "flex", alignItems: "center", gap: "6px", fontSize: "0.9rem", color: "#aaa", cursor: "pointer" }}>
                            <input 
                                type="checkbox" 
                                checked={showIncomplete} 
                                onChange={(e) => setShowIncomplete(e.target.checked)}
                                style={{ margin: 0, width: "auto", cursor: "pointer" }}
                            />
                            Show incomplete flights (airborne start/end)
                        </label>
                    </div>

                    {filteredSummaries.length === 0 ? (
                        <p style={{ textAlign: "center", color: "#888" }}>No flight logs found.</p>
                    ) : (
                        <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
                            {filteredSummaries.map((s, i) => {
                                const isCurrent = currentFlightId && s.filename.replace(".db", "") === currentFlightId;
                                return (
                                <div key={s.filename} style={{ 
                                    background: isCurrent ? "#1b3a24" : "#2a2a2a", 
                                    borderRadius: "8px", 
                                    overflow: "hidden", 
                                    border: isCurrent ? "1px solid #4caf50" : "1px solid #444",
                                    boxShadow: isCurrent ? "0 0 10px rgba(76, 175, 80, 0.2)" : "none"
                                }}>
                                    <div 
                                        onClick={() => setExpandedIndex(expandedIndex === i ? null : i)}
                                        style={{ 
                                            padding: "1rem", 
                                            cursor: "pointer", 
                                            display: "flex", 
                                            justifyContent: "space-between",
                                            alignItems: "center",
                                            background: expandedIndex === i ? (isCurrent ? "#244d31" : "#333") : "transparent"
                                        }}
                                    >
                                        <div style={{ display: "flex", gap: "20px", alignItems: "center" }}>
                                            <div>
                                                <div style={{ display: "flex", alignItems: "center", gap: "8px", marginBottom: "2px" }}>
                                                    <div style={{ fontSize: "0.7rem", color: isCurrent ? "#81c784" : "#888" }}>{s.aircraftTitle}</div>
                                                    {isCurrent && (
                                                        <div style={{ background: "#4caf50", color: "white", fontSize: "0.6rem", fontWeight: "bold", padding: "1px 6px", borderRadius: "10px", letterSpacing: "0.5px" }}>
                                                            LOGGING NOW
                                                        </div>
                                                    )}
                                                </div>
                                                <div style={{ fontWeight: "bold", fontSize: "1.1rem", color: isCurrent ? "#fff" : "#eee" }}>
                                                    {s.startIcao} → {s.endIcao}
                                                </div>
                                                <div style={{ fontSize: "0.7rem", color: isCurrent ? "#a5d6a7" : "#aaa" }}>
                                                    {s.startAirportName} to {s.endAirportName}
                                                </div>
                                            </div>
                                        </div>
                                        <div style={{ display: "flex", gap: "20px", alignItems: "center" }}>
                                            <div style={{ display: "flex", flexDirection: "column", gap: "2px", textAlign: "right" }}>
                                                <div style={{ color: isCurrent ? "#a5d6a7" : "#aaa", fontSize: "0.9rem" }}>{s.startTime.split(' ')[0]} {s.startTime.split(' ')[1].substring(0, 5)}</div>
                                                <div style={{ fontWeight: "bold", color: isCurrent ? "#81c784" : "#888", fontSize: "0.8rem", display: "flex", gap: "10px", justifyContent: "flex-end", alignItems: "center" }}>
                                                    {s.screenshotCount > 0 && (
                                                        <span title={`${s.screenshotCount} screenshots`} style={{ opacity: 0.8 }}>
                                                            📷 {s.screenshotCount}
                                                        </span>
                                                    )}
                                                    <span>{Math.floor(s.durationMinutes / 60)}h {s.durationMinutes % 60}m</span>
                                                </div>
                                            </div>
                                            <span style={{ color: isCurrent ? "#4caf50" : "inherit" }}>{expandedIndex === i ? "▲" : "▼"}</span>
                                        </div>
                                    </div>
                                    
                                    {expandedIndex === i && (
                                        <div style={{ padding: "1rem", borderTop: isCurrent ? "1px solid #2e5c3e" : "1px solid #444", background: isCurrent ? "#122618" : "#1a1a1a", fontSize: "0.9rem" }}>
                                            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px" }}>
                                                <div>
                                                    <p><span style={{ color: isCurrent ? "#81c784" : "#888" }}>Departure:</span> {s.startTime}</p>
                                                    <p><span style={{ color: isCurrent ? "#81c784" : "#888" }}>Arrival:</span> {isCurrent ? "Ongoing..." : s.endTime}</p>
                                                    <p><span style={{ color: isCurrent ? "#81c784" : "#888" }}>Aircraft:</span> {s.aircraftTitle}</p>
                                                </div>
                                                <div>
                                                    <p><span style={{ color: isCurrent ? "#81c784" : "#888" }}>Max Altitude:</span> {s.maxAltitude.toFixed(0)} ft</p>
                                                    <p><span style={{ color: isCurrent ? "#81c784" : "#888" }}>Max Speed:</span> {s.maxGroundSpeed.toFixed(0)} kt (GS)</p>
                                                    <p><span style={{ color: isCurrent ? "#81c784" : "#888" }}>Fuel Consumed:</span> {s.fuelConsumed.toFixed(1)} gal</p>
                                                </div>
                                            </div>
                                            <div style={{ marginTop: "1rem", display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                                                <span style={{ color: isCurrent ? "#4caf50" : "#555", fontSize: "0.7rem", opacity: isCurrent ? 0.8 : 1 }}>
                                                    {s.filename} ({(s.fileSizeBytes / 1024).toFixed(1)} KB)
                                                </span>
                                                <button 
                                                    onClick={() => onViewDetails(s)}
                                                    style={{ fontSize: "0.8rem", backgroundColor: isCurrent ? "#4caf50" : "" }}
                                                >
                                                    {isCurrent ? "View Live Data" : "View Detailed Log"}
                                                </button>
                                            </div>
                                        </div>
                                    )}
                                </div>
                            );
                            })}
                        </div>
                    )}
                </>
            )}
        </div>
    );
}
