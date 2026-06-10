import L from 'leaflet';

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

// Static Leaflet icons to prevent memory leaks from recreating icons on every render
export const screenshotIcon = L.divIcon({
    className: 'custom-scr-marker',
    html: `<div style="background-color: #e91e63; width: 24px; height: 24px; border-radius: 50%; border: 2px solid white; display: flex; align-items: center; justify-content: center; box-shadow: 0 2px 5px rgba(0,0,0,0.5);">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/></svg>
    </div>`,
    iconSize: [24, 24],
    iconAnchor: [12, 12]
});

const makeDotIcon = (color: string) => L.divIcon({
    className: 'custom-event-marker',
    html: `<div style="background-color: ${color}; width: 12px; height: 12px; border-radius: 50%; border: 2px solid white;"></div>`,
    iconSize: [12, 12],
    iconAnchor: [6, 6]
});

const eventIcons = {
    takeoff: makeDotIcon('#f44336'),
    landing: makeDotIcon('#f44336'),
    autopilot_on: makeDotIcon('#2196f3'),
    autopilot_off: makeDotIcon('#ff9800'),
    default: makeDotIcon('#4caf50'),
};

export const getEventIcon = (eventType?: string) => {
    if (!eventType) return eventIcons.default;
    if (eventType === 'takeoff' || eventType === 'landing') return eventIcons.takeoff;
    if (eventType === 'autopilot_on') return eventIcons.autopilot_on;
    if (eventType === 'autopilot_off') return eventIcons.autopilot_off;
    return eventIcons.default;
};

/** Short display label for a flight event type on map tooltips. */
export const eventLabel = (eventType?: string) => {
    switch (eventType) {
        case 'takeoff': return 'LIFT OFF';
        case 'landing': return 'TOUCHDOWN';
        case 'top_of_climb': return 'TOC';
        case 'top_of_descent': return 'TOD';
        case 'autopilot_on': return 'AP ON';
        case 'autopilot_off': return 'AP OFF';
        default: return eventType?.toUpperCase() ?? '';
    }
};

/** Tooltip CSS class for a flight event type. */
export const eventLabelClass = (eventType?: string) => {
    if (eventType === 'takeoff' || eventType === 'landing') return 'event-label-red';
    if (eventType === 'autopilot_on') return 'event-label-blue';
    if (eventType === 'autopilot_off') return 'event-label-orange';
    return 'event-label-green';
};
