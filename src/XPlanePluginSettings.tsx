import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// Mirrors xp_plugin_install.rs.
interface InstallStatus {
    path: string;
    installed: boolean;
    installedVersion: string | null;
    upToDate: boolean;
    restartNeeded: boolean;
}

interface PluginStatus {
    bundledVersion: string | null;
    installs: InstallStatus[];
}

function describe(install: InstallStatus, bundled: string | null): string {
    if (!install.installed) return "Not installed";
    if (install.upToDate) {
        const version = install.installedVersion ?? bundled;
        const base = version ? `Installed, version ${version}` : "Installed";
        return install.restartNeeded ? `${base}. Restart X-Plane to load it.` : base;
    }
    return install.installedVersion
        ? `Version ${install.installedVersion} installed; ${bundled ?? "a newer one"} is available`
        : `An older version is installed; ${bundled ?? "a newer one"} is available`;
}

/// The X-Plane plugin: where it's installed, and a button to install or update it.
export function XPlanePluginSettings() {
    const [status, setStatus] = useState<PluginStatus | null>(null);
    const [busy, setBusy] = useState<string | null>(null);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        invoke<PluginStatus>("get_xplane_plugin_status").then(setStatus).catch((e) => setError(String(e)));
    }, []);

    const installAt = async (path: string) => {
        setBusy(path);
        setError(null);
        try {
            setStatus(await invoke<PluginStatus>("install_xplane_plugin", { path }));
        } catch (e) {
            setError(String(e));
        } finally {
            setBusy(null);
        }
    };

    if (!status) return null;

    return (
        <section>
            <h4>X-Plane Plugin</h4>
            <div className="settings-stack">
                <span className="setting-hint">
                    The plugin shows other ButterLog pilots in X-Plane 12 and records your flights
                    from every sim frame. ButterLog keeps it up to date wherever it's installed.
                </span>
                {status.installs.length === 0 && (
                    <span className="setting-hint">No X-Plane 12 installation found on this computer.</span>
                )}
                {status.installs.map((entry) => (
                    <div key={entry.path} className="setting-input-group">
                        <label title={entry.path}>{entry.path}</label>
                        <div style={{ display: "flex", gap: "0.5rem", alignItems: "center", marginTop: "0.25rem" }}>
                            <span style={{ flex: 1 }}>{describe(entry, status.bundledVersion)}</span>
                            {status.bundledVersion && !entry.upToDate && (
                                <button
                                    onClick={() => installAt(entry.path)}
                                    disabled={busy !== null}
                                    className="btn-ghost-amber"
                                >
                                    {busy === entry.path ? "Installing…" : entry.installed ? "Update" : "Install"}
                                </button>
                            )}
                        </div>
                    </div>
                ))}
                {!status.bundledVersion && status.installs.length > 0 && (
                    <span className="setting-hint">This build of ButterLog doesn't include the plugin.</span>
                )}
                {error && <span className="setting-hint" style={{ color: "var(--bl-caution)" }}>{error}</span>}
            </div>
        </section>
    );
}
