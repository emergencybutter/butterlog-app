import { FlightEvent } from "../models";

function Metric({ label, value }: { label: string, value: string }) {
    return (
        <div>
            <div className="metric-label">{label}</div>
            <div className="metric-value">{value}</div>
        </div>
    );
}

export function LandingPerformance({ event }: { event: FlightEvent }) {
    return (
        <div className="panel-bordered" style={{ marginBottom: "2rem" }}>
            <h3 style={{ marginTop: 0, marginBottom: "1.5rem", color: "#888", fontSize: "1.1rem" }}>Landing Performance</h3>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))", gap: "20px" }}>
                <Metric label="TOUCHDOWN VS" value={`${Math.round(event.touchdownFpm ?? 0)} fpm`} />
                <Metric label="LANDING G" value={`${(event.landingG ?? 1.0).toFixed(2)} G`} />
                <Metric label="VS VAR (1m)" value={event.vsVariance != null ? Math.round(event.vsVariance).toLocaleString() : "N/A"} />
                <Metric label="IAS VAR (1m)" value={event.iasVariance != null ? event.iasVariance.toFixed(1) : "N/A"} />
                <Metric
                    label="OFFSET"
                    value={event.offsetPercent != null ? `${Math.abs(event.offsetPercent).toFixed(1)}% ${event.offsetPercent < 0 ? 'L' : 'R'}` : "N/A"}
                />
                <Metric label="THR DISTANCE" value={`${Math.round(event.thresholdDistFt ?? 0)} ft`} />
            </div>
        </div>
    );
}
