import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Config {
    logDirectory: string | null;
    screenshotDirectory: string | null;
    geotagScreenshots: boolean;
    screenshotRegexEnabled: boolean;
    screenshotRegex: string;
    autoUploadScreenshots: boolean;
    enableWebhook: boolean;
    webhookAddress: string;
    simulatorType: 'msfs' | 'xplane';
    xplaneWebsocketUrl: string;
}

export function Settings({ onBack }: { onBack: () => void }) {
    const [config, setConfig] = useState<Config | null>(null);
    const [status, setStatus] = useState<string>("");

    useEffect(() => {
        invoke<Config>("get_config")
            .then(setConfig)
            .catch(err => setStatus("Error loading config: " + err));
    }, []);

    const handleSave = async () => {
        if (!config) return;
        try {
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
                        <div className="setting-input-group">
                            <label>Simulator Type:</label>
                            <select 
                                className="setting-input"
                                value={config.simulatorType}
                                onChange={(e) => handleChange("simulatorType", e.target.value)}
                            >
                                <option value="msfs">Microsoft Flight Simulator (SimConnect)</option>
                                <option value="xplane">X-Plane 12 (REST/WebSocket)</option>
                            </select>
                        </div>
                        {config.simulatorType === 'xplane' && (
                            <div className="setting-input-group">
                                <label>X-Plane WebSocket URL:</label>
                                <input 
                                    type="text" 
                                    className="setting-input"
                                    value={config.xplaneWebsocketUrl} 
                                    onChange={(e) => handleChange("xplaneWebsocketUrl", e.target.value)}
                                    placeholder="ws://localhost:8080/api/v1/telemetry"
                                />
                            </div>
                        )}
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
                                <span>Enable Webhook</span>
                            </label>
                        </div>
                        <div className="setting-input-group">
                            <label>Webhook Address:</label>
                            <input 
                                type="text" 
                                className="setting-input"
                                value={config.webhookAddress} 
                                onChange={(e) => handleChange("webhookAddress", e.target.value)}
                                placeholder="https://..."
                                disabled={!config.enableWebhook}
                            />
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
