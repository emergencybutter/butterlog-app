import { useState, useEffect, useMemo } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { 
    LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer, AreaChart, Area, ReferenceLine
} from 'recharts';
import { MapContainer, TileLayer, Polyline, Marker, Popup, useMap, Tooltip as LeafletTooltip } from 'react-leaflet';
import L from 'leaflet';
import 'leaflet/dist/leaflet.css';
import { FlightEvent, FlightLogRow, FlightSummary, Runway, Screenshot } from "./models";

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

interface TrajectoryPoint {
    lat: number;
    lon: number;
    onGround: boolean;
    isEvent?: 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent' | 'autopilot_on' | 'autopilot_off';
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

function RunwayMap({ runways, icao, trajectory, fullTrajectory, title, screenshots }: { runways: Runway[], icao: string, trajectory: TrajectoryPoint[], fullTrajectory: {lat: number, lon: number}[], title: string, screenshots?: Screenshot[] }) {
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

    const eventPoints = trajectory.filter(p => p.isEvent === 'takeoff' || p.isEvent === 'landing' || p.isEvent === 'autopilot_on' || p.isEvent === 'autopilot_off');
    const fullTrajPath: L.LatLngExpression[] = fullTrajectory.map(p => [p.lat, p.lon]);

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

                    {/* Full Flight Path */}
                    {fullTrajPath.length > 1 && (
                        <Polyline 
                            positions={fullTrajPath}
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
                                html: `<div style="background-color: ${p.isEvent === 'takeoff' || p.isEvent === 'landing' ? '#f44336' : (p.isEvent === 'autopilot_on' ? '#2196f3' : (p.isEvent === 'autopilot_off' ? '#ff9800' : '#4caf50'))}; width: 12px; height: 12px; border-radius: 50%; border: 2px solid white;"></div>`,
                                iconSize: [12, 12],
                                iconAnchor: [6, 6]
                            })}
                        >
                            <Popup>
                                <strong>{p.isEvent?.toUpperCase().replace('_', ' ')}</strong>
                            </Popup>
                            <LeafletTooltip permanent direction="top" offset={[0, -10]} opacity={0.9} className={p.isEvent === 'takeoff' || p.isEvent === 'landing' ? 'event-label' : (p.isEvent === 'autopilot_on' ? 'event-label-blue' : (p.isEvent === 'autopilot_off' ? 'event-label-orange' : 'event-label-green'))}>
                                {p.isEvent === 'takeoff' ? "LIFT OFF" : (p.isEvent === 'landing' ? "TOUCHDOWN" : (p.isEvent === 'autopilot_on' ? "AP ON" : (p.isEvent === 'autopilot_off' ? "AP OFF" : p.isEvent?.toUpperCase())))}
                            </LeafletTooltip>
                        </Marker>
                    ))}

                    {/* Screenshots */}
                    {screenshots?.map((s, i) => (
                        <Marker 
                            key={`scr-${i}`} 
                            position={[s.latitude, s.longitude]}
                            icon={L.divIcon({
                                className: 'custom-scr-marker',
                                html: `<div style="background-color: #e91e63; width: 24px; height: 24px; border-radius: 50%; border: 2px solid white; display: flex; align-items: center; justify-content: center; box-shadow: 0 2px 5px rgba(0,0,0,0.5);">
                                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/></svg>
                                </div>`,
                                iconSize: [24, 24],
                                iconAnchor: [12, 12]
                            })}
                        >
                            <Popup>
                                <div style={{ width: "220px" }}>
                                    <img src={convertFileSrc(s.path)} alt="Screenshot" style={{ width: "100%", borderRadius: "2px" }} />
                                    <div style={{ fontSize: "0.7rem", marginTop: "5px" }}>
                                        {s.timestamp.includes(' ') ? s.timestamp.split(' ')[1] : s.timestamp}
                                    </div>
                                </div>
                            </Popup>
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
                    background: rgba(244, 67, 54, 0.6);
                    border: none;
                    color: white;
                    font-weight: bold;
                    font-size: 10px;
                    padding: 2px 5px;
                    border-radius: 4px;
                }
                .event-label::before {
                    border-top-color: rgba(244, 67, 54, 0.6);
                }
                .event-label-blue {
                    background: rgba(33, 150, 243, 0.6);
                    border: none;
                    color: white;
                    font-weight: bold;
                    font-size: 10px;
                    padding: 2px 5px;
                    border-radius: 4px;
                }
                .event-label-blue::before {
                    border-top-color: rgba(33, 150, 243, 0.6);
                }
                .event-label-orange {
                    background: rgba(255, 152, 0, 0.6);
                    border: none;
                    color: white;
                    font-weight: bold;
                    font-size: 10px;
                    padding: 2px 5px;
                    border-radius: 4px;
                }
                .event-label-orange::before {
                    border-top-color: rgba(255, 152, 0, 0.6);
                }
                .leaflet-container {
                    background: #111 !important;
                }
                .leaflet-control-attribution {
                    background: rgba(40, 40, 40, 0.9) !important;
                    color: #fff !important;
                    border-radius: 20px !important;
                    border: 1px solid #555 !important;
                    margin-bottom: 10px !important;
                    margin-right: 10px !important;
                    padding: 0 !important;
                    font-size: 10px !important;
                    max-width: 18px;
                    height: 18px;
                    line-height: 18px;
                    overflow: hidden;
                    white-space: nowrap;
                    transition: max-width 0.4s cubic-bezier(0.4, 0, 0.2, 1), background 0.3s, padding 0.4s;
                    cursor: pointer;
                    display: flex !important;
                    align-items: center;
                    box-shadow: 0 2px 5px rgba(0,0,0,0.5);
                }
                .leaflet-control-attribution:hover {
                    max-width: 600px;
                    background: #222 !important;
                    padding: 0 10px !important;
                }
                .leaflet-control-attribution a {
                    color: #4db8ff !important;
                    text-decoration: none;
                }
                .leaflet-control-attribution a:hover {
                    text-decoration: underline;
                }
                .leaflet-control-attribution::before {
                    content: "i";
                    font-family: serif;
                    font-style: italic;
                    font-weight: bold;
                    min-width: 18px;
                    text-align: center;
                    font-size: 12px;
                }
                .leaflet-control-attribution .leaflet-control-attribution-prefix {
                    display: none;
                }
                .leaflet-bar {
                    border: 1px solid #444 !important;
                    box-shadow: 0 2px 5px rgba(0,0,0,0.5) !important;
                }
                .leaflet-bar a {
                    background-color: #333 !important;
                    color: #fff !important;
                    border-bottom: 1px solid #444 !important;
                    transition: background-color 0.2s, border-color 0.2s !important;
                }
                .leaflet-bar a:hover {
                    background-color: #444 !important;
                    color: #4caf50 !important;
                    border-color: #4caf50 !important;
                }
                .leaflet-bar a.leaflet-disabled {
                    background-color: #222 !important;
                    color: #555 !important;
                }
            `}</style>
        </div>
    );
}

function FullFlightMap({ trajectory, events, screenshots }: { trajectory: {lat: number, lon: number}[], events: FlightEvent[], screenshots: Screenshot[] }) {
    const bounds = useMemo(() => {
        if (trajectory.length === 0) return null;
        const points: L.LatLngExpression[] = trajectory.map(p => [p.lat, p.lon]);
        return L.latLngBounds(points);
    }, [trajectory]);

    const filteredEvents = useMemo(() => {
        const result: FlightEvent[] = [];
        const types = ['takeoff', 'top_of_climb', 'top_of_descent'] as const;
        
        for (const type of types) {
            const found = events.find(e => e.eventType === type);
            if (found) result.push(found);
        }

        const landing = [...events].reverse().find(e => e.eventType === 'landing');
        if (landing) result.push(landing);

        // Include all autopilot toggles
        events.forEach(e => {
            if (e.eventType === 'autopilot_on' || e.eventType === 'autopilot_off') {
                result.push(e);
            }
        });

        return result;
    }, [events]);

    if (!bounds) return null;

    const trajPath: L.LatLngExpression[] = trajectory.map(p => [p.lat, p.lon]);

    return (
        <div style={{ background: "#1a1a1a", padding: "15px", borderRadius: "8px", border: "1px solid #333", marginBottom: "2rem" }}>
            <h3 style={{ marginTop: 0, marginBottom: "15px", color: "#888" }}>Full Flight Path</h3>
            <div style={{ height: "400px", borderRadius: "4px", overflow: "hidden" }}>
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
                    
                    {trajPath.length > 1 && (
                        <Polyline 
                            positions={trajPath}
                            color="#2196f3"
                            weight={3}
                            opacity={0.8}
                        />
                    )}

                    {filteredEvents.map((e, i) => (
                        <Marker 
                            key={`event-full-${i}`} 
                            position={[e.latitude, e.longitude]}
                            icon={L.divIcon({
                                className: 'custom-event-marker',
                                html: `<div style="background-color: ${e.eventType === 'takeoff' || e.eventType === 'landing' ? '#f44336' : (e.eventType.startsWith('autopilot') ? (e.eventType === 'autopilot_on' ? '#2196f3' : '#ff9800') : '#4caf50')}; width: 12px; height: 12px; border-radius: 50%; border: 2px solid white;"></div>`,
                                iconSize: [12, 12],
                                iconAnchor: [6, 6]
                            })}
                        >
                            <Popup>
                                <strong>{e.eventType.toUpperCase().replace('_', ' ')}</strong><br/>
                                {e.timestamp.includes(' ') ? e.timestamp.split(' ')[1] : e.timestamp}
                            </Popup>
                            <LeafletTooltip permanent direction="top" offset={[0, -10]} opacity={0.9} className={e.eventType === 'takeoff' || e.eventType === 'landing' ? 'event-label-red' : (e.eventType === 'autopilot_on' ? 'event-label-blue' : (e.eventType === 'autopilot_off' ? 'event-label-orange' : 'event-label-green'))}>
                                {e.eventType === 'top_of_climb' ? 'TOC' : (e.eventType === 'top_of_descent' ? 'TOD' : (e.eventType === 'autopilot_on' ? 'AP ON' : (e.eventType === 'autopilot_off' ? 'AP OFF' : e.eventType.toUpperCase())))}
                            </LeafletTooltip>
                        </Marker>
                    ))}

                    {screenshots.map((s, i) => (
                        <Marker 
                            key={`scr-full-${i}`} 
                            position={[s.latitude, s.longitude]}
                            icon={L.divIcon({
                                className: 'custom-scr-marker',
                                html: `<div style="background-color: #e91e63; width: 24px; height: 24px; border-radius: 50%; border: 2px solid white; display: flex; align-items: center; justify-content: center; box-shadow: 0 2px 5px rgba(0,0,0,0.5);">
                                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/></svg>
                                </div>`,
                                iconSize: [24, 24],
                                iconAnchor: [12, 12]
                            })}
                        >
                            <Popup>
                                <div style={{ width: "220px" }}>
                                    <img src={convertFileSrc(s.path)} alt="Screenshot" style={{ width: "100%", borderRadius: "2px" }} />
                                    <div style={{ fontSize: "0.7rem", marginTop: "5px" }}>
                                        {s.timestamp.includes(' ') ? s.timestamp.split(' ')[1] : s.timestamp}
                                    </div>
                                </div>
                            </Popup>
                        </Marker>
                    ))}

                    <MapAutoBounds bounds={bounds} />
                </MapContainer>
            </div>
            <style>{`
                .event-label-red {
                    background: rgba(244, 67, 54, 0.6);
                    border: none;
                    color: white;
                    font-weight: bold;
                    font-size: 10px;
                    padding: 2px 5px;
                    border-radius: 4px;
                }
                .event-label-red::before {
                    border-top-color: rgba(244, 67, 54, 0.6);
                }
                .event-label-green {
                    background: rgba(76, 175, 80, 0.6);
                    border: none;
                    color: white;
                    font-weight: bold;
                    font-size: 10px;
                    padding: 2px 5px;
                    border-radius: 4px;
                }
                .event-label-green::before {
                    border-top-color: rgba(76, 175, 80, 0.6);
                }
                .event-label-blue {
                    background: rgba(33, 150, 243, 0.6);
                    border: none;
                    color: white;
                    font-weight: bold;
                    font-size: 10px;
                    padding: 2px 5px;
                    border-radius: 4px;
                }
                .event-label-blue::before {
                    border-top-color: rgba(33, 150, 243, 0.6);
                }
                .event-label-orange {
                    background: rgba(255, 152, 0, 0.6);
                    border: none;
                    color: white;
                    font-weight: bold;
                    font-size: 10px;
                    padding: 2px 5px;
                    border-radius: 4px;
                }
                .event-label-orange::before {
                    border-top-color: rgba(255, 152, 0, 0.6);
                }
            `}</style>
        </div>
    );
}

export function FlightDetails({ flight: initialFlight, onBack, currentFlightId }: { flight: FlightSummary, onBack: () => void, currentFlightId?: string }) {
    const [flight, setFlight] = useState<FlightSummary>(initialFlight);
    const [data, setData] = useState<FlightLogRow[]>([]);
    const [startRunways, setStartRunways] = useState<Runway[]>([]);
    const [endRunways, setEndRunways] = useState<Runway[]>([]);
    const [screenshots, setScreenshots] = useState<Screenshot[]>([]);
    const [loading, setLoading] = useState(true);
    const [exporting, setExporting] = useState(false);
    const [remoteId, setRemoteId] = useState<number | null>(null);
    const [webhookEnabled, setWebhookEnabled] = useState(false);
    const [uploadingIds, setUploadingIds] = useState<Set<number>>(new Set());

    const isCurrentFlight = useMemo(() => {
        return currentFlightId && flight.filename.replace(".db", "") === currentFlightId;
    }, [currentFlightId, flight.filename]);

    const fetchData = () => {
        const flightId = flight.filename.replace(".db", "");
        Promise.all([
            invoke<FlightLogRow[]>("get_flight_data", { filename: flight.filename }),
            invoke<Screenshot[]>("get_screenshots_for_flight", { flightId })
        ]).then(([flightData, scrs]) => {
            setData(flightData);
            setScreenshots(scrs);
        }).finally(() => setLoading(false));
    };

    const fetchSummary = async () => {
        try {
            const updatedSummary = await invoke<FlightSummary>("get_flight_summary", { filename: flight.filename });
            setFlight(updatedSummary);
        } catch (e) {
            console.error("Failed to fetch flight summary:", e);
        }
    };

    useEffect(() => {
        setLoading(true);
        const flightId = flight.filename.replace(".db", "");
        Promise.all([
            invoke<FlightLogRow[]>("get_flight_data", { filename: flight.filename }),
            invoke<Runway[]>("get_runways", { ident: flight.startIcao }),
            invoke<Runway[]>("get_runways", { ident: flight.endIcao }),
            invoke<Screenshot[]>("get_screenshots_for_flight", { flightId }),
            invoke<any>("get_config"),
            invoke<number | null>("get_remote_id", { filename: flight.filename })
        ]).then(([flightData, startRwys, endRwys, scrs, config, rId]) => {
            setData(flightData);
            setStartRunways(startRwys);
            setEndRunways(endRwys);
            setScreenshots(scrs);
            setWebhookEnabled(config.enableWebhook && !!config.webhookUrl);
            setRemoteId(rId);
        }).finally(() => setLoading(false));

        const unlistenNew = listen("new-screenshot", () => {
            const flightId = flight.filename.replace(".db", "");
            invoke<Screenshot[]>("get_screenshots_for_flight", { flightId }).then(setScreenshots);
        });

        const unlistenUploaded = listen("screenshot-uploaded", () => {
            const flightId = flight.filename.replace(".db", "");
            invoke<Screenshot[]>("get_screenshots_for_flight", { flightId }).then(setScreenshots);
        });

        const unlistenSummary = listen("flight-logs-updated", () => {
            fetchSummary();
        });

        if (isCurrentFlight) {
            const interval = setInterval(fetchData, 2000);
            return () => {
                clearInterval(interval);
                unlistenNew.then(fn => fn());
                unlistenUploaded.then(fn => fn());
                unlistenSummary.then(fn => fn());
            };
        }

        return () => {
            unlistenNew.then(fn => fn());
            unlistenUploaded.then(fn => fn());
            unlistenSummary.then(fn => fn());
        };
    }, [flight.filename, flight.startIcao, flight.endIcao, isCurrentFlight]);

    const handleUploadScreenshot = async (screenshotId: number) => {
        setUploadingIds(prev => new Set(prev).add(screenshotId));
        try {
            await invoke("upload_screenshot", { 
                screenshotId, 
                flightFilename: flight.filename 
            });
            // Refresh screenshots to show 'uploaded' status
            const flightId = flight.filename.replace(".db", "");
            const updatedScrs = await invoke<Screenshot[]>("get_screenshots_for_flight", { flightId });
            setScreenshots(updatedScrs);
        } catch (e) {
            alert(`Upload failed: ${e}`);
        } finally {
            setUploadingIds(prev => {
                const next = new Set(prev);
                next.delete(screenshotId);
                return next;
            });
        }
    };

    const { departureTrajectory, arrivalTrajectory } = useMemo(() => {
        if (data.length === 0) return { departureTrajectory: [], arrivalTrajectory: [] };

        const findClosestIndex = (timestamp: string) => {
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
                if (diff > minDiff && i > 0) break; 
            }
            return minDiff < 5000 ? bestIdx : -1;
        };

        const mapToTraj = (row: FlightLogRow, eventType?: 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent' | 'autopilot_on' | 'autopilot_off'): TrajectoryPoint => ({
            lat: row.metrics.Latitude,
            lon: row.metrics.Longitude,
            onGround: row.metrics.sim_on_ground > 0.5,
            isEvent: eventType
        });

        const firstTakeoff = flight.events.find(e => e.eventType === 'takeoff');
        const takeoffIdx = firstTakeoff ? findClosestIndex(firstTakeoff.timestamp) : -1;

        const lastLanding = [...flight.events].reverse().find(e => e.eventType === 'landing');
        const landingIdx = lastLanding ? findClosestIndex(lastLanding.timestamp) : -1;

        const eventIndexMap = new Map<number, 'takeoff' | 'landing' | 'top_of_climb' | 'top_of_descent' | 'autopilot_on' | 'autopilot_off'>();
        flight.events.forEach(e => {
            const idx = findClosestIndex(e.timestamp);
            if (idx > -1) eventIndexMap.set(idx, e.eventType as any);
        });

        const depStart = Math.max(0, (takeoffIdx > -1 ? takeoffIdx : 0) - 60);
        const depEnd = Math.min(data.length, (takeoffIdx > -1 ? takeoffIdx : 60) + 60);
        const departureTrajectory = data.slice(depStart, depEnd).map((row, i) => 
            mapToTraj(row, eventIndexMap.get(depStart + i))
        );

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
            altitude: Math.round(row.metrics.AltMSL),
            ias: Math.round(row.metrics.IAS),
            gs: Math.round(row.metrics.GndSpd),
            vs: Math.round(row.metrics.VSpd),
            pitch: parseFloat(row.metrics.Pitch.toFixed(1)),
            bank: parseFloat(row.metrics.Roll.toFixed(1)),
            gforce: row.metrics.gforce !== undefined ? parseFloat(row.metrics.gforce.toFixed(2)) : (row.metrics.NormAc !== undefined ? parseFloat(row.metrics.NormAc.toFixed(2)) : 1.0),
        }));
    }, [data]);

    const findChartTime = (eventTime: string) => {
        if (!eventTime) return null;
        const timePart = eventTime.split(' ')[1];
        if (chartData.some(d => d.time === timePart)) return timePart;
        
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
        return minDiff < 10000 ? best : null;
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

    const apOnPoints = useMemo(() => {
        return flight.events.filter(e => e.eventType === 'autopilot_on').map(e => findChartTime(e.timestamp)).filter(t => t !== null) as string[];
    }, [flight.events, chartData]);

    const apOffPoints = useMemo(() => {
        return flight.events.filter(e => e.eventType === 'autopilot_off').map(e => findChartTime(e.timestamp)).filter(t => t !== null) as string[];
    }, [flight.events, chartData]);

    const fullTrajectory = useMemo(() => {
        if (data.length === 0) return [];
        const sampleRate = Math.max(1, Math.floor(data.length / 500));
        return data.filter((_, i) => i % sampleRate === 0).map(row => ({
            lat: row.metrics.Latitude,
            lon: row.metrics.Longitude
        }));
    }, [data]);

    const handleExport = async () => {
        setExporting(true);
        try {
            const path = await invoke<string>("export_flight_to_csv", { filename: flight.filename });
            await revealItemInDir(path);
        } catch (e) {
            alert(`Export failed: ${e}`);
        } finally {
            setExporting(false);
        }
    };

    const landingEvent = useMemo(() => [...flight.events].reverse().find(e => e.eventType === 'landing'), [flight.events]);

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

            {landingEvent && (
                <div style={{ marginBottom: "2rem", background: "#2a2a2a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333" }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888", fontSize: "1.1rem" }}>Landing Performance</h3>
                    <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(180px, 1fr))", gap: "20px" }}>
                        <div>
                            <div style={{ color: "#aaa", fontSize: "0.8rem", fontWeight: "bold", marginBottom: "0.5rem" }}>TOUCHDOWN VS</div>
                            <div style={{ fontSize: "1.5rem", fontWeight: "bold", color: "#eee" }}>{Math.round(landingEvent.touchdownFpm || 0)} fpm</div>
                        </div>
                        <div>
                            <div style={{ color: "#aaa", fontSize: "0.8rem", fontWeight: "bold", marginBottom: "0.5rem" }}>LANDING G</div>
                            <div style={{ fontSize: "1.5rem", fontWeight: "bold", color: "#eee" }}>{(landingEvent.landingG || 1.0).toFixed(2)} G</div>
                        </div>
                        <div>
                            <div style={{ color: "#aaa", fontSize: "0.8rem", fontWeight: "bold", marginBottom: "0.5rem" }}>OFFSET</div>
                            <div style={{ fontSize: "1.5rem", fontWeight: "bold", color: "#eee" }}>
                                {landingEvent.offsetPercent !== undefined ? `${Math.abs(landingEvent.offsetPercent).toFixed(1)}% ${landingEvent.offsetPercent < 0 ? 'L' : 'R'}` : "N/A"}
                            </div>
                        </div>
                        <div>
                            <div style={{ color: "#aaa", fontSize: "0.8rem", fontWeight: "bold", marginBottom: "0.5rem" }}>THR DISTANCE</div>
                            <div style={{ fontSize: "1.5rem", fontWeight: "bold", color: "#eee" }}>{Math.round(landingEvent.thresholdDistFt || 0)} ft</div>
                        </div>
                    </div>
                </div>
            )}

            <FullFlightMap trajectory={fullTrajectory} events={flight.events} screenshots={screenshots} />

            {screenshots.length > 0 && (
                <div style={{ marginBottom: "2rem" }}>
                    <h3 style={{ color: "#888", marginBottom: "1rem" }}>Screenshots</h3>
                    <div style={{ display: "flex", gap: "15px", overflowX: "auto", paddingBottom: "10px" }}>
                        {screenshots.map((s, i) => (
                            <div key={i} style={{ flex: "0 0 auto", width: "280px", background: "#1a1a1a", borderRadius: "4px", overflow: "hidden", border: "1px solid #333", position: "relative" }}>
                                <img src={convertFileSrc(s.path)} alt="Flight Screenshot" style={{ width: "100%", height: "170px", objectFit: "cover" }} />
                                
                                {webhookEnabled && remoteId && (
                                    <div 
                                        onClick={s.remoteHash || uploadingIds.has(s.id) ? undefined : () => handleUploadScreenshot(s.id)}
                                        style={{ 
                                            position: "absolute", 
                                            bottom: "35px", 
                                            right: "5px", 
                                            background: s.remoteHash ? "rgba(76, 175, 80, 0.9)" : "rgba(0,0,0,0.6)", 
                                            border: s.remoteHash ? "none" : "1px solid #555",
                                            color: "white", 
                                            padding: "4px", 
                                            borderRadius: "4px",
                                            cursor: s.remoteHash ? "default" : "pointer",
                                            display: "flex",
                                            alignItems: "center",
                                            justifyContent: "center",
                                            boxShadow: s.remoteHash ? "0 2px 4px rgba(0,0,0,0.3)" : "none"
                                        }}
                                        title={s.remoteHash ? "Uploaded to Butterlog" : "Upload to Butterlog"}
                                    >
                                        {uploadingIds.has(s.id) ? (
                                            <div className="spinner" style={{ width: "14px", height: "14px", border: "2px solid #fff", borderTop: "2px solid transparent", borderRadius: "50%", animation: "spin 1s linear infinite" }}></div>
                                        ) : s.remoteHash ? (
                                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                                        ) : (
                                            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v4a2 2 0 0 1 2-2h4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                                        )}
                                    </div>
                                )}

                                <div style={{ padding: "5px", fontSize: "0.7rem", color: "#888", textAlign: "center" }}>
                                    {s.timestamp.includes(' ') ? s.timestamp.split(' ')[1] : s.timestamp}
                                </div>
                            </div>
                        ))}
                    </div>
                </div>
            )}
            <style>{`
                @keyframes spin {
                    0% { transform: rotate(0deg); }
                    100% { transform: rotate(360deg); }
                }
            `}</style>

            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px", marginBottom: "2rem" }}>
                <RunwayMap 
                    runways={startRunways} 
                    icao={flight.startIcao} 
                    trajectory={departureTrajectory} 
                    fullTrajectory={fullTrajectory}
                    title="Departure"
                    screenshots={screenshots}
                />
                <RunwayMap 
                    runways={endRunways} 
                    icao={flight.endIcao} 
                    trajectory={arrivalTrajectory} 
                    fullTrajectory={fullTrajectory}
                    title="Arrival"
                    screenshots={screenshots}
                />
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: "40px" }}>
                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Altitude Profile (ft)</h3>
                    <div style={{ width: '100%', height: 250, minWidth: 0 }}>
                        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                            <AreaChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
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
                                    <ReferenceLine x={takeoffPoint} stroke="#f44336" strokeWidth={2} label={{ value: 'LIFT OFF', position: 'top', fill: '#f44336', fontSize: 10, fontWeight: 'bold' }} />
                                )}
                                {landingPoint && (
                                    <ReferenceLine x={landingPoint} stroke="#f44336" strokeWidth={2} label={{ value: 'TOUCHDOWN', position: 'top', fill: '#f44336', fontSize: 10, fontWeight: 'bold' }} />
                                )}
                                {tocPoint && (
                                    <ReferenceLine x={tocPoint} stroke="#4caf50" strokeDasharray="3 3" label={{ value: 'TOC', position: 'top', fill: '#4caf50', fontSize: 10, fontWeight: 'bold' }} />
                                )}
                                {todPoint && (
                                    <ReferenceLine x={todPoint} stroke="#ff9800" strokeDasharray="3 3" label={{ value: 'TOD', position: 'top', fill: '#ff9800', fontSize: 10, fontWeight: 'bold' }} />
                                )}
                                {apOnPoints.map((p, idx) => (
                                    <ReferenceLine key={`ap-on-${idx}`} x={p} stroke="#2196f3" strokeDasharray="3 3" label={{ value: 'AP ON', position: 'top', fill: '#2196f3', fontSize: 10, fontWeight: 'bold' }} />
                                ))}
                                {apOffPoints.map((p, idx) => (
                                    <ReferenceLine key={`ap-off-${idx}`} x={p} stroke="#ff9800" strokeDasharray="3 3" label={{ value: 'AP OFF', position: 'top', fill: '#ff9800', fontSize: 10, fontWeight: 'bold' }} />
                                ))}
                                <Area type="monotone" dataKey="altitude" stroke="#8884d8" fillOpacity={1} fill="url(#colorAlt)" />
                            </AreaChart>
                        </ResponsiveContainer>
                    </div>
                </div>

                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Airspeed & Groundspeed (kt)</h3>
                    <div style={{ width: '100%', height: 250, minWidth: 0 }}>
                        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                            <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <YAxis stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <Tooltip 
                                    contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }}
                                />
                                <Legend />
                                {takeoffPoint && (
                                    <ReferenceLine x={takeoffPoint} stroke="#f44336" strokeWidth={2} label={{ value: 'LIFT OFF', position: 'top', fill: '#f44336', fontSize: 10, fontWeight: 'bold' }} />
                                )}
                                {landingPoint && (
                                    <ReferenceLine x={landingPoint} stroke="#f44336" strokeWidth={2} label={{ value: 'TOUCHDOWN', position: 'top', fill: '#f44336', fontSize: 10, fontWeight: 'bold' }} />
                                )}
                                {tocPoint && (
                                    <ReferenceLine x={tocPoint} stroke="#4caf50" strokeDasharray="3 3" label={{ value: 'TOC', position: 'top', fill: '#4caf50', fontSize: 10, fontWeight: 'bold' }} />
                                )}
                                {todPoint && (
                                    <ReferenceLine x={todPoint} stroke="#ff9800" strokeDasharray="3 3" label={{ value: 'TOD', position: 'top', fill: '#ff9800', fontSize: 10, fontWeight: 'bold' }} />
                                )}
                                {apOnPoints.map((p, idx) => (
                                    <ReferenceLine key={`ap-on-${idx}`} x={p} stroke="#2196f3" strokeDasharray="3 3" label={{ value: 'AP ON', position: 'top', fill: '#2196f3', fontSize: 10, fontWeight: 'bold' }} />
                                ))}
                                {apOffPoints.map((p, idx) => (
                                    <ReferenceLine key={`ap-off-${idx}`} x={p} stroke="#ff9800" strokeDasharray="3 3" label={{ value: 'AP OFF', position: 'top', fill: '#ff9800', fontSize: 10, fontWeight: 'bold' }} />
                                ))}
                                <Line type="monotone" dataKey="ias" name="Indicated Airspeed" stroke="#4caf50" dot={false} strokeWidth={2} />
                                <Line type="monotone" dataKey="gs" name="Groundspeed" stroke="#2196f3" dot={false} strokeWidth={2} />
                            </LineChart>
                        </ResponsiveContainer>
                    </div>
                </div>

                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Vertical Speed (fpm)</h3>
                    <div style={{ width: '100%', height: 200, minWidth: 0 }}>
                        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                            <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
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

                <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                    <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>G-Force (G)</h3>
                    <div style={{ width: '100%', height: 200, minWidth: 0 }}>
                        <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                            <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                                <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{fill: '#666'}} />
                                <YAxis stroke="#666" fontSize={12} tick={{fill: '#666'}} domain={['auto', 'auto']} />
                                <Tooltip 
                                    contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }}
                                />
                                {takeoffPoint && <ReferenceLine x={takeoffPoint} stroke="#f44336" strokeWidth={1} label={{ value: 'LIFT OFF', fill: '#f44336', fontSize: 10, position: 'top' }} />}
                                {landingPoint && <ReferenceLine x={landingPoint} stroke="#f44336" strokeWidth={1} label={{ value: 'TOUCHDOWN', fill: '#f44336', fontSize: 10, position: 'top' }} />}
                                <ReferenceLine y={1.0} stroke="#555" strokeDasharray="3 3" />
                                <Line type="monotone" dataKey="gforce" name="G-Force" stroke="#e91e63" dot={false} strokeWidth={1.5} />
                            </LineChart>
                        </ResponsiveContainer>
                    </div>
                </div>

                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px" }}>
                    <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                        <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Pitch Angle (deg)</h3>
                        <div style={{ width: '100%', height: 200, minWidth: 0 }}>
                            <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                                <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                                    <XAxis dataKey="time" stroke="#666" fontSize={10} tick={{fill: '#666'}} />
                                    <YAxis stroke="#666" fontSize={10} tick={{fill: '#666'}} domain={['auto', 'auto']} />
                                    <Tooltip contentStyle={{ background: '#2a2a2a', border: '1px solid #444' }} />
                                    <Line type="monotone" dataKey="pitch" name="Pitch" stroke="#ff9800" dot={false} />
                                </LineChart>
                            </ResponsiveContainer>
                        </div>
                    </div>

                    <div style={{ background: "#1a1a1a", padding: "1.5rem", borderRadius: "8px", border: "1px solid #333", minWidth: 0 }}>
                        <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888" }}>Bank Angle (deg)</h3>
                        <div style={{ width: '100%', height: 200, minWidth: 0 }}>
                            <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                                <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
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
