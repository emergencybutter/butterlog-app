import { FlightEvent } from "../models";

/// Mirrors FlightEvent::calculate_landing_score on the Rust side: penalties
/// only apply when the underlying measurement exists.
export function landingPenalties(event: FlightEvent) {
    const centerline = event.offsetPercent != null ? -Math.abs(event.offsetPercent) : 0;
    const touchdown = event.thresholdDistFt != null ? -Math.abs(event.thresholdDistFt - 300) / 10 : 0;
    const smoothness = event.landingG != null && event.landingG > 1.2 ? -(event.landingG - 1.2) * 50 : 0;
    return {
        centerline,
        touchdown,
        smoothness,
        total: Math.round(centerline + touchdown + smoothness),
    };
}

const penaltyColor = (penalty: number) =>
    penalty >= -5 ? "#4caf50" : penalty >= -15 ? "#ff9800" : "#f44336";

function ScoreBar({ label, penalty }: { label: string, penalty: number }) {
    return (
        <div style={{ marginBottom: "20px" }}>
            <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "8px" }}>
                <span style={{ color: "#aaa", fontSize: "0.9rem" }}>{label}</span>
                <span style={{ fontWeight: "bold", color: penalty === 0 ? "#4caf50" : "#eee" }}>
                    {penalty.toFixed(1)}
                </span>
            </div>
            <div className="score-bar-track">
                <div
                    className="score-bar-fill"
                    style={{
                        width: `${Math.max(0, 100 + penalty * 2)}%`,
                        background: penaltyColor(penalty),
                    }}
                ></div>
            </div>
        </div>
    );
}

function BigScore({ value, label, color }: { value: number, label: string, color: string }) {
    return (
        <div style={{ textAlign: "center", minWidth: "120px" }}>
            <div style={{ fontSize: "3.5rem", fontWeight: "bold", color }}>{value}</div>
            <div style={{ color: "#888", fontSize: "0.8rem", marginTop: "5px", letterSpacing: "1px" }}>{label}</div>
        </div>
    );
}

export function LandingScorecard({ event }: { event: FlightEvent }) {
    const penalties = landingPenalties(event);
    const totalColor = penalties.total >= -10 ? "#4caf50" : penalties.total >= -30 ? "#ff9800" : "#f44336";

    return (
        <div className="panel-dark" style={{ marginBottom: "2rem" }}>
            <h3 className="panel-title">Landing Scorecard</h3>
            <div style={{ display: "flex", alignItems: "center", gap: "40px", flexWrap: "wrap" }}>
                <BigScore value={penalties.total} label="TOTAL SCORE" color={totalColor} />
                {event.approachStability != null && (
                    <BigScore
                        value={Math.round(event.approachStability)}
                        label="APPROACH STABILITY"
                        color={event.approachStability >= 75 ? "#4caf50" : event.approachStability >= 50 ? "#ff9800" : "#f44336"}
                    />
                )}
                <div style={{ flex: 1, minWidth: "300px" }}>
                    <ScoreBar label="Centerline Alignment" penalty={penalties.centerline} />
                    <ScoreBar label="Touchdown Zone (Target: 300ft)" penalty={penalties.touchdown} />
                    <ScoreBar label="Landing Smoothness (Target: ≤ 1.2G)" penalty={penalties.smoothness} />
                </div>
            </div>
        </div>
    );
}
