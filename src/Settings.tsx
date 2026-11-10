import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { Config, ScreenshotFolder, ServiceStatus } from "./models";
import { FlystoSettings } from "./FlystoSettings";
import { XPlanePluginSettings } from "./XPlanePluginSettings";

function isValidRegex(pattern: string): boolean {
    try { new RegExp(pattern); return true; } catch { return false; }
}

export function Settings({ serviceStatus }: { serviceStatus: ServiceStatus | null }) {
    const [config, setConfig] = useState<Config | null>(null);
    const [status, setStatus] = useState<string>("");
    const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

    const isLoggedIn = !!(config && config.apiToken && config.apiToken.trim().length > 0);

    useEffect(() => {
        invoke<Config>("get_config")
            .then(async (cfg) => {
                // Double check actual autostart status
                try {
                    const active = await isEnabled();
                    setConfig({ ...cfg, openAtLogin: active });
                } catch (e) {
                    setConfig(cfg);
                }
            })
            .catch(err => setStatus("Error loading config: " + err));
    }, []);

    // If the backend forces a logout (token rejected with 401), reload the
    // config so this view reflects the disconnected state.
    useEffect(() => {
        const unlisten = listen("auth-logout", () => {
            invoke<Config>("get_config")
                .then(cfg => setConfig(prev => prev ? { ...cfg, openAtLogin: prev.openAtLogin } : cfg))
                .catch(() => {});
            setStatus("Session expired — you've been logged out.");
        });
        return () => { unlisten.then(f => f()); };
    }, []);

    const persist = async (next: Config) => {
        try {
            await invoke("set_config", { config: next });
            setStatus("All changes saved");
            setTimeout(() => setStatus(""), 2000);
        } catch (err) {
            setStatus("Error saving config: " + err);
        }
    };

    const handleChange = (key: keyof Config, value: any) => {
        if (!config) return;
        const next: Config = { ...config, [key]: value };
        setConfig(next);
        persist(next);
    };

    // Autostart is owned by the OS plugin; only persist the flag once the plugin
    // call succeeds so the checkbox can't drift from the real autostart state.
    const handleAutostartChange = async (value: boolean) => {
        if (!config) return;
        try {
            if (value) await enable(); else await disable();
        } catch (e) {
            setStatus("Failed to update autostart: " + e);
            return;
        }
        const next: Config = { ...config, openAtLogin: value };
        setConfig(next);
        persist(next);
    };

    useEffect(() => () => { if (saveTimer.current) clearTimeout(saveTimer.current); }, []);

    // Typing a pattern saves on a debounce, like the other text inputs.
    const setFolders = (folders: ScreenshotFolder[], debounce = false) => {
        if (!config) return;
        const next: Config = { ...config, screenshotFolders: folders };
        setConfig(next);
        if (saveTimer.current) clearTimeout(saveTimer.current);
        if (debounce) saveTimer.current = setTimeout(() => persist(next), 400);
        else persist(next);
    };
    const setFolder = (i: number, folder: ScreenshotFolder, debounce = false) => {
        if (!config) return;
        setFolders(config.screenshotFolders.map((f, j) => (j === i ? folder : f)), debounce);
    };

    const pickDirectory = async (): Promise<string | null> => {
        const selected = await open({ directory: true, multiple: false });
        return typeof selected === "string" ? selected : null;
    };

    if (!config) return <div>Loading settings...</div>;

    return (
        <div className="settings-page page" style={{ maxWidth: 760 }}>
            <div className="page-head">
                <h2>Settings</h2>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: "1.5rem" }}>
                <section>
                    <h4>App Behavior</h4>
                    <div className="settings-stack">
                        <div className="setting-control">
                            <label>
                                <input
                                    type="checkbox"
                                    checked={config.openAtLogin}
                                    onChange={(e) => handleAutostartChange(e.target.checked)}
                                />
                                <span>Start automatically on login</span>
                            </label>
                        </div>
                        <div className="setting-control">
                            <label>
                                <input 
                                    type="checkbox" 
                                    checked={config.startMinimized} 
                                    onChange={(e) => handleChange("startMinimized", e.target.checked)}
                                /> 
                                <span>Start minimized to tray</span>
                            </label>
                        </div>
                    </div>
                </section>

                <section>
                    <h4>Screenshot folders</h4>
                    <div className="settings-stack">
                        <span className="setting-hint">
                            Watched while a simulator is connected. Each folder can send its screenshots
                            up by itself, and take only the files whose name matches a pattern.
                        </span>
                        {(config.screenshotFolders || []).map((folder, i) => (
                            <div key={folder.path} className="folder">
                                <div className="row">
                                    <input type="text" className="setting-input" style={{ flex: 1 }} value={folder.path} readOnly title={folder.path} />
                                    <button
                                        onClick={async () => {
                                            const picked = await pickDirectory();
                                            if (picked && !config.screenshotFolders.some((f, j) => f.path === picked && j !== i)) {
                                                setFolder(i, { ...folder, path: picked });
                                            }
                                        }}
                                        className="btn-ghost-amber"
                                    >Browse…</button>
                                    <button
                                        title="Stop watching this folder"
                                        onClick={() => setFolders(config.screenshotFolders.filter((_, j) => j !== i))}
                                        className="btn-ghost-red"
                                    >✕</button>
                                </div>
                                <div className="row">
                                    <label className="row" style={{ cursor: "pointer" }}>
                                        <input
                                            type="checkbox"
                                            checked={folder.autoUpload}
                                            onChange={(e) => setFolder(i, { ...folder, autoUpload: e.target.checked })}
                                        />
                                        <span>Upload by itself</span>
                                    </label>
                                    <input
                                        type="text"
                                        style={{ flex: 1 }}
                                        className="mono"
                                        value={folder.regex}
                                        onChange={(e) => setFolder(i, { ...folder, regex: e.target.value }, true)}
                                        placeholder="Every file (optional pattern, e.g. ^X-Plane)"
                                        title="Only files whose name matches this regular expression, case aside"
                                    />
                                </div>
                                {folder.regex.trim() && !isValidRegex(folder.regex.trim()) && (
                                    <span className="setting-hint bad">Not a valid pattern: every file of this folder is taken.</span>
                                )}
                            </div>
                        ))}
                        <div className="row">
                            <button
                                onClick={async () => {
                                    const dir = await pickDirectory();
                                    if (dir && !config.screenshotFolders.some((f) => f.path === dir)) {
                                        setFolders([...config.screenshotFolders, { path: dir, autoUpload: false, regex: "" }]);
                                    }
                                }}
                                className="btn-ghost-amber"
                            >+ Add folder</button>
                        </div>
                    </div>
                </section>

                <FlystoSettings status={serviceStatus} />

                <XPlanePluginSettings />

                <section>
                    <h4>Multiplayer</h4>
                    <div className="settings-stack">
                        <div className="setting-control" style={{ opacity: isLoggedIn ? 1 : 0.5 }}>
                            <label style={{ cursor: isLoggedIn ? "pointer" : "not-allowed" }}>
                                <input
                                    type="checkbox"
                                    checked={config.injectButterlogTraffic}
                                    onChange={(e) => handleChange("injectButterlogTraffic", e.target.checked)}
                                    disabled={!isLoggedIn}
                                />
                                <span>Inject traffic from other butterlog users</span>
                            </label>
                            {!isLoggedIn && (
                                <span className="setting-hint">
                                    Sign in on the Service page first.
                                </span>
                            )}
                        </div>
                    </div>
                </section>

                <div className="settings-status" aria-live="polite" style={{ marginTop: "1rem", minHeight: "1.2rem", fontSize: "0.85rem", fontFamily: "var(--bl-mono)", letterSpacing: "0.04em" }}>
                    {status && <span style={{ color: /^(Error|Failed)/.test(status) ? "var(--bl-caution)" : "var(--bl-teal)" }}>{status}</span>}
                </div>
            </div>
        </div>
    );
}
