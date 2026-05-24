import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

interface Config {
    logDirectory: string | null;
    screenshotDirectory: string | null;
    geotagScreenshots: boolean;
    screenshotRegexEnabled: boolean;
    screenshotRegex: string;
    autoUploadScreenshots: boolean;
    enableWebhook: boolean;
    webhookUrl: string;
    openAtLogin: boolean;
    startMinimized: boolean;
    enableMultiplayerHubs: boolean;
    injectButterlogTraffic: boolean;
}

export function Settings({ onBack }: { onBack: () => void }) {
    const [config, setConfig] = useState<Config | null>(null);
    const [status, setStatus] = useState<string>("");
    const [loginLoading, setLoginLoading] = useState<boolean>(false);

    const isLoggedIn = !!(config && config.webhookUrl && config.webhookUrl.startsWith("https://butterlog.flyvoyager.net/api/v0/users/") && config.webhookUrl.replace("https://butterlog.flyvoyager.net/api/v0/users/", "").trim().length > 0);

    const handleDiscordLogin = async () => {
        setLoginLoading(true);
        setStatus("Opening browser for Discord login...");
        try {
            const token = await invoke<string>("start_discord_login");
            setStatus("Successfully authenticated with ButterLog service!");
            if (config) {
                setConfig({
                    ...config,
                    webhookUrl: `https://butterlog.flyvoyager.net/api/v0/users/${token}`,
                    enableWebhook: true
                });
            }
        } catch (err) {
            setStatus("Authentication failed: " + err);
        } finally {
            setLoginLoading(false);
        }
    };

    const handleDiscordLogout = async () => {
        if (!config) return;
        setStatus("Logging out of ButterLog service...");
        try {
            const updatedConfig = {
                ...config,
                webhookUrl: "",
                enableWebhook: false,
                injectButterlogTraffic: false,
                enableMultiplayerHubs: false
            };
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

    const handleSave = async () => {
        if (!config) return;
        try {
            // Handle autostart plugin
            try {
                if (config.openAtLogin) {
                    await enable();
                } else {
                    await disable();
                }
            } catch (e) {
                console.error("Failed to update autostart:", e);
            }

            const configToSave = {
                ...config,
                enableMultiplayerHubs: config.injectButterlogTraffic
            };
            await invoke("set_config", { config: configToSave });
            setConfig(configToSave);
            setStatus("Settings saved successfully!");
            setTimeout(() => setStatus(""), 3000);
        } catch (err) {
            setStatus("Error saving config: " + err);
        }
    };

    const handleChange = (key: keyof Config, value: any) => {
        if (!config) return;
        if (key === "injectButterlogTraffic") {
            setConfig({
                ...config,
                injectButterlogTraffic: value,
                enableMultiplayerHubs: value
            });
        } else {
            setConfig({ ...config, [key]: value });
        }
    };

    if (!config) return <div>Loading settings...</div>;

    return (
        <div className="settings-page" style={{ textAlign: "left", padding: "1rem", maxWidth: "800px", margin: "0 auto" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "2rem" }}>
                <h2>Settings</h2>
                <button onClick={onBack}>Back to Dashboard</button>
            </div>

            <div style={{ display: "flex", flexDirection: "column", gap: "1.5rem" }}>
                <section>
                    <h4>App Behavior</h4>
                    <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
                        <div className="setting-control">
                            <label>
                                <input 
                                    type="checkbox" 
                                    checked={config.openAtLogin} 
                                    onChange={(e) => handleChange("openAtLogin", e.target.checked)}
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
                    <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
                        <div className="setting-input-group">
                            <label>Log Directory:</label>
                            <input 
                                type="text" 
                                className="setting-input"
                                value={config.logDirectory || ""} 
                                onChange={(e) => handleChange("logDirectory", e.target.value || null)}
                                placeholder="Default (App Data)"
                            />
                        </div>
                        <div className="setting-input-group">
                            <label>Screenshot Directory:</label>
                            <input 
                                type="text" 
                                className="setting-input"
                                value={config.screenshotDirectory || ""} 
                                onChange={(e) => handleChange("screenshotDirectory", e.target.value || null)}
                                placeholder="Default (App Data)"
                            />
                        </div>
                    </div>
                </section>

                <section>
                    <h4>Screenshots</h4>
                    <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
                        <div className="setting-control">
                            <label>
                                <input 
                                    type="checkbox" 
                                    checked={config.geotagScreenshots} 
                                    onChange={(e) => handleChange("geotagScreenshots", e.target.checked)}
                                /> 
                                <span>Geotag Screenshots</span>
                            </label>
                        </div>
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
                            onChange={(e) => handleChange("screenshotRegex", e.target.value)}
                            disabled={!config.screenshotRegexEnabled}
                        />
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
                    </div>
                </section>



                <section>
                    <h4>ButterLog Service Authentication</h4>
                    <div style={{
                        background: "rgba(255, 255, 255, 0.02)",
                        border: "1px solid rgba(255, 255, 255, 0.08)",
                        borderRadius: "12px",
                        padding: "1.5rem",
                        display: "flex",
                        flexDirection: "column",
                        gap: "1rem",
                        position: "relative",
                        overflow: "hidden"
                    }}>
                        {isLoggedIn && (
                            <div style={{
                                position: "absolute",
                                top: "-50%",
                                right: "-50%",
                                width: "200px",
                                height: "200px",
                                background: "radial-gradient(circle, rgba(166, 227, 161, 0.15) 0%, rgba(0,0,0,0) 70%)",
                                pointerEvents: "none"
                            }} />
                        )}
                        
                        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: "1rem" }}>
                            <div style={{ display: "flex", alignItems: "center", gap: "0.75rem" }}>
                                <div style={{
                                    backgroundColor: isLoggedIn ? "rgba(166, 227, 161, 0.1)" : "rgba(88, 101, 242, 0.1)",
                                    borderRadius: "50%",
                                    width: "48px",
                                    height: "48px",
                                    display: "flex",
                                    alignItems: "center",
                                    justifyContent: "center",
                                    border: isLoggedIn ? "1px solid rgba(166, 227, 161, 0.2)" : "1px solid rgba(88, 101, 242, 0.2)",
                                }}>
                                    {isLoggedIn ? (
                                        <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#a6e3a1" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                                            <polyline points="20 6 9 17 4 12"></polyline>
                                        </svg>
                                    ) : (
                                        <svg width="24" height="24" viewBox="0 0 127.14 96.36" fill="#5865F2">
                                            <path d="M107.7,8.07A105.15,105.15,0,0,0,77.26,0a77.19,77.19,0,0,0-3.3,6.83A96.67,96.67,0,0,0,53.22,6.83,77.19,77.19,0,0,0,49.88,0,105.15,105.15,0,0,0,19.44,8.07C3.66,31.58-1.86,54.65,1,77.53A105.73,105.73,0,0,0,32,96.36a77.7,77.7,0,0,0,6.63-10.85,68.43,68.43,0,0,1-10.4-5c.88-.65,1.72-1.33,2.53-2a75.46,75.46,0,0,0,72.63,0c.81.71,1.65,1.39,2.53,2a68.43,68.43,0,0,1-10.4,5,77.7,77.7,0,0,0,6.63,10.85,105.73,105.73,0,0,0,31-18.83C129,54.65,122.56,31.58,107.7,8.07ZM42.45,65.69C36.18,65.69,31,60,31,53S36.18,40.36,42.45,40.36,53.83,46,53.83,53,48.72,65.69,42.45,65.69Zm42.24,0C78.41,65.69,73.24,60,73.24,53S78.41,40.36,84.69,40.36,96.07,46,96.07,53,91,65.69,84.69,65.69Z"/>
                                        </svg>
                                    )}
                                </div>
                                <div>
                                    <div style={{ fontWeight: "bold", fontSize: "1rem", color: isLoggedIn ? "#a6e3a1" : "#cdd6f4" }}>
                                        {isLoggedIn ? "Status: Connected" : "Status: Not Connected"}
                                    </div>
                                    <div style={{ fontSize: "0.85rem", color: "#a6adc8", marginTop: "2px" }}>
                                        {isLoggedIn 
                                            ? "Your Discord account is linked to the ButterLog service." 
                                            : "Link your Discord account to sync logs and upload screenshots."}
                                    </div>
                                </div>
                            </div>
                            
                            {isLoggedIn ? (
                                <button 
                                    onClick={handleDiscordLogout} 
                                    style={{ 
                                        backgroundColor: "rgba(243, 139, 168, 0.1)", 
                                        color: "#f38ba8", 
                                        border: "1px solid rgba(243, 139, 168, 0.2)", 
                                        padding: "0.65rem 1.25rem", 
                                        borderRadius: "8px",
                                        fontWeight: "bold",
                                        cursor: "pointer",
                                        transition: "all 0.2s ease-in-out",
                                        display: "flex",
                                        alignItems: "center",
                                        gap: "0.5rem"
                                    }}
                                    onMouseOver={(e) => {
                                        e.currentTarget.style.backgroundColor = "rgba(243, 139, 168, 0.2)";
                                        e.currentTarget.style.borderColor = "rgba(243, 139, 168, 0.35)";
                                    }}
                                    onMouseOut={(e) => {
                                        e.currentTarget.style.backgroundColor = "rgba(243, 139, 168, 0.1)";
                                        e.currentTarget.style.borderColor = "rgba(243, 139, 168, 0.2)";
                                    }}
                                >
                                    Log Out
                                </button>
                            ) : (
                                <button 
                                    onClick={handleDiscordLogin} 
                                    disabled={loginLoading}
                                    style={{ 
                                        backgroundColor: "#5865F2", 
                                        color: "white", 
                                        border: "none", 
                                        padding: "0.65rem 1.25rem", 
                                        borderRadius: "8px",
                                        fontWeight: "bold",
                                        cursor: loginLoading ? "not-allowed" : "pointer",
                                        opacity: loginLoading ? 0.7 : 1,
                                        transition: "all 0.2s ease-in-out",
                                        display: "flex",
                                        alignItems: "center",
                                        gap: "0.5rem",
                                        boxShadow: "0 4px 14px rgba(88, 101, 242, 0.3)"
                                    }}
                                    onMouseOver={(e) => {
                                        if (!loginLoading) {
                                            e.currentTarget.style.backgroundColor = "#4752C4";
                                            e.currentTarget.style.transform = "translateY(-1px)";
                                        }
                                    }}
                                    onMouseOut={(e) => {
                                        if (!loginLoading) {
                                            e.currentTarget.style.backgroundColor = "#5865F2";
                                            e.currentTarget.style.transform = "translateY(0)";
                                        }
                                    }}
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
                        <div className="setting-control" style={{ opacity: isLoggedIn ? 1 : 0.5, borderTop: "1px solid rgba(255,255,255,0.05)", paddingTop: "1rem", marginTop: "0.5rem" }}>
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
                                <span style={{ fontSize: "0.75rem", color: "#f38ba8", marginLeft: "28px", display: "block", marginTop: "2px" }}>
                                    Requires connection to ButterLog service.
                                </span>
                            )}
                        </div>
                        <p style={{ fontSize: "0.8rem", color: "#888", margin: "0", borderTop: "1px solid rgba(255,255,255,0.05)", paddingTop: "0.5rem" }}>
                            We use Discord to login to not make you create yet another account.
                        </p>
                    </div>
                </section>



                <div style={{ marginTop: "1rem", display: "flex", alignItems: "center", gap: "1rem" }}>
                    <button onClick={handleSave} style={{ backgroundColor: "#4caf50", color: "white" }}>Save Settings</button>
                    {status && <span style={{ color: status.startsWith("Error") ? "#f44336" : "#4caf50" }}>{status}</span>}
                </div>
            </div>
        </div>
    );
}
