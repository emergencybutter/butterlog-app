import { useState, useEffect, useMemo, memo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import 'leaflet/dist/leaflet.css';
import { Config, FlightLogRow, FlightSummary, Runway, Screenshot } from "./models";
import { RunwayMap, TrajectoryPoint } from "./flight-details/RunwayMap";
import { FullFlightMap } from "./flight-details/FullFlightMap";
import { NotesEditor } from "./flight-details/NotesEditor";
import { LandingPerformance } from "./flight-details/LandingPerformance";
import { LandingScorecard } from "./flight-details/LandingScorecard";
import { ScreenshotGallery } from "./flight-details/ScreenshotGallery";
import { FlightCharts, ChartRow, EventMark } from "./flight-details/FlightCharts";

function FlightDetailsComponent({ flight: initialFlight, currentFlightId }: { flight: FlightSummary, onBack: () => void, currentFlightId?: string }) {
    const [flight, setFlight] = useState<FlightSummary>(initialFlight);
    const [data, setData] = useState<FlightLogRow[]>([]);
    const [startRunways, setStartRunways] = useState<Runway[]>([]);
    const [endRunways, setEndRunways] = useState<Runway[]>([]);
    const [screenshots, setScreenshots] = useState<Screenshot[]>([]);
    const [loading, setLoading] = useState(true);
    const [exporting, setExporting] = useState(false);
    const [sharing, setSharing] = useState(false);
    const [shareUrl, setShareUrl] = useState<string | null>(null);
    const [shareToast, setShareToast] = useState<string | null>(null);
    const [remoteId, setRemoteId] = useState<number | null>(null);
    const [webhookEnabled, setWebhookEnabled] = useState(false);
    const [uploadingIds, setUploadingIds] = useState<Set<number>>(new Set());
    const [notes, setNotes] = useState<string>(initialFlight.notes || "");
    const [notesSaving, setNotesSaving] = useState(false);
    const [notesStatus, setNotesStatus] = useState<string | null>(null);

    // Ref for incremental live fetch — not state so it doesn't cause re-renders
    const lastTimestamp = useRef<string | null>(null);

    const isCurrentFlight = useMemo(() => {
        return currentFlightId && flight.filename.replace(".db", "") === currentFlightId;
    }, [currentFlightId, flight.filename]);

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
        let active = true;
        const flightId = flight.filename.replace(".db", "");

        Promise.all([
            invoke<FlightLogRow[]>("get_flight_data", { filename: flight.filename }),
            invoke<Runway[]>("get_runways", { ident: flight.startIcao }),
            invoke<Runway[]>("get_runways", { ident: flight.endIcao }),
            invoke<Screenshot[]>("get_screenshots_for_flight", { flightId }),
            invoke<Config>("get_config"),
            invoke<number | null>("get_remote_id", { filename: flight.filename })
        ]).then(async ([flightData, startRwys, endRwys, scrs, config, rId]) => {
            if (!active) return;
            setData(flightData);
            lastTimestamp.current = flightData.length > 0 ? flightData[flightData.length - 1].timestamp : null;
            setStartRunways(startRwys);
            setEndRunways(endRwys);
            setWebhookEnabled(config.enableWebhook && !!config.apiToken);
            setRemoteId(rId);

            // For completed flights, retroactively scan for missed screenshots
            const isCompleted = !isCurrentFlight;
            if (isCompleted) {
                const found = await invoke<number>("rescan_flight_screenshots", { filename: flight.filename }).catch(() => 0);
                const finalScrs = found > 0
                    ? await invoke<Screenshot[]>("get_screenshots_for_flight", { flightId }).catch(() => scrs)
                    : scrs;
                if (active) setScreenshots(finalScrs);
            } else {
                if (active) setScreenshots(scrs);
            }
        }).finally(() => {
            if (active) setLoading(false);
        });

        let unlistenNewFn: (() => void) | null = null;
        let unlistenUploadedFn: (() => void) | null = null;
        let unlistenSummaryFn: (() => void) | null = null;

        const refreshScreenshots = () => {
            if (!active) return;
            invoke<Screenshot[]>("get_screenshots_for_flight", { flightId }).then((scrs) => {
                if (active) setScreenshots(scrs);
            });
        };

        listen("new-screenshot", refreshScreenshots).then(fn => {
            if (!active) fn();
            else unlistenNewFn = fn;
        });

        listen("screenshot-uploaded", refreshScreenshots).then(fn => {
            if (!active) fn();
            else unlistenUploadedFn = fn;
        });

        listen("flight-logs-updated", () => {
            if (!active) return;
            fetchSummary();
        }).then(fn => {
            if (!active) fn();
            else unlistenSummaryFn = fn;
        });

        let interval: any = null;
        if (isCurrentFlight) {
            interval = setInterval(() => {
                if (!active) return;
                const since = lastTimestamp.current;
                Promise.all([
                    since
                        ? invoke<FlightLogRow[]>("get_flight_data_since", { filename: flight.filename, since })
                        : invoke<FlightLogRow[]>("get_flight_data", { filename: flight.filename }),
                    invoke<Screenshot[]>("get_screenshots_for_flight", { flightId })
                ]).then(([newRows, scrs]) => {
                    if (!active) return;
                    if (newRows.length > 0) {
                        lastTimestamp.current = newRows[newRows.length - 1].timestamp;
                        setData(prev => [...prev, ...newRows]);
                    }
                    setScreenshots(scrs);
                }).finally(() => {
                    if (active) setLoading(false);
                });
            }, 2000);
        }

        return () => {
            active = false;
            if (interval) clearInterval(interval);
            if (unlistenNewFn) unlistenNewFn();
            if (unlistenUploadedFn) unlistenUploadedFn();
            if (unlistenSummaryFn) unlistenSummaryFn();
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
        const depData = data;

        if (data.length === 0)
            return { departureTrajectory: [], arrivalTrajectory: [] };

        const findClosestIn = (rows: FlightLogRow[], timestamp: string) => {
            let bestIdx = -1;
            let minDiff = Infinity;
            const targetTs = new Date(timestamp.replace(' ', 'T')).getTime();
            for (let i = 0; i < rows.length; i++) {
                const currentTs = new Date(rows[i].timestamp.replace(' ', 'T')).getTime();
                const diff = Math.abs(currentTs - targetTs);
                if (diff < minDiff) { minDiff = diff; bestIdx = i; }
                if (diff > minDiff && i > 0) break;
            }
            return minDiff < 5000 ? bestIdx : -1;
        };

        const mapToTraj = (row: FlightLogRow, eventType?: string): TrajectoryPoint => ({
            lat: row.metrics.Latitude,
            lon: row.metrics.Longitude,
            onGround: row.metrics.sim_on_ground > 0.5,
            isEvent: eventType
        });

        const firstTakeoff = flight.events.find(e => e.eventType === 'takeoff');
        const takeoffIdx = firstTakeoff ? findClosestIn(depData, firstTakeoff.timestamp) : -1;

        const depEventMap = new Map<number, string>();
        flight.events.forEach(e => {
            const idx = findClosestIn(depData, e.timestamp);
            if (idx > -1) depEventMap.set(idx, e.eventType);
        });

        const depStart = Math.max(0, (takeoffIdx > -1 ? takeoffIdx : 0) - 60);
        const depEnd = Math.min(depData.length, (takeoffIdx > -1 ? takeoffIdx : 60) + 60);
        const departureTrajectory = depData.slice(depStart, depEnd).map((row, i) =>
            mapToTraj(row, depEventMap.get(depStart + i))
        );

        const lastLanding = [...flight.events].reverse().find(e => e.eventType === 'landing');
        const landingIdx = lastLanding ? findClosestIn(data, lastLanding.timestamp) : -1;

        const arrEventMap = new Map<number, string>();
        flight.events.forEach(e => {
            const idx = findClosestIn(data, e.timestamp);
            if (idx > -1) arrEventMap.set(idx, e.eventType);
        });

        const arrStart = Math.max(0, (landingIdx > -1 ? landingIdx : data.length) - 120);
        const arrEnd = Math.min(data.length, (landingIdx > -1 ? landingIdx : data.length) + 30);
        const arrivalTrajectory = data.slice(arrStart, arrEnd).map((row, i) =>
            mapToTraj(row, arrEventMap.get(arrStart + i))
        );

        return { departureTrajectory, arrivalTrajectory };
    }, [data, flight.events]);

    const chartData = useMemo<ChartRow[]>(() => {
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

    // Map an event timestamp to the nearest downsampled chart x value
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

    const { fullMarks, primaryMarks } = useMemo(() => {
        const takeoff = flight.events.find(e => e.eventType === 'takeoff');
        const landing = [...flight.events].reverse().find(e => e.eventType === 'landing');
        const toc = flight.events.find(e => e.eventType === 'top_of_climb');
        const tod = flight.events.find(e => e.eventType === 'top_of_descent');

        const takeoffPoint = takeoff ? findChartTime(takeoff.timestamp) : null;
        const landingPoint = landing ? findChartTime(landing.timestamp) : null;
        const tocPoint = toc ? findChartTime(toc.timestamp) : null;
        const todPoint = tod ? findChartTime(tod.timestamp) : null;
        const apOnPoints = flight.events.filter(e => e.eventType === 'autopilot_on').map(e => findChartTime(e.timestamp)).filter(t => t !== null) as string[];
        const apOffPoints = flight.events.filter(e => e.eventType === 'autopilot_off').map(e => findChartTime(e.timestamp)).filter(t => t !== null) as string[];

        const fullMarks: EventMark[] = [
            ...(takeoffPoint ? [{ x: takeoffPoint, color: '#f44336', label: 'LIFT OFF', width: 2 }] : []),
            ...(landingPoint ? [{ x: landingPoint, color: '#f44336', label: 'TOUCHDOWN', width: 2 }] : []),
            ...(tocPoint ? [{ x: tocPoint, color: '#4caf50', label: 'TOC', dashed: true, width: 1 }] : []),
            ...(todPoint ? [{ x: todPoint, color: '#ff9800', label: 'TOD', dashed: true, width: 1 }] : []),
            ...apOnPoints.map(p => ({ x: p, color: '#2196f3', label: 'AP ON', dashed: true, width: 1 })),
            ...apOffPoints.map(p => ({ x: p, color: '#ff9800', label: 'AP OFF', dashed: true, width: 1 })),
        ];

        const primaryMarks: EventMark[] = [
            ...(takeoffPoint ? [{ x: takeoffPoint, color: '#f44336', label: 'LIFT OFF', width: 1 }] : []),
            ...(landingPoint ? [{ x: landingPoint, color: '#f44336', label: 'TOUCHDOWN', width: 1 }] : []),
        ];

        return { fullMarks, primaryMarks };
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

    const notesDirty = notes !== (flight.notes || "");

    const handleSaveNotes = async () => {
        setNotesSaving(true);
        setNotesStatus(null);
        try {
            await invoke("set_flight_notes", { filename: flight.filename, notes });
            setFlight({ ...flight, notes });
            setNotesStatus("Saved");
            setTimeout(() => setNotesStatus(null), 2000);
        } catch (e) {
            setNotesStatus(`Error: ${e}`);
        } finally {
            setNotesSaving(false);
        }
    };

    useEffect(() => {
        invoke<string | null>("get_share_url", { filename: flight.filename })
            .then(url => { if (url) setShareUrl(url); })
            .catch(() => {});
    }, [flight.filename]);

    const showToast = (msg: string) => {
        setShareToast(msg);
        setTimeout(() => setShareToast(null), 3000);
    };

    const handleShare = async () => {
        if (shareUrl) {
            await navigator.clipboard.writeText(shareUrl).catch(() => {});
            showToast("URL copied to clipboard");
            return;
        }
        setSharing(true);
        try {
            const url = await invoke<string>("share_flight", { filename: flight.filename });
            setShareUrl(url);
            await navigator.clipboard.writeText(url).catch(() => {});
            showToast("Flight shared — URL copied to clipboard");
        } catch (e) {
            showToast(`Share failed: ${e}`);
        } finally {
            setSharing(false);
        }
    };

    const landingEvent = useMemo(() => {
        const landingEvents = flight.events.filter(e => e.eventType === 'landing');
        if (landingEvents.length === 0) return undefined;

        // Pick the last landing event
        const lastLanding = landingEvents[landingEvents.length - 1];
        const lastTime = new Date(lastLanding.timestamp.split('.')[0]).getTime();

        // Find the first landing event that is no more than 30 seconds before the last one
        return landingEvents.find(e => {
            const time = new Date(e.timestamp.split('.')[0]).getTime();
            return (lastTime - time) <= 30000;
        });
    }, [flight.events]);

    if (loading) return <div>Loading flight data...</div>;

    const hasLanded = flight.events.some(e => e.eventType === 'landing');
    const destination = hasLanded ? flight.endIcao : (isCurrentFlight ? null : flight.endIcao);

    return (
        <div className="flight-details-view page">
            {shareToast && (
                <div style={{
                    position: "fixed", bottom: "2rem", left: "50%", transform: "translateX(-50%)",
                    background: "#14181a", border: "1px solid var(--bl-line-amber)", borderRadius: "6px",
                    padding: "0.75rem 1.5rem", color: "var(--bl-paper)", fontSize: "0.95rem",
                    boxShadow: "0 8px 24px rgba(0,0,0,0.5)", zIndex: 9999,
                    animation: "fadeInUp 0.2s ease"
                }}>
                    {shareToast}
                </div>
            )}
            <div className="view-header">
                <div>
                    <div style={{ fontSize: "0.8rem", color: "#888", marginBottom: "5px" }}>
                        {flight.aircraftTitle}
                        {flight.atcModel && flight.atcModel !== "Unknown" && flight.atcModel.trim() !== "" ? ` [${flight.atcModel}]` : ""}
                        {flight.atcId && flight.atcId !== "Unknown" && flight.atcId.trim() !== "" ? ` (${flight.atcId})` : ""}
                    </div>
                    <h2 style={{ margin: 0 }}>
                        {flight.startIcao}{destination ? ` → ${destination}` : ''}
                    </h2>
                    <p style={{ color: "#888", margin: "5px 0" }}>{flight.startTime} ({flight.durationMinutes} min)</p>
                </div>
                <div>
                    {!isCurrentFlight && (
                        <button
                            onClick={handleShare}
                            disabled={sharing}
                            style={{ marginRight: "10px", backgroundColor: shareUrl ? "#36e3c6" : "#ffb000", color: "#11111b" }}
                            title={shareUrl ? "Click to copy share URL" : "Upload and share this flight"}
                        >
                            {sharing ? "Sharing..." : shareUrl ? "Copy Share URL" : "Share"}
                        </button>
                    )}
                    <button
                        onClick={handleExport}
                        disabled={exporting}
                        style={{ backgroundColor: "#4caf50" }}
                    >
                        {exporting ? "Exporting..." : "Export G1000 Log (CSV)"}
                    </button>
                </div>
            </div>

            <div className="stat-grid">
                <div className="stat-card">
                    <div className="stat-label">MAX ALTITUDE</div>
                    <div className="stat-value">{flight.maxAltitude.toFixed(0)} ft</div>
                </div>
                <div className="stat-card">
                    <div className="stat-label">MAX SPEED (GS)</div>
                    <div className="stat-value">{flight.maxGroundSpeed.toFixed(0)} kt</div>
                </div>
                <div className="stat-card">
                    <div className="stat-label">FUEL CONSUMED</div>
                    <div className="stat-value">{flight.fuelConsumed.toFixed(1)} gal</div>
                </div>
                <div className="stat-card">
                    <div className="stat-label">DURATION</div>
                    <div className="stat-value">{flight.durationMinutes} min</div>
                </div>
            </div>

            <NotesEditor
                notes={notes}
                onChange={setNotes}
                onSave={handleSaveNotes}
                saving={notesSaving}
                dirty={notesDirty}
                status={notesStatus}
            />

            {landingEvent && <LandingPerformance event={landingEvent} />}

            <FullFlightMap trajectory={fullTrajectory} events={flight.events} screenshots={screenshots} />

            <ScreenshotGallery
                screenshots={screenshots}
                canUpload={webhookEnabled && remoteId !== null}
                uploadingIds={uploadingIds}
                onUpload={handleUploadScreenshot}
            />

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

            {landingEvent && <LandingScorecard event={landingEvent} />}

            <FlightCharts chartData={chartData} fullMarks={fullMarks} primaryMarks={primaryMarks} />
        </div>
    );
}

export const FlightDetails = memo(FlightDetailsComponent);
