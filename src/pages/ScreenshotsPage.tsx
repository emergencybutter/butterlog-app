import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Screenshot } from "../models";

/// When it was taken, in local time: the database keeps UTC.
function takenAt(timestamp: string): string {
  const d = new Date(timestamp.replace(" ", "T") + "Z");
  return isNaN(d.getTime()) ? timestamp : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/// Thumbnails, made one at a time: each first one decodes a full-size picture.
function useThumbnails(shots: Screenshot[]) {
  const [thumbs, setThumbs] = useState<Record<number, string>>({});
  const asked = useRef(new Set<number>());
  useEffect(() => {
    let stopped = false;
    (async () => {
      for (const s of shots) {
        if (stopped) return;
        if (asked.current.has(s.id)) continue;
        asked.current.add(s.id);
        try {
          const path = await invoke<string>("get_screenshot_thumbnail", { id: s.id });
          if (!stopped) setThumbs((t) => ({ ...t, [s.id]: convertFileSrc(path) }));
        } catch (e) {
          console.error("No thumbnail for", s.path, e);
        }
      }
    })();
    return () => { stopped = true; };
  }, [shots]);
  return thumbs;
}

export function ScreenshotsPage({ signedIn, autoUpload, onSettings }: {
  signedIn: boolean;
  autoUpload: boolean;
  onSettings: () => void;
}) {
  const [shots, setShots] = useState<Screenshot[] | null>(null);
  const [busy, setBusy] = useState<Record<number, boolean>>({});
  const [errors, setErrors] = useState<Record<number, string>>({});
  const thumbs = useThumbnails(shots ?? []);

  const load = useCallback(() => {
    invoke<Screenshot[]>("get_recent_screenshots").then(setShots).catch((e) => console.error(e));
  }, []);

  useEffect(() => {
    load();
    const a = listen("new-screenshot", load);
    const b = listen("screenshot-uploaded", load);
    return () => { a.then((f) => f()); b.then((f) => f()); };
  }, [load]);

  const upload = async (s: Screenshot) => {
    setBusy((b) => ({ ...b, [s.id]: true }));
    setErrors((e) => ({ ...e, [s.id]: "" }));
    try {
      await invoke("upload_screenshot", { screenshotId: s.id });
      load();
    } catch (e) {
      setErrors((x) => ({ ...x, [s.id]: String(e) }));
    } finally {
      setBusy((b) => ({ ...b, [s.id]: false }));
    }
  };

  const state = (s: Screenshot) => {
    if (s.remoteHash) {
      return (
        <>
          <span className="ok">{autoUpload ? "Uploaded by itself" : "Uploaded"}</span>
          {s.remoteUrl && <button className="quiet" onClick={() => openUrl(s.remoteUrl!)}>View ↗</button>}
        </>
      );
    }
    if (!s.sessionId) return <span className="faint" title="No telemetry was recorded when it was taken, so no flight has it">Not on a recorded flight</span>;
    if (!signedIn) return <span className="faint">Sign in to upload</span>;
    return (
      <>
        <span className="dim">Not uploaded</span>
        <button className="primary" disabled={busy[s.id]} onClick={() => upload(s)}>
          {busy[s.id] ? "Uploading…" : "Upload"}
        </button>
      </>
    );
  };

  return (
    <div className="page">
      <div className="page-head">
        <h2>Screenshots</h2>
        <span className="sub">
          {autoUpload ? "Taken while flying, they go up by themselves." : "Upload the ones you want on the flight's page."}{" "}
          <button className="quiet" onClick={onSettings}>Settings</button>
        </span>
      </div>
      {shots === null ? null : shots.length === 0 ? (
        <div className="empty">
          No screenshots yet. Ones taken while a simulator is connected show here, from the folders set in Settings.
        </div>
      ) : (
        <div className="shots">
          {shots.map((s) => (
            <div className="shot" key={s.id}>
              <div className="thumb" title="Show in its folder" onClick={() => invoke("reveal_screenshot", { id: s.id }).catch(console.error)}>
                {thumbs[s.id] ? <img src={thumbs[s.id]} alt="" /> : <span>…</span>}
              </div>
              <div className="meta">
                <span className="title" title={s.aircraftTitle}>{s.aircraftTitle || "Unknown aircraft"}</span>
                <span className="dim">{takenAt(s.timestamp)}</span>
                <div className="state">{state(s)}</div>
                {errors[s.id] && <span className="bad">{errors[s.id]}</span>}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
