import { useState, useEffect, useMemo } from "react";
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

interface FlightEvent {
    timestamp: string;
    eventType: 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent';
    latitude: number;
    longitude: number;
}

interface FlightSummary {
    filename: string;
    startIcao: string;
    startAirportName: string;
    endIcao: string;
    endAirportName: string;
    startTime: string;
    endTime: string;
    durationMinutes: number;
    aircraftTitle: string;
    maxAltitude: number;
    maxGroundSpeed: number;
    fuelConsumed: number;
    events: FlightEvent[];
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

export function AircraftStats({ onViewDetails }: { onViewDetails: (f: FlightSummary) => void }) {
    const [stats, setStats] = useState<AircraftStats[]>([]);
    const [flights, setFlights] = useState<FlightSummary[]>([]);
    const [loading, setLoading] = useState(true);
    const [viewMode, setViewMode] = useState<"all" | "completed">("all");
    const [expandedAircraft, setExpandedAircraft] = useState<string | null>(null);

    useEffect(() => {
        setLoading(true);
        Promise.all([
            invoke<AircraftStats[]>("get_aircraft_stats"),
            invoke<FlightSummary[]>("get_flight_summaries")
        ])
            .then(([s, f]) => {
                setStats(s);
                setFlights(f);
            })
            .catch(console.error)
            .finally(() => setLoading(false));
    }, []);

    const aircraftFlights = useMemo(() => {
        const map: Record<string, FlightSummary[]> = {};
        flights.forEach(f => {
            if (!map[f.aircraftTitle]) map[f.aircraftTitle] = [];
            map[f.aircraftTitle].push(f);
        });
        // Sort each list by startTime descending
        Object.keys(map).forEach(key => {
            map[key].sort((a, b) => b.startTime.localeCompare(a.startTime));
        });
        return map;
    }, [flights]);

    if (loading) return <div>Loading aircraft statistics...</div>;

    return (
        <div className="stats-view" style={{ textAlign: "left", padding: "1rem", maxWidth: "1200px", margin: "0 auto" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
                <h2>Aircraft Statistics</h2>
                <div style={{ display: "flex", gap: "10px", background: "#2a2a2a", padding: "4px", borderRadius: "8px" }}>
                    <button 
                        onClick={() => setViewMode("all")}
                        style={{ 
                            background: viewMode === "all" ? "#4caf50" : "transparent",
                            border: "none",
                            fontSize: "0.8rem",
                            padding: "6px 12px",
                            cursor: "pointer"
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
                            padding: "6px 12px",
                            cursor: "pointer"
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
                            <th style={{ padding: "12px" }}></th>
                        </tr>
                    </thead>
                    <tbody>
                        {stats.map((s, i) => (
                            <>
                                <tr key={i} style={{ borderBottom: "1px solid #333", cursor: "pointer" }} onClick={() => setExpandedAircraft(expandedAircraft === s.aircraftType ? null : s.aircraftType)}>
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
                                    <td style={{ padding: "12px", textAlign: "right" }}>
                                        <span style={{ fontSize: "1.2rem", color: "#666" }}>
                                            {expandedAircraft === s.aircraftType ? "▲" : "▼"}
                                        </span>
                                    </td>
                                </tr>
                                {expandedAircraft === s.aircraftType && (
                                    <tr>
                                        <td colSpan={7} style={{ padding: "0 12px 20px 12px", background: "#141414" }}>
                                            <div style={{ marginTop: "10px", padding: "10px", background: "#111", borderRadius: "4px", border: "1px solid #222" }}>
                                                <h4 style={{ margin: "0 0 10px 0", color: "#888", fontSize: "0.9rem" }}>Recent Flights</h4>
                                                <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
                                                    {(aircraftFlights[s.aircraftType] || []).map((f, fi) => (
                                                        <div key={fi} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "8px 12px", background: "#1a1a1a", borderRadius: "4px", border: "1px solid #2a2a2a" }}>
                                                            <div>
                                                                <span style={{ fontWeight: "bold", marginRight: "15px" }}>{f.startTime.split(' ')[0]}</span>
                                                                <span style={{ color: "#4db8ff", marginRight: "15px" }}>{f.startIcao} → {f.endIcao}</span>
                                                                <span style={{ color: "#888", fontSize: "0.85rem" }}>{f.durationMinutes} min | {(f.fuelConsumed).toFixed(1)} gal</span>
                                                            </div>
                                                            <button 
                                                                onClick={(e) => {
                                                                    e.stopPropagation();
                                                                    onViewDetails(f);
                                                                }}
                                                                style={{
                                                                    padding: "4px 12px",
                                                                    background: "#333",
                                                                    border: "1px solid #444",
                                                                    borderRadius: "4px",
                                                                    fontSize: "0.8rem",
                                                                    cursor: "pointer"
                                                                }}
                                                            >
                                                                Details
                                                            </button>
                                                        </div>
                                                    ))}
                                                    {(!aircraftFlights[s.aircraftType] || aircraftFlights[s.aircraftType].length === 0) && (
                                                        <div style={{ padding: "10px", color: "#555", fontStyle: "italic", fontSize: "0.85rem" }}>No detailed flight logs found for this aircraft.</div>
                                                    )}
                                                </div>
                                            </div>
                                        </td>
                                    </tr>
                                )}
                            </>
                        ))}
                    </tbody>
                </table>
            )}
        </div>
    );
}
