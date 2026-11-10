import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { Config } from "./models";
import { XPlanePluginSettings } from "./XPlanePluginSettings";

function isValidRegex(pattern: string): boolean {
    try { new RegExp(pattern); return true; } catch { return false; }
}

export function Settings() {
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

    // Text inputs save on a debounce so we don't write the config to disk (and
    // flash "saved") on every keystroke.
    const handleTextChange = (key: keyof Config, value: string) => {
        if (!config) return;
        const next: Config = { ...config, [key]: value };
        setConfig(next);
        if (saveTimer.current) clearTimeout(saveTimer.current);
        saveTimer.current = setTimeout(() => persist(next), 400);
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
                        <div className="setting-input-group">
                            <label>Watched while a simulator is connected:</label>
                            <div style={{ display: "flex", flexDirection: "column", gap: "0.4rem", marginTop: "0.25rem" }}>
                                {(config.screenshotDirectories || []).map((dir, i) => (
                                    <div key={i} style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
                                        <input
                                            type="text"
                                            className="setting-input"
                                            style={{ flex: 1 }}
                                            value={dir}
                                            readOnly
                                            title={dir}
                                        />
                                        <button
                                            onClick={async () => {
                                                const picked = await pickDirectory();
                                                if (picked && !(config.screenshotDirectories || []).some((d, j) => d === picked && j !== i)) {
                                                    const dirs = [...config.screenshotDirectories];
                                                    dirs[i] = picked;
                                                    handleChange("screenshotDirectories", dirs);
                                                }
                                            }}
                                            className="btn-ghost-amber"
                                        >Browse…</button>
                                        <button
                                            onClick={() => handleChange("screenshotDirectories", config.screenshotDirectories.filter((_, j) => j !== i))}
                                            className="btn-ghost-red"
                                        >✕</button>
                                    </div>
                                ))}
                                <button
                                    onClick={async () => {
                                        const dir = await pickDirectory();
                                        if (dir && !(config.screenshotDirectories || []).includes(dir)) {
                                            handleChange("screenshotDirectories", [...(config.screenshotDirectories || []), dir]);
                                        }
                                    }}
                                    className="btn-ghost-amber" style={{ alignSelf: "flex-start" }}
                                >+ Add directory</button>
                            </div>
                        </div>
                    </div>
                </section>

                <section>
                    <h4>Screenshots</h4>
                    <div className="settings-stack">
                        <div className="setting-control">
                            <label>
                                <input
                                    type="checkbox"
                                    checked={config.autoUploadScreenshots}
                                    onChange={(e) => handleChange("autoUploadScreenshots", e.target.checked)}
                                />
                                <span>Upload screenshots by themselves</span>
                            </label>
                        </div>
                        {config.autoUploadScreenshots && (
                            <>
                                <div className="setting-control">
                                    <label>
                                        <input
                                            type="checkbox"
                                            checked={config.screenshotRegexEnabled}
                                            onChange={(e) => handleChange("screenshotRegexEnabled", e.target.checked)}
                                        />
                                        <span>Only those whose file name matches</span>
                                    </label>
                                </div>
                                <input
                                    type="text"
                                    className="setting-input"
                                    value={config.screenshotRegex}
                                    onChange={(e) => handleTextChange("screenshotRegex", e.target.value)}
                                    disabled={!config.screenshotRegexEnabled}
                                    placeholder="Only auto-upload files matching this regex"
                                />
                                {config.screenshotRegexEnabled && config.screenshotRegex && !isValidRegex(config.screenshotRegex) && (
                                    <span className="setting-hint">Invalid regular expression — this filter won't match anything.</span>
                                )}
                            </>
                        )}
                    </div>
                </section>



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
