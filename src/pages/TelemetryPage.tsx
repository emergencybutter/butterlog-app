import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LiveChannel, LiveTelemetry, SimTelemetry } from "../models";
import { METRIC_LABELS } from "./metricLabels";

/// How often the page asks for fresh values while it is shown.
const POLL_MS = 500;

const SIM_NAMES: Record<string, string> = {
  msfs2020: "Microsoft Flight Simulator 2020",
  msfs2024: "Microsoft Flight Simulator 2024",
  msfs: "Microsoft Flight Simulator",
  xplane11: "X-Plane 11",
  xplane12: "X-Plane 12",
  xplane: "X-Plane",
};

/// How each sim is read, in words.
const CAPTURES: Record<string, string> = {
  simconnect: "SimConnect, every sim frame",
  xp_plugin: "ButterLog X-Plane plugin, every sim frame",
  xp_webapi: "X-Plane Web API (install the plugin in Settings for every frame)",
};

function simName(sim: SimTelemetry): string {
  const live = sim.live;
  return (live && SIM_NAMES[live.sim]) || SIM_NAMES[sim.monitor] || sim.monitor;
}

function formatValue(c: LiveChannel): string {
  const v = c.value;
  if (v === null || v === undefined) return "—";
  if (c.kind === "Bool") return v >= 0.5 ? "on" : "off";
  if (c.kind === "Enum" || c.kind === "I64") return String(Math.round(v));
  if (Number.isInteger(v)) return String(v);
  const a = Math.abs(v);
  return a >= 1000 ? v.toFixed(1) : a >= 1 ? v.toFixed(3) : v.toPrecision(4);
}

function ago(ms: number, now: number): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  return s < 2 ? "now" : `${s}s ago`;
}

/// Ticks a second, from two readings of the counter.
function useRate(live: LiveTelemetry | null): number | null {
  const last = useRef<{ ticks: number; at: number } | null>(null);
  const [rate, setRate] = useState<number | null>(null);
  useEffect(() => {
    if (!live) { last.current = null; setRate(null); return; }
    const now = performance.now();
    const prev = last.current;
    if (!prev || live.ticks < prev.ticks) { last.current = { ticks: live.ticks, at: now }; return; }
    if (now - prev.at >= 2000) {
      setRate(((live.ticks - prev.ticks) * 1000) / (now - prev.at));
      last.current = { ticks: live.ticks, at: now };
    }
  }, [live]);
  return rate;
}

function SimPanel({ sim, signedIn }: { sim: SimTelemetry; signedIn: boolean }) {
  const [filter, setFilter] = useState("");
  const [showMetrics, setShowMetrics] = useState(false);
  const live = sim.live;
  const rate = useRate(live);
  const now = Date.now();

  const channels = useMemo(() => {
    const all = live?.channels ?? [];
    const f = filter.trim().toLowerCase();
    return f ? all.filter((c) => c.name.toLowerCase().includes(f) || c.unit.toLowerCase().includes(f)) : all;
  }, [live, filter]);
  const read = live ? live.channels.filter((c) => c.value !== null).length : 0;

  return (
    <div className="box">
      <div className="box-head">
        <h3><span className="dot ok" />{simName(sim)}</h3>
        <span className="dim">{sim.aircraft || live?.aircraft || "No aircraft loaded yet"}</span>
      </div>

      <dl className="kv" style={{ marginBottom: 12 }}>
        {live?.simVersion && <><dt>Version</dt><dd className="mono">{live.simVersion}</dd></>}
        <dt>Read through</dt>
        <dd>{live ? CAPTURES[live.capture] ?? live.capture : sim.monitor === "msfs" ? CAPTURES.simconnect : "—"}</dd>
        <dt>Recording</dt>
        <dd>
          {live?.session
            ? <><span className="ok">Sending to the service</span> <span className="faint mono">session {live.session}</span></>
            : signedIn
              ? <span className="warn">Not yet: starts once the aircraft is identified</span>
              : <span className="warn">Not recording: sign in on the Service page</span>}
        </dd>
        <dt>Sampling</dt>
        <dd>
          {live ? <>{live.ticks.toLocaleString()} samples{rate !== null && <> · {rate.toFixed(1)} a second</>} · last {ago(live.updatedMs, now)}</> : "Waiting for the first sample"}
        </dd>
        <dt>Channels</dt>
        <dd>{live ? `${read} of ${live.channels.length} read` : "—"}</dd>
      </dl>

      {live && (
        <>
          <div className="row" style={{ marginBottom: 6 }}>
            <input
              placeholder="Filter channels"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              style={{ width: 260 }}
            />
            {filter && <span className="dim">{channels.length} shown</span>}
          </div>
          <div className="scroll">
            <table className="channels">
              <thead>
                <tr><th>{sim.monitor === "msfs" ? "SimVar" : "Dataref"}</th><th style={{ textAlign: "right" }}>Value</th><th>Unit</th><th>Read</th></tr>
              </thead>
              <tbody>
                {channels.map((c) => (
                  <tr key={c.id} className={c.value === null ? "unread" : ""}>
                    <td className="name" title={c.name}>{c.name}</td>
                    <td className="value">{formatValue(c)}</td>
                    <td className="unit">{c.unit}</td>
                    <td className="rate">{c.rate === "OnChange" ? "on change" : c.rate.toLowerCase()}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <h4 className="dim" style={{ margin: "12px 0 4px" }}>Sim events</h4>
          {live.events.length === 0 ? (
            <p className="faint">None since the sim connected.</p>
          ) : (
            <div className="events">
              {[...live.events].reverse().map((e, i) => (
                <div key={i}>
                  <span className="faint">{new Date(e.atMs).toLocaleTimeString()}</span>
                  <span>{e.kind}</span>
                  <span className="dim">{e.detail}</span>
                </div>
              ))}
            </div>
          )}
        </>
      )}

      <div style={{ marginTop: 12 }}>
        <button className="quiet" onClick={() => setShowMetrics(!showMetrics)}>
          {showMetrics ? "Hide" : "Show"} what multiplayer peers get
        </button>
        {showMetrics && (
          <div className="metric-grid" style={{ marginTop: 8 }}>
            {Object.entries(sim.metrics).map(([key, value]) => (
              <div key={key}>
                <span className="dim">{METRIC_LABELS[key] || key}</span>
                <span className="v">{typeof value === "number" ? value.toFixed(2) : String(value)}</span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

/// What each connected sim's capture reads: every channel the telemetry records, with its
/// latest value, so what goes to the service can be checked by eye.
export function TelemetryPage({ signedIn }: { signedIn: boolean }) {
  const [sims, setSims] = useState<SimTelemetry[] | null>(null);

  useEffect(() => {
    let busy = false;
    const poll = async () => {
      if (busy || document.hidden) return;
      busy = true;
      try { setSims(await invoke<SimTelemetry[]>("get_sim_telemetry")); }
      catch (e) { console.error("Telemetry poll failed:", e); }
      finally { busy = false; }
    };
    poll();
    const t = window.setInterval(poll, POLL_MS);
    return () => clearInterval(t);
  }, []);

  return (
    <div className="page">
      <div className="page-head">
        <h2>Simulator telemetry</h2>
        <span className="sub">Everything read from the sim, as it is sent</span>
      </div>
      {sims === null ? null : sims.length === 0 ? (
        <div className="empty">
          No simulator connected. Start Microsoft Flight Simulator or X-Plane 12; ButterLog connects by itself.
        </div>
      ) : (
        sims.map((s) => <SimPanel key={s.monitor} sim={s} signedIn={signedIn} />)
      )}
    </div>
  );
}
