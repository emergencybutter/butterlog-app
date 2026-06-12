import { useEffect, useMemo } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { MapContainer, TileLayer, Polyline, Marker, Popup, useMap, Tooltip as LeafletTooltip } from 'react-leaflet';
import L from 'leaflet';
import { Runway, Screenshot } from "../models";
import { screenshotIcon, getEventIcon, eventLabel, eventLabelClass } from "./icons";

export interface TrajectoryPoint {
    lat: number;
    lon: number;
    onGround: boolean;
    isEvent?: string;
}

export function MapAutoBounds({ bounds }: { bounds: L.LatLngBoundsExpression }) {
    const map = useMap();
    useEffect(() => {
        if (bounds) {
            map.fitBounds(bounds, { padding: [20, 20] });
        }
    }, [bounds, map]);
    return null;
}

export function ScreenshotMarker({ screenshot }: { screenshot: Screenshot }) {
    return (
        <Marker position={[screenshot.latitude, screenshot.longitude]} icon={screenshotIcon}>
            <Popup>
                <div style={{ width: "220px" }}>
                    <img src={convertFileSrc(screenshot.path)} alt="Screenshot" style={{ width: "100%", borderRadius: "2px" }} />
                    <div style={{ fontSize: "0.7rem", marginTop: "5px" }}>
                        {screenshot.timestamp.includes(' ') ? screenshot.timestamp.split(' ')[1] : screenshot.timestamp}
                    </div>
                </div>
            </Popup>
        </Marker>
    );
}

export function RunwayMap({ runways, icao, trajectory, fullTrajectory, title, screenshots }: {
    runways: Runway[],
    icao: string,
    trajectory: TrajectoryPoint[],
    fullTrajectory: { lat: number, lon: number }[],
    title: string,
    screenshots?: Screenshot[]
}) {
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
        return <div style={{ height: 350, display: "flex", alignItems: "center", justifyContent: "center", border: "1px solid #333", borderRadius: "8px", background: "#0e1113" }}>No map data for {icao}</div>;
    }

    const eventPoints = trajectory.filter(p => p.isEvent === 'takeoff' || p.isEvent === 'landing' || p.isEvent === 'autopilot_on' || p.isEvent === 'autopilot_off');
    const fullTrajPath: L.LatLngExpression[] = fullTrajectory.map(p => [p.lat, p.lon]);

    return (
        <div className="chart-panel" style={{ textAlign: "center", padding: "15px" }}>
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
                            icon={getEventIcon(p.isEvent)}
                        >
                            <Popup>
                                <strong>{p.isEvent?.toUpperCase().replace('_', ' ')}</strong>
                            </Popup>
                            <LeafletTooltip permanent direction="top" offset={[0, -10]} opacity={0.9} className={eventLabelClass(p.isEvent)}>
                                {eventLabel(p.isEvent)}
                            </LeafletTooltip>
                        </Marker>
                    ))}

                    {/* Screenshots */}
                    {screenshots?.map((s, i) => (
                        <ScreenshotMarker key={`scr-${i}`} screenshot={s} />
                    ))}

                    <MapAutoBounds bounds={bounds} />
                </MapContainer>
            </div>
        </div>
    );
}
