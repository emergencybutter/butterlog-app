import { useState, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { 
    LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer, AreaChart, Area, ReferenceLine, Label
} from 'recharts';
import { MapContainer, TileLayer, Polyline, Marker, Popup, useMap, Tooltip as LeafletTooltip } from 'react-leaflet';
import L from 'leaflet';
import 'leaflet/dist/leaflet.css';

// Fix for default marker icons in Leaflet
import markerIcon from 'leaflet/dist/images/marker-icon.png';
import markerIcon2x from 'leaflet/dist/images/marker-icon-2x.png';
import markerShadow from 'leaflet/dist/images/marker-shadow.png';

// @ts-ignore
delete L.Icon.Default.prototype._getIconUrl;
L.Icon.Default.mergeOptions({
    iconUrl: markerIcon,
    iconRetinaUrl: markerIcon2x,
    shadowUrl: markerShadow,
});

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

function MapAutoBounds({ bounds }: { bounds: L.LatLngBoundsExpression }) {
    const map = useMap();
    useEffect(() => {
        if (bounds) {
            map.fitBounds(bounds, { padding: [20, 20] });
        }
    }, [bounds, map]);
    return null;
}

function RunwayMap({ runways, icao, trajectory, title }: { runways: Runway[], icao: string, trajectory: TrajectoryPoint[], title: string }) {
    const validRunways = useMemo(() => runways.filter(r => 
        r.le_latitude_deg !== null && r.le_longitude_deg !== null && 
        r.he_latitude_deg !== null && r.he_longitude_deg !== null
    ), [runways]);

    const bounds = useMemo(() => {
        const points: L.LatLngExpression[] = [];
        validRunways.forEach(r => {
            points.push([r.le_latitude_deg!, r.le_longitude_deg!]);
            points.push([r.he_latitude_deg!, r.he_longitude_deg!]);
        });
        trajectory.forEach(p => points.push([p.lat, p.lon]));
        
        if (points.length === 0) return null;
        return L.latLngBounds(points);
    }, [validRunways, trajectory]);

    if (!bounds) {
        return <div style={{ height: 350, display: "flex", alignItems: "center", justifyContent: "center", border: "1px solid #333", borderRadius: "8px", background: "#1a1a1a" }}>No map data for {icao}</div>;
    }

    const eventPoints = trajectory.filter(p => p.isEvent === 'takeoff' || p.isEvent === 'landing');
    const trajPath: L.LatLngExpression[] = trajectory.map(p => [p.lat, p.lon]);

    return (
        <div style={{ textAlign: "center", background: "#1a1a1a", padding: "15px", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
            <h4 style={{ margin: "0 0 15px 0", color: "#888" }}>{title} ({icao})</h4>
            <div style={{ height: "350px", borderRadius: "4px", overflow: "hidden" }}>
                <MapContainer 
                    bounds={bounds} 
                    style={{ height: "100%", width: "100%" }}
                    zoomControl={true}
                    scrollWheelZoom={true}
                >
                    <TileLayer
                        attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors &copy; <a href="https://carto.com/attributions">CARTO</a>'
                        url="https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png"
                    />
                    
                    {/* Runways */}
                    {validRunways.map((r, i) => (
                        <Polyline 
                            key={`rwy-${i}`}
                            positions={[[r.le_latitude_deg!, r.le_longitude_deg!], [r.he_latitude_deg!, r.he_longitude_deg!]]}
                            color="#666"
                            weight={Math.max(4, (r.width_ft || 100) / 15)}
                            opacity={0.8}
                        >
                            <LeafletTooltip permanent direction="center" opacity={0.7} className="runway-label">
                                {r.le_ident} / {r.he_ident}
                            </LeafletTooltip>
                        </Polyline>
                    ))}

                    {/* Flight Path */}
                    {trajPath.length > 1 && (
                        <Polyline 
                            positions={trajPath}
                            color="#2196f3"
                            weight={3}
                            opacity={0.8}
                        />
                    )}

                    {/* Events */}
                    {eventPoints.map((p, i) => (
                        <Marker 
                            key={`event-${i}`} 
                            position={[p.lat, p.lon]}
                            icon={L.divIcon({
                                className: 'custom-event-marker',
                                html: `<div style="background-color: #f44336; width: 12px; height: 12px; border-radius: 50%; border: 2px solid white;"></div>`,
                                iconSize: [12, 12],
                                iconAnchor: [6, 6]
                            })}
                        >
                            <Popup>
                                <strong>{p.isEvent?.toUpperCase().replace('_', ' ')}</strong>
                            </Popup>
                            <LeafletTooltip permanent direction="top" offset={[0, -10]} opacity={0.9} className="event-label">
                                {p.isEvent === 'takeoff' ? "LIFT OFF" : "TOUCHDOWN"}
                            </LeafletTooltip>
                        </Marker>
                    ))}

                    <MapAutoBounds bounds={bounds} />
                </MapContainer>
            </div>
            <style>{`
                .runway-label {
                    background: transparent;
                    border: none;
                    box-shadow: none;
                    color: #fff;
                    font-weight: bold;
                    font-size: 10px;
                    text-shadow: 1px 1px 2px #000;
                }
                .runway-label::before {
                    display: none;
                }
                .event-label {
                    background: rgba(244, 67, 54, 0.8);
                    border: none;
                    color: white;
                    font-weight: bold;
                    font-size: 10px;
                    padding: 2px 5px;
                    border-radius: 4px;
                }
                .event-label::before {
                    border-top-color: rgba(244, 67, 54, 0.8);
                }
                .leaflet-container {
                    background: #111 !important;
                }
            `}</style>
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

        // Helper to find closest data index for a given timestamp
        const findClosestIndex = (timestamp: string) => {
            // Binary search or simple find if data is sorted
            let bestIdx = -1;
            let minDiff = Infinity;
            
            const targetTs = new Date(timestamp.replace(' ', 'T')).getTime();
            
            for (let i = 0; i < data.length; i++) {
                const currentTs = new Date(data[i].timestamp.replace(' ', 'T')).getTime();
                const diff = Math.abs(currentTs - targetTs);
                if (diff < minDiff) {
                    minDiff = diff;
                    bestIdx = i;
                }
                // Optimization: if diff starts increasing, we passed the target
                if (diff > minDiff && i > 0) break; 
            }
            // Only return if it's within a reasonable window (e.g. 5 seconds)
            return minDiff < 5000 ? bestIdx : -1;
        };

        const mapToTraj = (row: FlightLogRow, eventType?: 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent'): TrajectoryPoint => ({
            lat: row.metrics.latitude,
            lon: row.metrics.longitude,
            onGround: row.metrics.sim_on_ground > 0.5,
            isEvent: eventType
        });

        // Find key event indices using closest match
        const firstTakeoff = flight.events.find(e => e.eventType === 'takeoff');
        const takeoffIdx = firstTakeoff ? findClosestIndex(firstTakeoff.timestamp) : -1;

        const lastLanding = [...flight.events].reverse().find(e => e.eventType === 'landing');
        const landingIdx = lastLanding ? findClosestIndex(lastLanding.timestamp) : -1;

        // Map all events to their closest indices for the traj mapper
        const eventIndexMap = new Map<number, 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent'>();
        flight.events.forEach(e => {
            const idx = findClosestIndex(e.timestamp);
            if (idx > -1) eventIndexMap.set(idx, e.eventType);
        });

        // Departure: Window around first takeoff
        const depStart = Math.max(0, (takeoffIdx > -1 ? takeoffIdx : 0) - 60);
        const depEnd = Math.min(data.length, (takeoffIdx > -1 ? takeoffIdx : 60) + 60);
        const departureTrajectory = data.slice(depStart, depEnd).map((row, i) => 
            mapToTraj(row, eventIndexMap.get(depStart + i))
        );

        // Arrival: Window around last landing
        const arrStart = Math.max(0, (landingIdx > -1 ? landingIdx : data.length) - 120);
        const arrEnd = Math.min(data.length, (landingIdx > -1 ? landingIdx : data.length) + 30);
        const arrivalTrajectory = data.slice(arrStart, arrEnd).map((row, i) => 
            mapToTraj(row, eventIndexMap.get(arrStart + i))
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

    // Find indices in chartData for event markers
    const findChartTime = (eventTime: string) => {
        if (!eventTime) return null;
        const timePart = eventTime.split(' ')[1];
        // Try exact match first
        if (chartData.some(d => d.time === timePart)) return timePart;
        
        // Find closest time in sampled chartData
        let best = null;
        let minDiff = Infinity;
        const target = new Date(`1970-01-01T${timePart}`).getTime();
        
        for (const d of chartData) {
            const current = new Date(`1970-01-01T${d.time}`).getTime();
            const diff = Math.abs(current - target);
            if (diff < minDiff) {
                minDiff = diff;
                best = d.time;
            }
        }
        return minDiff < 10000 ? best : null; // Within 10s
    };

    const tocPoint = useMemo(() => {
        const toc = flight.events.find(e => e.eventType === 'top_of_climb');
        return toc ? findChartTime(toc.timestamp) : null;
    }, [flight.events, chartData]);

    const todPoint = useMemo(() => {
        const tod = flight.events.find(e => e.eventType === 'top_of_descent');
        return tod ? findChartTime(tod.timestamp) : null;
    }, [flight.events, chartData]);

    const takeoffPoint = useMemo(() => {
        const takeoff = flight.events.find(e => e.eventType === 'takeoff');
        return takeoff ? findChartTime(takeoff.timestamp) : null;
    }, [flight.events, chartData]);

    const landingPoint = useMemo(() => {
        const landing = [...flight.events].reverse().find(e => e.eventType === 'landing');
        return landing ? findChartTime(landing.timestamp) : null;
    }, [flight.events, chartData]);

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
                    <div style={{ fontSize: "0.8rem", color: "#888", marginBottom: "5px" }}>{flight.aircraftTitle}</div>
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
                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Altitude Profile (ft)</h3>
                    <div style={{ width: '100%', height: 250, minWidth: 0 }}>
                        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
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
                                {takeoffPoint && (
                                    <ReferenceLine x={takeoffPoint} stroke="#f44336" strokeWidth={2}>
                                        <Label value="LIFT OFF" position="top" fill="#f44336" fontSize={10} fontWeight="bold" />
                                    </ReferenceLine>
                                )}
                                {landingPoint && (
                                    <ReferenceLine x={landingPoint} stroke="#f44336" strokeWidth={2}>
                                        <Label value="TOUCHDOWN" position="top" fill="#f44336" fontSize={10} fontWeight="bold" />
                                    </ReferenceLine>
                                )}
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
                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Airspeed & Groundspeed (kt)</h3>
                    <div style={{ width: '100%', height: 250, minWidth: 0 }}>
                        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                            <LineChart data={chartData}>
                                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <YAxis stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <Tooltip 
                                    contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }}
                                />
                                <Legend />
                                {takeoffPoint && (
                                    <ReferenceLine x={takeoffPoint} stroke="#f44336" strokeWidth={2}>
                                        <Label value="LIFT OFF" position="top" fill="#f44336" fontSize={10} fontWeight="bold" />
                                    </ReferenceLine>
                                )}
                                {landingPoint && (
                                    <ReferenceLine x={landingPoint} stroke="#f44336" strokeWidth={2}>
                                        <Label value="TOUCHDOWN" position="top" fill="#f44336" fontSize={10} fontWeight="bold" />
                                    </ReferenceLine>
                                )}
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
                                <Line type="monotone" dataKey="ias" name="Indicated Airspeed" stroke="#4caf50" dot={false} strokeWidth={2} />
                                <Line type="monotone" dataKey="gs" name="Groundspeed" stroke="#2196f3" dot={false} strokeWidth={2} />
                            </LineChart>
                        </ResponsiveContainer>
                    </div>
                </div>

                {/* Vertical Speed Chart */}
                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Vertical Speed (fpm)</h3>
                    <div style={{ width: '100%', height: 200, minWidth: 0 }}>
                        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                            <LineChart data={chartData}>
                                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <YAxis stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <Tooltip 
                                    contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }}
                                />
                                {takeoffPoint && <ReferenceLine x={takeoffPoint} stroke="#f44336" strokeWidth={1} label={{ value: 'LIFT OFF', fill: '#f44336', fontSize: 10, position: 'top' }} />}
                                {landingPoint && <ReferenceLine x={landingPoint} stroke="#f44336" strokeWidth={1} label={{ value: 'TOUCHDOWN', fill: '#f44336', fontSize: 10, position: 'top' }} />}
                                <Line type="monotone" dataKey="vs" name="Vertical Speed" stroke="#f44336" dot={false} strokeWidth={1.5} />
                            </LineChart>
                        </ResponsiveContainer>
                    </div>
                </div>

                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px" }}>
                    {/* Pitch Chart */}
                    <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                        <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Pitch Angle (deg)</h3>
                        <div style={{ width: '100%', height: 200, minWidth: 0 }}>
                            <ResponsiveContainer width="100%" height="100%" minWidth={0}>
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
                    <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                        <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Bank Angle (deg)</h3>
                        <div style={{ width: '100%', height: 200, minWidth: 0 }}>
                            <ResponsiveContainer width="100%" height="100%" minWidth={0}>
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
