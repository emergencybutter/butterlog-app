import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { 
    LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer, AreaChart, Area, ReferenceLine, Label
} from 'recharts';

interface FlightMetrics {
  latitude: number;
  longitude: number;
  alt_msl: number;
  ias: number;
  gnd_spd: number;
  v_spd: number;
  pitch: number;
  roll: number;
  hdg: number;
  sim_on_ground: number;
}

interface FlightLogRow {
  timestamp: string;
  metrics: FlightMetrics;
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

interface Runway {
    airport_ident: string;
    length_ft: number | null;
    width_ft: number | null;
    le_ident: string | null;
    le_latitude_deg: number | null;
    le_longitude_deg: number | null;
    he_ident: string | null;
    he_latitude_deg: number | null;
    he_longitude_deg: number | null;
}

interface TrajectoryPoint {
    lat: number;
    lon: number;
    onGround: boolean;
    isEvent?: 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent';
}

function RunwayMap({ runways, icao, trajectory, title }: { runways: Runway[], icao: string, trajectory: TrajectoryPoint[], title: string }) {
    if (runways.length === 0 && trajectory.length === 0) return <div style={{ height: 300, display: "flex", alignItems: "center", justifyContent: "center", border: "1px solid #333", borderRadius: "8px" }}>No data for {icao}</div>;

    const validRunways = runways.filter(r => 
        r.le_latitude_deg !== null && r.le_longitude_deg !== null && 
        r.he_latitude_deg !== null && r.he_longitude_deg !== null
    );

    // Determine bounding box
    const rLats = validRunways.flatMap(r => [r.le_latitude_deg!, r.he_latitude_deg!]);
    const rLons = validRunways.flatMap(r => [r.le_longitude_deg!, r.he_longitude_deg!]);
    const tLats = trajectory.map(p => p.lat);
    const tLons = trajectory.map(p => p.lon);
    
    const allLats = [...rLats, ...tLats];
    const allLons = [...rLons, ...tLons];

    if (allLats.length === 0) return <div style={{ height: 300, display: "flex", alignItems: "center", justifyContent: "center" }}>No map data</div>;

    const minLat = Math.min(...allLats);
    const maxLat = Math.max(...allLats);
    const minLon = Math.min(...allLons);
    const maxLon = Math.max(...allLons);

    // Padding & Aspect Ratio
    const latDiff = Math.max(maxLat - minLat, 0.002);
    const lonDiff = Math.max(maxLon - minLon, 0.002);
    const padding = 0.25;

    const mapMinLat = minLat - latDiff * padding;
    const mapMaxLat = maxLat + latDiff * padding;
    const mapMinLon = minLon - lonDiff * padding;
    const mapMaxLon = maxLon + lonDiff * padding;

    const width = 350;
    const height = 350;

    const scaleX = (lon: number) => (lon - mapMinLon) / (mapMaxLon - mapMinLon) * width;
    const scaleY = (lat: number) => height - (lat - mapMinLat) / (mapMaxLat - mapMinLat) * height;

    const eventPoints = trajectory.filter(p => p.isEvent === 'takeoff' || p.isEvent === 'landing');

    return (
        <div style={{ textAlign: "center", background: "#1a1a1a", padding: "15px", borderRadius: "8px", border: "1px solid #333" }}>
            <h4 style={{ margin: "0 0 15px 0", color: "#888" }}>{title} ({icao})</h4>
            <svg width="100%" height="auto" viewBox={`0 0 ${width} ${height}`} style={{ maxWidth: "400px" }}>
                {/* Draw Runways */}
                {validRunways.map((r, i) => {
                    const x1 = scaleX(r.le_longitude_deg!);
                    const y1 = scaleY(r.le_latitude_deg!);
                    const x2 = scaleX(r.he_longitude_deg!);
                    const y2 = scaleY(r.he_latitude_deg!);
                    
                    return (
                        <g key={`rwy-${i}`}>
                            <line 
                                x1={x1} y1={y1} x2={x2} y2={y2} 
                                stroke="#444" 
                                strokeWidth={r.width_ft ? Math.max(3, r.width_ft / 15) : 6} 
                                strokeLinecap="square"
                            />
                            <text x={x1} y={y1} fill="#666" fontSize="10" dy="-8" textAnchor="middle" fontWeight="bold">{r.le_ident}</text>
                            <text x={x2} y={y2} fill="#666" fontSize="10" dy="16" textAnchor="middle" fontWeight="bold">{r.he_ident}</text>
                        </g>
                    );
                })}

                {/* Draw Trajectory */}
                {trajectory.length > 1 && (
                    <polyline
                        points={trajectory.map(p => `${scaleX(p.lon)},${scaleY(p.lat)}`).join(' ')}
                        fill="none"
                        stroke="#2196f3"
                        strokeWidth="2"
                        opacity={0.8}
                    />
                )}

                {/* Draw Events (Red dots for takeoff/landing) */}
                {eventPoints.map((p, i) => (
                    <g key={`event-${i}`}>
                        <circle 
                            cx={scaleX(p.lon)} cy={scaleY(p.lat)} r="6" 
                            fill="#f44336" 
                            stroke="#fff" strokeWidth="1.5"
                        />
                        <text 
                            x={scaleX(p.lon)} y={scaleY(p.lat)} 
                            fill="#fff" fontSize="9" fontWeight="bold" 
                            dy={p.isEvent === 'takeoff' ? -12 : 20} textAnchor="middle"
                            style={{ textShadow: "0 0 3px #000" }}
                        >
                            {p.isEvent === 'takeoff' ? "LIFT OFF" : "TOUCHDOWN"}
                        </text>
                    </g>
                ))}
            </svg>
        </div>
    );
}

export function FlightDetails({ flight, onBack }: { flight: FlightSummary, onBack: () => void }) {
    const [data, setData] = useState<FlightLogRow[]>([]);
    const [startRunways, setStartRunways] = useState<Runway[]>([]);
    const [endRunways, setEndRunways] = useState<Runway[]>([]);
    const [loading, setLoading] = useState(true);
    const [exporting, setExporting] = useState(false);

    useEffect(() => {
        setLoading(true);
        Promise.all([
            invoke<FlightLogRow[]>("get_flight_data", { filename: flight.filename }),
            invoke<Runway[]>("get_runways", { ident: flight.startIcao }),
            invoke<Runway[]>("get_runways", { ident: flight.endIcao })
        ]).then(([flightData, startRwys, endRwys]) => {
            setData(flightData);
            setStartRunways(startRwys);
            setEndRunways(endRwys);
        }).finally(() => setLoading(false));
    }, [flight.filename, flight.startIcao, flight.endIcao]);

    const { departureTrajectory, arrivalTrajectory } = useMemo(() => {
        if (data.length === 0) return { departureTrajectory: [], arrivalTrajectory: [] };

        const mapToTraj = (row: FlightLogRow, eventType?: 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent'): TrajectoryPoint => ({
            lat: row.metrics.latitude,
            lon: row.metrics.longitude,
            onGround: row.metrics.sim_on_ground > 0.5,
            isEvent: eventType
        });

        // Find all event indices once for fast lookup
        const eventIndices = new Map<string, 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent'>();
        flight.events.forEach(e => {
            eventIndices.set(e.timestamp, e.eventType);
        });

        // Takeoff point (first takeoff)
        const firstTakeoff = flight.events.find(e => e.eventType === 'takeoff');
        const takeoffIdx = firstTakeoff ? data.findIndex(row => row.timestamp === firstTakeoff.timestamp) : -1;

        // Landing point (last landing)
        const lastLanding = [...flight.events].reverse().find(e => e.eventType === 'landing');
        const landingIdx = lastLanding ? data.findIndex(row => row.timestamp === lastLanding.timestamp) : -1;

        // Departure: Window around first takeoff
        const depStart = Math.max(0, (takeoffIdx > -1 ? takeoffIdx : 0) - 60);
        const depEnd = Math.min(data.length, (takeoffIdx > -1 ? takeoffIdx : 60) + 60);
        const departureTrajectory = data.slice(depStart, depEnd).map(row => 
            mapToTraj(row, eventIndices.get(row.timestamp))
        );

        // Arrival: Window around last landing
        const arrStart = Math.max(0, (landingIdx > -1 ? landingIdx : data.length) - 120);
        const arrEnd = Math.min(data.length, (landingIdx > -1 ? landingIdx : data.length) + 30);
        const arrivalTrajectory = data.slice(arrStart, arrEnd).map(row => 
            mapToTraj(row, eventIndices.get(row.timestamp))
        );

        return { departureTrajectory, arrivalTrajectory };
    }, [data, flight.events]);

    const chartData = useMemo(() => {
        if (data.length === 0) return [];
        const sampleRate = Math.max(1, Math.floor(data.length / 300));
        return data.filter((_, i) => i % sampleRate === 0).map(row => ({
            time: row.timestamp.split(' ')[1],
            altitude: Math.round(row.metrics.alt_msl),
            ias: Math.round(row.metrics.ias),
            gs: Math.round(row.metrics.gnd_spd),
            vs: Math.round(row.metrics.v_spd),
            pitch: parseFloat(row.metrics.pitch.toFixed(1)),
            bank: parseFloat(row.metrics.roll.toFixed(1)),
        }));
    }, [data]);

    // Find first TOC and TOD for chart markings (using first one for simplicity in UI)
    const tocPoint = useMemo(() => {
        const toc = flight.events.find(e => e.eventType === 'top_of_climb');
        return toc ? toc.timestamp.split(' ')[1] : null;
    }, [flight.events]);

    const todPoint = useMemo(() => {
        const tod = flight.events.find(e => e.eventType === 'top_of_descent');
        return tod ? tod.timestamp.split(' ')[1] : null;
    }, [flight.events]);

    const handleExport = async () => {
        setExporting(true);
        try {
            const path = await invoke<string>("export_flight_to_csv", { filename: flight.filename });
            alert(`Flight exported to: ${path}`);
        } catch (e) {
            alert(`Export failed: ${e}`);
        } finally {
            setExporting(false);
        }
    };

    if (loading) return <div>Loading flight data...</div>;

    return (
        <div className="flight-details-view" style={{ textAlign: "left", padding: "1rem" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
                <div>
                    <div style={{ fontSize: "0.8rem", color: "#888", marginBottom: "5px" }}>{flight.aircraftTitle} ({flight.aircraftModel})</div>
                    <h2 style={{ margin: 0 }}>{flight.startIcao} → {flight.endIcao}</h2>
                    <p style={{ color: "#888", margin: "5px 0" }}>{flight.startTime} ({flight.durationMinutes} min)</p>
                </div>
                <div>
                    <button 
                        onClick={handleExport} 
                        disabled={exporting}
                        style={{ marginRight: "10px", backgroundColor: "#4caf50" }}
                    >
                        {exporting ? "Exporting..." : "Export G1000 Log (CSV)"}
                    </button>
                    <button onClick={onBack}>Back to History</button>
                </div>
            </div>

            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: "20px", marginBottom: "2rem" }}>
                <div style={{ background: "#2a2a2a", padding: "1.5rem", borderRadius: "8px", textAlign: "center" }}>
                    <div style={{ color: "#888", fontSize: "0.9rem", marginBottom: "0.5rem" }}>MAX ALTITUDE</div>
                    <div style={{ fontSize: "1.5rem", fontWeight: "bold" }}>{flight.maxAltitude.toFixed(0)} ft</div>
                </div>
                <div style={{ background: "#2a2a2a", padding: "1.5rem", borderRadius: "8px", textAlign: "center" }}>
                    <div style={{ color: "#888", fontSize: "0.9rem", marginBottom: "0.5rem" }}>MAX SPEED (GS)</div>
                    <div style={{ fontSize: "1.5rem", fontWeight: "bold" }}>{flight.maxGroundSpeed.toFixed(0)} kt</div>
                </div>
                <div style={{ background: "#2a2a2a", padding: "1.5rem", borderRadius: "8px", textAlign: "center" }}>
                    <div style={{ color: "#888", fontSize: "0.9rem", marginBottom: "0.5rem" }}>FUEL CONSUMED</div>
                    <div style={{ fontSize: "1.5rem", fontWeight: "bold" }}>{flight.fuelConsumed.toFixed(1)} gal</div>
                </div>
                <div style={{ background: "#2a2a2a", padding: "1.5rem", borderRadius: "8px", textAlign: "center" }}>
                    <div style={{ color: "#888", fontSize: "0.9rem", marginBottom: "0.5rem" }}>DURATION</div>
                    <div style={{ fontSize: "1.5rem", fontWeight: "bold" }}>{flight.durationMinutes} min</div>
                </div>
            </div>

            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px", marginBottom: "2rem" }}>
                <RunwayMap 
                    runways={startRunways} 
                    icao={flight.startIcao} 
                    trajectory={departureTrajectory} 
                    title="Departure"
                />
                <RunwayMap 
                    runways={endRunways} 
                    icao={flight.endIcao} 
                    trajectory={arrivalTrajectory} 
                    title="Arrival"
                />
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: "40px" }}>
                {/* Altitude Chart */}
                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333" }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Altitude Profile (ft)</h3>
                    <div style={{ width: '100%', height: 250 }}>
                        <ResponsiveContainer>
                            <AreaChart data={chartData}>
                                <defs>
                                    <linearGradient id="colorAlt" x1="0" y1="0" x2="0" y2="1">
                                        <stop offset="5%" stopColor="#8884d8" stopOpacity={0.8}/>
                                        <stop offset="95%" stopColor="#8884d8" stopOpacity={0}/>
                                    </linearGradient>
                                </defs>
                                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <YAxis stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <Tooltip 
                                    contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }}
                                    itemStyle={{ color: '#fff' }}
                                />
                                {tocPoint && (
                                    <ReferenceLine x={tocPoint} stroke="#4caf50" strokeDasharray="3 3">
                                        <Label value="TOC" position="top" fill="#4caf50" fontSize={10} fontWeight="bold" />
                                    </ReferenceLine>
                                )}
                                {todPoint && (
                                    <ReferenceLine x={todPoint} stroke="#ff9800" strokeDasharray="3 3">
                                        <Label value="TOD" position="top" fill="#ff9800" fontSize={10} fontWeight="bold" />
                                    </ReferenceLine>
                                )}
                                <Area type="monotone" dataKey="altitude" stroke="#8884d8" fillOpacity={1} fill="url(#colorAlt)" />
                            </AreaChart>
                        </ResponsiveContainer>
                    </div>
                </div>

                {/* Speed Chart */}
                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333" }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Airspeed & Groundspeed (kt)</h3>
                    <div style={{ width: '100%', height: 250 }}>
                        <ResponsiveContainer>
                            <LineChart data={chartData}>
                                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <YAxis stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <Tooltip 
                                    contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }}
                                />
                                <Legend />
                                {tocPoint && <ReferenceLine x={tocPoint} stroke="#4caf50" strokeDasharray="3 3" label={{ value: 'TOC', fill: '#4caf50', fontSize: 10 }} />}
                                {todPoint && <ReferenceLine x={todPoint} stroke="#ff9800" strokeDasharray="3 3" label={{ value: 'TOD', fill: '#ff9800', fontSize: 10 }} />}
                                <Line type="monotone" dataKey="ias" name="Indicated Airspeed" stroke="#4caf50" dot={false} strokeWidth={2} />
                                <Line type="monotone" dataKey="gs" name="Groundspeed" stroke="#2196f3" dot={false} strokeWidth={2} />
                            </LineChart>
                        </ResponsiveContainer>
                    </div>
                </div>

                {/* Vertical Speed Chart */}
                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333" }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Vertical Speed (fpm)</h3>
                    <div style={{ width: '100%', height: 200 }}>
                        <ResponsiveContainer>
                            <LineChart data={chartData}>
                                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <YAxis stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <Tooltip 
                                    contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }}
                                />
                                <Line type="monotone" dataKey="vs" name="Vertical Speed" stroke="#f44336" dot={false} strokeWidth={1.5} />
                            </LineChart>
                        </ResponsiveContainer>
                    </div>
                </div>

                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px" }}>
                    {/* Pitch Chart */}
                    <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333" }}>
                        <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Pitch Angle (deg)</h3>
                        <div style={{ width: '100%', height: 200 }}>
                            <ResponsiveContainer>
                                <LineChart data={chartData}>
                                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                    <XAxis dataKey="time" stroke="#666" fontSize={10} tick={{fill: '#666'}} />
                                    <YAxis stroke="#666" fontSize={10} tick={{fill: '#666'}} domain={['auto', 'auto']} />
                                    <Tooltip contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }} />
                                    <Line type="monotone" dataKey="pitch" name="Pitch" stroke="#ff9800" dot={false} />
                                </LineChart>
                            </ResponsiveContainer>
                        </div>
                    </div>

                    {/* Bank Chart */}
                    <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333" }}>
                        <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Bank Angle (deg)</h3>
                        <div style={{ width: '100%', height: 200 }}>
                            <ResponsiveContainer>
                                <LineChart data={chartData}>
                                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                    <XAxis dataKey="time" stroke="#666" fontSize={10} tick={{fill: '#666'}} />
                                    <YAxis stroke="#666" fontSize={10} tick={{fill: '#666'}} domain={['auto', 'auto']} />
                                    <Tooltip contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }} />
                                    <Line type="monotone" dataKey="bank" name="Roll/Bank" stroke="#00bcd4" dot={false} />
                                </LineChart>
                            </ResponsiveContainer>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}
