import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface FlightSummary {
    filename: string;
    startIcao: string;
    endIcao: string;
    startTime: string;
    endTime: string;
    durationMinutes: number;
    fileSizeBytes: number;
}

export function FlightLogs({ onBack }: { onBack: () => void }) {
    const [summaries, setSummaries] = useState<FlightSummary[]>([]);
    const [loading, setLoading] = useState(true);
    const [expandedIndex, setExpandedIndex] = useState<number | null>(null);

    useEffect(() => {
        invoke<FlightSummary[]>("get_flight_summaries")
            .then(setSummaries)
            .finally(() => setLoading(false));
    }, []);

    if (loading) return <div>Scanning logs...</div>;

    return (
        <div className="logs-view" style={{ textAlign: "left", padding: "1rem", maxWidth: "800px", margin: "0 auto" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
                <h2>Flight History</h2>
                <button onClick={onBack}>Back to Dashboard</button>
            </div>

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
                                    <span style={{ fontWeight: "bold", fontSize: "1.1rem", color: "#4caf50" }}>
                                        {s.startIcao} → {s.endIcao}
                                    </span>
                                    <span style={{ color: "#aaa", fontSize: "0.9rem" }}>{s.startTime.split(' ')[0]}</span>
                                </div>
                                <div style={{ display: "flex", gap: "20px", alignItems: "center" }}>
                                    <span style={{ fontWeight: "bold" }}>{Math.floor(s.durationMinutes / 60)}h {s.durationMinutes % 60}m</span>
                                    <span>{expandedIndex === i ? "▲" : "▼"}</span>
                                </div>
                            </div>
                            
                            {expandedIndex === i && (
                                <div style={{ padding: "1rem", borderTop: "1px solid #444", background: "#1a1a1a", fontSize: "0.9rem" }}>
                                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "10px" }}>
                                        <div>
                                            <p><span style={{ color: "#888" }}>Departure:</span> {s.startTime}</p>
                                            <p><span style={{ color: "#888" }}>Arrival:</span> {s.endTime}</p>
                                        </div>
                                        <div>
                                            <p><span style={{ color: "#888" }}>File:</span> {s.filename}</p>
                                            <p><span style={{ color: "#888" }}>Size:</span> {(s.fileSizeBytes / 1024).toFixed(1)} KB</p>
                                        </div>
                                    </div>
                                    <div style={{ marginTop: "1rem", textAlign: "right" }}>
                                        <button disabled style={{ fontSize: "0.8rem", opacity: 0.5 }}>View Detailed Log (Coming Soon)</button>
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
