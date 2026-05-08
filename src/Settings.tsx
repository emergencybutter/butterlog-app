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
}

export function Settings({ onBack }: { onBack: () => void }) {
    const [config, setConfig] = useState<Config | null>(null);
    const [status, setStatus] = useState<string>("");

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

            await invoke("set_config", { config });
            setStatus("Settings saved successfully!");
            setTimeout(() => setStatus(""), 3000);
        } catch (err) {
            setStatus("Error saving config: " + err);
        }
    };

    const handleChange = (key: keyof Config, value: any) => {
        if (!config) return;
        setConfig({ ...config, [key]: value });
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
                    <h4>Simulator</h4>
                    <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
                        <p style={{ fontSize: "0.85rem", color: "#888", margin: "0 0 0.5rem 0" }}>
                            The app automatically detects and connects to Microsoft Flight Simulator and X-Plane.
                        </p>
                    </div>
                </section>

                <section>
                    <h4>Webhooks</h4>
                    <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
                        <div className="setting-control">
                            <label>
                                <input 
                                    type="checkbox" 
                                    checked={config.enableWebhook} 
                                    onChange={(e) => handleChange("enableWebhook", e.target.checked)}
                                /> 
                                <span>Enable Webhook Service</span>
                            </label>
                        </div>
                        <div className="setting-input-group">
                            <label>Authenticated Webhook URL:</label>
                            <input 
                                type="text" 
                                className="setting-input"
                                value={config.webhookUrl} 
                                onChange={(e) => handleChange("webhookUrl", e.target.value)}
                                placeholder="https://butterlog.flyvoyager.net/api/users/YOUR_TOKEN"
                                disabled={!config.enableWebhook}
                            />
                            <p style={{ fontSize: "0.75rem", color: "#888", marginTop: "4px" }}>
                                Copy the full URL from your Butterlog dashboard. It should include your private token.
                            </p>
                        </div>
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
