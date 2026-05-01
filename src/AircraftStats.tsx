import { useState, useEffect } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";

interface AircraftStats {
    aircraftType: string;
    totalHoursAll: number;
    totalFuelAll: number;
    totalFlightsAll: number;
    totalHoursCompleted: number;
    totalFuelCompleted: number;
    totalFlightsCompleted: number;
    lastAirport: string;
}

interface Screenshot {
    path: string;
}

function AircraftThumbnail({ title }: { title: string }) {
    const [screenshot, setScreenshot] = useState<Screenshot | null>(null);

    useEffect(() => {
        invoke<Screenshot | null>("get_random_screenshot_for_aircraft", { aircraftTitle: title })
            .then(setScreenshot)
            .catch(console.error);
    }, [title]);

    if (!screenshot) return (
        <div style={{ 
            width: "150px", 
            height: "100px", 
            background: "#2a2a2a", 
            borderRadius: "4px", 
            display: "flex", 
            alignItems: "center", 
            justifyContent: "center",
            fontSize: "0.8rem",
            color: "#555"
        }}>
            NO IMG
        </div>
    );

    return (
        <img 
            src={convertFileSrc(screenshot.path)} 
            alt={title} 
            style={{ 
                width: "150px", 
                height: "100px", 
                objectFit: "cover", 
                borderRadius: "4px",
                border: "1px solid #444"
            }} 
        />
    );
}

export function AircraftStats() {
    const [stats, setStats] = useState<AircraftStats[]>([]);
    const [loading, setLoading] = useState(true);
    const [viewMode, setViewMode] = useState<"all" | "completed">("all");

    useEffect(() => {
        setLoading(true);
        invoke<AircraftStats[]>("get_aircraft_stats")
            .then(setStats)
            .catch(console.error)
            .finally(() => setLoading(false));
    }, []);

    if (loading) return <div>Loading aircraft statistics...</div>;

    return (
        <div className="stats-view" style={{ textAlign: "left", padding: "1rem", maxWidth: "1000px", margin: "0 auto" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
                <h2>Aircraft Statistics</h2>
                <div style={{ display: "flex", gap: "10px", background: "#2a2a2a", padding: "4px", borderRadius: "8px" }}>
                    <button 
                        onClick={() => setViewMode("all")}
                        style={{ 
                            background: viewMode === "all" ? "#4caf50" : "transparent",
                            border: "none",
                            fontSize: "0.8rem",
                            padding: "6px 12px"
                        }}
                    >
                        All Flights
                    </button>
                    <button 
                        onClick={() => setViewMode("completed")}
                        style={{ 
                            background: viewMode === "completed" ? "#4caf50" : "transparent",
                            border: "none",
                            fontSize: "0.8rem",
                            padding: "6px 12px"
                        }}
                    >
                        Completed Only
                    </button>
                </div>
            </div>

            {stats.length === 0 ? (
                <p style={{ textAlign: "center", color: "#888" }}>No aircraft data tracked yet.</p>
            ) : (
                <table style={{ width: "100%", borderCollapse: "collapse", background: "#1a1a1a", borderRadius: "8px", overflow: "hidden" }}>
                    <thead>
                        <tr style={{ background: "#2a2a2a", textAlign: "left" }}>
                            <th style={{ padding: "12px", width: "80px" }}></th>
                            <th style={{ padding: "12px" }}>Aircraft Type</th>
                            <th style={{ padding: "12px" }}>Flights</th>
                            <th style={{ padding: "12px" }}>Total Hours</th>
                            <th style={{ padding: "12px" }}>Fuel Used (gal)</th>
                            <th style={{ padding: "12px" }}>Last Seen At</th>
                        </tr>
                    </thead>
                    <tbody>
                        {stats.map((s, i) => (
                            <tr key={i} style={{ borderBottom: "1px solid #333" }}>
                                <td style={{ padding: "12px" }}>
                                    <AircraftThumbnail title={s.aircraftType} />
                                </td>
                                <td style={{ padding: "12px", fontWeight: "bold" }}>{s.aircraftType}</td>
                                <td style={{ padding: "12px" }}>
                                    {viewMode === "all" ? s.totalFlightsAll : s.totalFlightsCompleted}
                                </td>
                                <td style={{ padding: "12px" }}>
                                    {(viewMode === "all" ? s.totalHoursAll : s.totalHoursCompleted).toFixed(1)}h
                                </td>
                                <td style={{ padding: "12px" }}>
                                    {(viewMode === "all" ? s.totalFuelAll : s.totalFuelCompleted).toFixed(1)}
                                </td>
                                <td style={{ padding: "12px", color: "#4db8ff" }}>{s.lastAirport}</td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            )}
        </div>
    );
}
