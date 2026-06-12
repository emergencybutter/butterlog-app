import {
    LineChart, Line, XAxis, YAxis, CartesianGrid, Tooltip, Legend, ResponsiveContainer, AreaChart, Area, ReferenceLine
} from 'recharts';

/** One downsampled chart row derived from the flight log. */
export interface ChartRow {
    time: string;
    altitude: number;
    ias: number;
    gs: number;
    vs: number;
    pitch: number;
    bank: number;
    gforce: number;
}

/** A vertical event marker drawn on the time axis. */
export interface EventMark {
    x: string;
    color: string;
    label: string;
    dashed?: boolean;
    width?: number;
}

function eventReferenceLines(marks: EventMark[], bold: boolean) {
    return marks.map((m, idx) => (
        <ReferenceLine
            key={`mark-${m.label}-${idx}`}
            x={m.x}
            stroke={m.color}
            strokeWidth={m.width ?? 2}
            strokeDasharray={m.dashed ? "3 3" : undefined}
            label={{ value: m.label, position: 'top', fill: m.color, fontSize: 10, ...(bold ? { fontWeight: 'bold' } : {}) }}
        />
    ));
}

const tooltipStyle = { background: '#14181a', border: '1px solid #444' };

function ChartPanel({ title, height, children }: { title: string, height: number, children: React.ReactElement }) {
    return (
        <div className="chart-panel">
            <h3 className="panel-title">{title}</h3>
            <div style={{ width: '100%', height, minWidth: 0 }}>
                <ResponsiveContainer width="100%" height="100%" minWidth={0}>
                    {children}
                </ResponsiveContainer>
            </div>
        </div>
    );
}

export function FlightCharts({ chartData, fullMarks, primaryMarks }: {
    chartData: ChartRow[],
    /** All event markers (takeoff/landing/TOC/TOD/AP), for the main charts. */
    fullMarks: EventMark[],
    /** Takeoff/landing only, drawn thinner on the secondary charts. */
    primaryMarks: EventMark[],
}) {
    return (
        <div style={{ display: "flex", flexDirection: "column", gap: "40px" }}>
            <ChartPanel title="Altitude Profile (ft)" height={250}>
                <AreaChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                    <defs>
                        <linearGradient id="colorAlt" x1="0" y1="0" x2="0" y2="1">
                            <stop offset="5%" stopColor="#ffb000" stopOpacity={0.8} />
                            <stop offset="95%" stopColor="#ffb000" stopOpacity={0} />
                        </linearGradient>
                    </defs>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{ fill: '#666' }} />
                    <YAxis stroke="#666" fontSize={12} tick={{ fill: '#666' }} />
                    <Tooltip contentStyle={tooltipStyle} itemStyle={{ color: '#fff' }} />
                    {eventReferenceLines(fullMarks, true)}
                    <Area type="monotone" dataKey="altitude" stroke="#ffb000" fillOpacity={1} fill="url(#colorAlt)" />
                </AreaChart>
            </ChartPanel>

            <ChartPanel title="Airspeed & Groundspeed (kt)" height={250}>
                <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{ fill: '#666' }} />
                    <YAxis stroke="#666" fontSize={12} tick={{ fill: '#666' }} />
                    <Tooltip contentStyle={tooltipStyle} />
                    <Legend />
                    {eventReferenceLines(fullMarks, true)}
                    <Line type="monotone" dataKey="ias" name="Indicated Airspeed" stroke="#4caf50" dot={false} strokeWidth={2} />
                    <Line type="monotone" dataKey="gs" name="Groundspeed" stroke="#2196f3" dot={false} strokeWidth={2} />
                </LineChart>
            </ChartPanel>

            <ChartPanel title="Vertical Speed (fpm)" height={200}>
                <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{ fill: '#666' }} />
                    <YAxis stroke="#666" fontSize={12} tick={{ fill: '#666' }} />
                    <Tooltip contentStyle={tooltipStyle} />
                    {eventReferenceLines(primaryMarks, false)}
                    <Line type="monotone" dataKey="vs" name="Vertical Speed" stroke="#f44336" dot={false} strokeWidth={1.5} />
                </LineChart>
            </ChartPanel>

            <ChartPanel title="G-Force (G)" height={200}>
                <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                    <XAxis dataKey="time" stroke="#666" fontSize={12} tick={{ fill: '#666' }} />
                    <YAxis stroke="#666" fontSize={12} tick={{ fill: '#666' }} domain={['auto', 'auto']} />
                    <Tooltip contentStyle={tooltipStyle} />
                    {eventReferenceLines(primaryMarks, false)}
                    <ReferenceLine y={1.0} stroke="#555" strokeDasharray="3 3" />
                    <Line type="monotone" dataKey="gforce" name="G-Force" stroke="#e91e63" dot={false} strokeWidth={1.5} />
                </LineChart>
            </ChartPanel>

            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "20px" }}>
                <ChartPanel title="Pitch Angle (deg)" height={200}>
                    <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                        <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                        <XAxis dataKey="time" stroke="#666" fontSize={10} tick={{ fill: '#666' }} />
                        <YAxis stroke="#666" fontSize={10} tick={{ fill: '#666' }} domain={['auto', 'auto']} />
                        <Tooltip contentStyle={tooltipStyle} />
                        <Line type="monotone" dataKey="pitch" name="Pitch" stroke="#ff9800" dot={false} />
                    </LineChart>
                </ChartPanel>

                <ChartPanel title="Bank Angle (deg)" height={200}>
                    <LineChart data={chartData} margin={{ top: 25, right: 20, left: 0, bottom: 0 }}>
                        <CartesianGrid strokeDasharray="3 3" stroke="#333" />
                        <XAxis dataKey="time" stroke="#666" fontSize={10} tick={{ fill: '#666' }} />
                        <YAxis stroke="#666" fontSize={10} tick={{ fill: '#666' }} domain={['auto', 'auto']} />
                        <Tooltip contentStyle={tooltipStyle} />
                        <Line type="monotone" dataKey="bank" name="Roll/Bank" stroke="#00bcd4" dot={false} />
                    </LineChart>
                </ChartPanel>
            </div>
        </div>
    );
}
