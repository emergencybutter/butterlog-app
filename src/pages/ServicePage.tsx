import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { Config, FlightRef, ServiceStatus } from "../models";

export function linkText(s: ServiceStatus | null): { dot: string; text: string } {
  switch (s?.link) {
    case "connected": return { dot: "ok", text: s.me ? `Connected as ${s.me.displayName}` : "Connected" };
    case "connecting": return { dot: "warn", text: "Connecting…" };
    case "unreachable": return { dot: "bad", text: "Can't reach the service" };
    default: return { dot: "", text: "Not signed in" };
  }
}

function since(ms: number | null, now: number): string {
  if (ms === null) return "never";
  const s = Math.max(0, Math.round((now - ms) / 1000));
  return s < 5 ? "just now" : s < 120 ? `${s} s ago` : `${Math.round(s / 60)} min ago`;
}

function endedAt(f: FlightRef): string {
  if (!f.endedAt) return "";
  const d = new Date(f.endedAt);
  return isNaN(d.getTime()) ? "" : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function route(f: FlightRef): string {
  return f.arrival ? `${f.departure} → ${f.arrival}` : f.departure;
}

const open = (page: string) => invoke("open_service_page", { page }).catch((e) => console.error(e));

/// The service connection: who the app is signed in as, whether the service answers, the
/// flights it made from this app's telemetry, and signing in or out.
export function ServicePage({ status }: { status: ServiceStatus | null }) {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [now, setNow] = useState(Date.now());

  // Only the "last answered" age moves; once every 5 s is plenty.
  useEffect(() => {
    const t = window.setInterval(() => !document.hidden && setNow(Date.now()), 5000);
    return () => clearInterval(t);
  }, []);

  const signIn = async () => {
    setBusy(true);
    setMessage("Opening the browser to sign in with Discord…");
    try {
      await invoke<string>("start_discord_login");
      setMessage("");
    } catch (err) {
      setMessage("Sign-in failed: " + err);
    } finally {
      setBusy(false);
    }
  };

  const signOut = async () => {
    const confirmed = await ask(
      "Sign out of the ButterLog service? Flights are no longer recorded until you sign in again. Your multiplayer preferences are kept.",
      { title: "Sign out", kind: "warning" }
    );
    if (!confirmed) return;
    try {
      const config = await invoke<Config>("get_config");
      await invoke("set_config", { config: { ...config, apiToken: "" } });
    } catch (err) {
      setMessage("Couldn't sign out: " + err);
    }
  };

  const link = linkText(status);
  const me = status?.me ?? null;
  const signedIn = !!status && status.link !== "signed_out";

  return (
    <div className="page" style={{ maxWidth: 760 }}>
      <div className="page-head">
        <h2>ButterLog service</h2>
        <button className="primary" onClick={() => open("flights")}>Open ButterLog on the web ↗</button>
      </div>

      {status?.flightDone && (
        <div className="notice">
          <span className="dot accent" />
          <span className="grow">Flight done: <strong>{route(status.flightDone)}</strong>. Its debrief is ready.</span>
          <button className="primary" onClick={() => open("debrief")}>Open the debrief ↗</button>
        </div>
      )}

      <div className="box">
        <div className="box-head">
          <h3><span className={`dot ${link.dot}`} />{link.text}</h3>
          {signedIn
            ? <button onClick={signOut}>Sign out</button>
            : <button className="primary" disabled={busy} onClick={signIn}>{busy ? "Waiting for the browser…" : "Sign in with Discord"}</button>}
        </div>
        {!signedIn && (
          <p className="dim">
            ButterLog records your flights by sending the simulator's telemetry to the service, which
            makes the flight, its map and its debrief. Sign in with Discord to start: no new account to make.
          </p>
        )}
        {signedIn && (
          <div className="row" style={{ gap: 14, alignItems: "flex-start" }}>
            {me?.avatarUrl && <img className="avatar" src={me.avatarUrl} alt="" />}
            <dl className="kv" style={{ flex: 1 }}>
              {me && <><dt>Account</dt><dd>{me.displayName} <span className="faint">@{me.username}</span></dd></>}
              <dt>Service</dt><dd className="mono">{status!.serviceUrl}</dd>
              <dt>Last answered</dt><dd>{since(status!.lastOkMs, now)}</dd>
              {status!.error && <><dt>Problem</dt><dd className="bad">{status!.error}</dd></>}
              <dt>Telemetry</dt>
              <dd>
                {status!.recording ? <span className="ok">Recording a session</span> : <span className="dim">No session open: none is while no aircraft is loaded</span>}
                {status!.unsentKb > 0 && <span className="warn"> · {status!.unsentKb.toLocaleString()} kB waiting to be sent</span>}
              </dd>
              {status!.uploadError && <><dt>Upload</dt><dd className="bad">Paused: {status!.uploadError}</dd></>}
              {me && <><dt>FlySto</dt><dd>{me.flystoConnected ? (me.flystoAuto ? "Connected: flights go there by themselves" : "Connected: you upload the flights you choose") : <button className="quiet" onClick={() => open("settings")}>Connect on the web ↗</button>}</dd></>}
            </dl>
          </div>
        )}
        {message && <p className={/fail|Couldn't/.test(message) ? "bad" : "dim"} style={{ marginTop: 8 }}>{message}</p>}
      </div>

      {me && (
        <div className="box">
          <div className="box-head"><h3>Flights</h3><button onClick={() => open("flights")}>All my flights ↗</button></div>
          <dl className="kv">
            <dt>Flying now</dt>
            <dd>
              {me.live
                ? <span className="row">{route(me.live)} {me.live.aircraft && <span className="dim">{me.live.aircraft}</span>}<button onClick={() => open("live")}>Watch live ↗</button></span>
                : <span className="dim">Nothing</span>}
            </dd>
            <dt>Last flight</dt>
            <dd>
              {me.lastEnded
                ? <span className="row">
                    {route(me.lastEnded)}
                    {me.lastEnded.aircraft && <span className="dim">{me.lastEnded.aircraft}</span>}
                    <span className="faint">{endedAt(me.lastEnded)}</span>
                    <button onClick={() => open("debrief")}>Debrief ↗</button>
                  </span>
                : <span className="dim">None yet</span>}
            </dd>
          </dl>
        </div>
      )}

      <p className="faint" style={{ fontSize: 12 }}>
        ButterLog runs in the tray. Closing this window frees it entirely; it draws without the GPU
        while open. When a flight is done the tray icon gets a dot: click it for the debrief.
      </p>
    </div>
  );
}
