import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { open, ask } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { Config } from "./models";

function isValidRegex(pattern: string): boolean {
    try { new RegExp(pattern); return true; } catch { return false; }
}

export function Settings() {
    const [config, setConfig] = useState<Config | null>(null);
    const [status, setStatus] = useState<string>("");
    const [loginLoading, setLoginLoading] = useState<boolean>(false);
    const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

    const isLoggedIn = !!(config && config.apiToken && config.apiToken.trim().length > 0);

    const handleDiscordLogin = async () => {
        setLoginLoading(true);
        setStatus("Opening browser for Discord login...");
        try {
            await invoke<string>("start_discord_login");
            // The backend saved the token and service URL; reload the config
            // so this view reflects the logged-in state.
            const cfg = await invoke<Config>("get_config");
            setConfig(prev => prev ? { ...cfg, openAtLogin: prev.openAtLogin } : cfg);
            setStatus("Successfully authenticated with ButterLog service!");
        } catch (err) {
            setStatus("Authentication failed: " + err);
        } finally {
            setLoginLoading(false);
        }
    };

    const handleDiscordLogout = async () => {
        if (!config) return;
        const confirmed = await ask(
            "Disconnect your Discord account from the ButterLog service? Your multiplayer and sharing preferences are kept and resume when you reconnect.",
            { title: "Log Out", kind: "warning" }
        );
        if (!confirmed) return;
        setStatus("Logging out of ButterLog service...");
        try {
            // Mirror the backend's force_logout: clear only the credentials and
            // leave traffic/sharing preferences intact so they resume on re-login.
            const updatedConfig = { ...config, apiToken: "", enableWebhook: false };
            setConfig(updatedConfig);
            await invoke("set_config", { config: updatedConfig });
            setStatus("Logged out successfully.");
            setTimeout(() => setStatus(""), 3000);
        } catch (err) {
            setStatus("Failed to logout: " + err);
        }
    };

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
        <div className="settings-page page page-narrow">
            <div className="view-header">
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
                    <h4>Directories</h4>
                    <div className="settings-stack">
                        <div className="setting-input-group">
                            <label>Exported log directory:</label>
                            <div style={{ display: "flex", gap: "0.5rem", alignItems: "center", marginTop: "0.25rem" }}>
                                <input
                                    type="text"
                                    className="setting-input"
                                    style={{ flex: 1 }}
                                    value={config.logDirectory || ""}
                                    readOnly
                                    title={config.logDirectory || ""}
                                    placeholder="Default: Documents/butterlog"
                                />
                                <button
                                    onClick={async () => {
                                        const dir = await pickDirectory();
                                        if (dir) handleChange("logDirectory", dir);
                                    }}
                                    className="btn-ghost-amber"
                                >Browse…</button>
                                {config.logDirectory && (
                                    <button
                                        onClick={() => handleChange("logDirectory", null)}
                                        title="Reset to default"
                                        className="btn-ghost-red"
                                    >✕</button>
                                )}
                            </div>
                        </div>
                        <div className="setting-input-group">
                            <label>Screenshot Directories:</label>
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
                                <span>Auto-upload Screenshots</span>
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
                                        <span>Enable Screenshot Window Regex</span>
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
                                    Requires connection to ButterLog service.
                                </span>
                            )}
                        </div>
                    </div>
                </section>

                <section>
                    <h4>ButterLog Service</h4>
                    <div className={`auth-card${isLoggedIn ? " connected" : ""}`}>
                        <div className="auth-head">
                            <div className="auth-id">
                                <div className={`auth-badge${isLoggedIn ? " connected" : ""}`}>
                                    {isLoggedIn ? (
                                        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#36e3c6" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                                            <polyline points="20 6 9 17 4 12"></polyline>
                                        </svg>
                                    ) : (
                                        <svg width="24" height="24" viewBox="0 0 127.14 96.36" fill="#5865F2">
                                            <path d="M107.7,8.07A105.15,105.15,0,0,0,77.26,0a77.19,77.19,0,0,0-3.3,6.83A96.67,96.67,0,0,0,53.22,6.83,77.19,77.19,0,0,0,49.88,0,105.15,105.15,0,0,0,19.44,8.07C3.66,31.58-1.86,54.65,1,77.53A105.73,105.73,0,0,0,32,96.36a77.7,77.7,0,0,0,6.63-10.85,68.43,68.43,0,0,1-10.4-5c.88-.65,1.72-1.33,2.53-2a75.46,75.46,0,0,0,72.63,0c.81.71,1.65,1.39,2.53,2a68.43,68.43,0,0,1-10.4,5,77.7,77.7,0,0,0,6.63,10.85,105.73,105.73,0,0,0,31-18.83C129,54.65,122.56,31.58,107.7,8.07ZM42.45,65.69C36.18,65.69,31,60,31,53S36.18,40.36,42.45,40.36,53.83,46,53.83,53,48.72,65.69,42.45,65.69Zm42.24,0C78.41,65.69,73.24,60,73.24,53S78.41,40.36,84.69,40.36,96.07,46,96.07,53,91,65.69,84.69,65.69Z"/>
                                        </svg>
                                    )}
                                </div>
                                <div>
                                    <div className={`auth-title${isLoggedIn ? " connected" : ""}`}>
                                        {isLoggedIn ? "Connected" : "Not Connected"}
                                    </div>
                                    <div className="auth-sub">
                                        {isLoggedIn
                                            ? "Your Discord account is linked to the ButterLog service."
                                            : "Link your Discord account to sync logs and upload screenshots."}
                                    </div>
                                </div>
                            </div>

                            {isLoggedIn ? (
                                <button onClick={handleDiscordLogout} className="btn-logout">
                                    Log Out
                                </button>
                            ) : (
                                <button
                                    onClick={handleDiscordLogin}
                                    disabled={loginLoading}
                                    className="btn-discord"
                                >
                                    {loginLoading ? (
                                        <>
                                            <svg style={{ animation: "spin 1s linear infinite" }} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3">
                                                <circle cx="12" cy="12" r="10" stroke="rgba(255,255,255,0.2)" />
                                                <path d="M12 2a10 10 0 0 1 10 10" stroke="currentColor" />
                                            </svg>
                                            <span>Connecting...</span>
                                        </>
                                    ) : (
                                        "Connect with Discord"
                                    )}
                                </button>
                            )}
                        </div>
                        <div className="setting-control auth-row" style={{ opacity: isLoggedIn ? 1 : 0.5 }}>
                            <label style={{ cursor: isLoggedIn ? "pointer" : "not-allowed" }}>
                                <input
                                    type="checkbox"
                                    checked={config.autoShareFlights}
                                    onChange={(e) => handleChange("autoShareFlights", e.target.checked)}
                                    disabled={!isLoggedIn}
                                />
                                <span>Automatically share each completed flight</span>
                            </label>
                            {!isLoggedIn && (
                                <span className="setting-hint">
                                    Requires connection to ButterLog service.
                                </span>
                            )}
                        </div>
                        <div className="setting-control auth-row" style={{ opacity: isLoggedIn ? 1 : 0.5 }}>
                            <label style={{ cursor: isLoggedIn ? "pointer" : "not-allowed" }}>
                                <input
                                    type="checkbox"
                                    checked={config.shareLiveFlights}
                                    onChange={(e) => handleChange("shareLiveFlights", e.target.checked)}
                                    disabled={!isLoggedIn}
                                />
                                <span>Show flights live on the web while flying</span>
                            </label>
                            <span className="setting-hint">
                                Streams the track as you fly so the flight page updates in real time.
                            </span>
                        </div>
                        <div className="setting-control auth-row" style={{ opacity: isLoggedIn ? 1 : 0.5 }}>
                            <label style={{ cursor: isLoggedIn ? "pointer" : "not-allowed" }}>
                                <input
                                    type="checkbox"
                                    checked={config.allowRemoteCommands}
                                    onChange={(e) => handleChange("allowRemoteCommands", e.target.checked)}
                                    disabled={!isLoggedIn}
                                />
                                <span>Allow controlling the sim from your live flight page</span>
                            </label>
                            <span className="setting-hint">
                                Lets you pause the simulator from your own flight page. Only you can
                                send commands, and only while a flight is in progress.
                            </span>
                        </div>
                        {config.allowRemoteCommands && (
                            <div className="setting-control auth-row" style={{ opacity: isLoggedIn ? 1 : 0.5, marginLeft: "1.5rem" }}>
                                <label style={{ cursor: isLoggedIn ? "pointer" : "not-allowed" }}>
                                    <input
                                        type="checkbox"
                                        checked={config.allowBetaCommands}
                                        onChange={(e) => handleChange("allowBetaCommands", e.target.checked)}
                                        disabled={!isLoggedIn}
                                    />
                                    <span>Also allow autopilot controls (beta)</span>
                                </label>
                                <span className="setting-hint">
                                    Heading, altitude, V/S and mode switches. These drive the default
                                    autopilot — study-level add-ons such as PMDG or Fenix run their own
                                    and will ignore them.
                                </span>
                            </div>
                        )}
                        <p className="auth-note">
                            We use Discord to login to not make you create yet another account.
                        </p>
                    </div>
                </section>



                <div className="settings-status" aria-live="polite" style={{ marginTop: "1rem", minHeight: "1.2rem", fontSize: "0.85rem", fontFamily: "var(--bl-mono)", letterSpacing: "0.04em" }}>
                    {status && <span style={{ color: /^(Error|Failed)/.test(status) ? "var(--bl-caution)" : "var(--bl-teal)" }}>{status}</span>}
                </div>
            </div>
        </div>
    );
}
