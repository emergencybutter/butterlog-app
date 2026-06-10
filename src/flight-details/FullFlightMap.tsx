import { useMemo } from "react";
import { MapContainer, TileLayer, Polyline, Marker, Popup, Tooltip as LeafletTooltip } from 'react-leaflet';
import L from 'leaflet';
import { FlightEvent, Screenshot } from "../models";
import { getEventIcon, eventLabel, eventLabelClass } from "./icons";
import { MapAutoBounds, ScreenshotMarker } from "./RunwayMap";

export function FullFlightMap({ trajectory, events, screenshots }: {
    trajectory: { lat: number, lon: number }[],
    events: FlightEvent[],
    screenshots: Screenshot[]
}) {
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
        <div className="chart-panel" style={{ padding: "15px", marginBottom: "2rem" }}>
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
                            icon={getEventIcon(e.eventType)}
                        >
                            <Popup>
                                <strong>{e.eventType.toUpperCase().replace('_', ' ')}</strong><br />
                                {e.timestamp.includes(' ') ? e.timestamp.split(' ')[1] : e.timestamp}
                            </Popup>
                            <LeafletTooltip permanent direction="top" offset={[0, -10]} opacity={0.9} className={eventLabelClass(e.eventType)}>
                                {eventLabel(e.eventType)}
                            </LeafletTooltip>
                        </Marker>
                    ))}

                    {screenshots.map((s, i) => (
                        <ScreenshotMarker key={`scr-full-${i}`} screenshot={s} />
                    ))}

                    <MapAutoBounds bounds={bounds} />
                </MapContainer>
            </div>
        </div>
    );
}
