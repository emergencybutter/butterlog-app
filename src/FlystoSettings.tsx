import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ServiceStatus } from "./models";

/// Where a FlySto account's API tokens are made.
const FLYSTO_API_PAGE = "https://www.flysto.net/account/api";

/// The pilot's FlySto API token: kept by the service, which checks it with FlySto first and
/// then sends each finished flight there. The token is never kept in the app.
export function FlystoSettings({ status }: { status: ServiceStatus | null }) {
    const [token, setToken] = useState("");
    const [busy, setBusy] = useState(false);
    const [message, setMessage] = useState<{ text: string; bad: boolean } | null>(null);

    const signedIn = !!status && status.link !== "signed_out";
    const connected = !!status?.me?.flystoConnected;

    const connect = async () => {
        setBusy(true);
        setMessage(null);
        try {
            await invoke("set_flysto_token", { token: token.trim() });
            setToken("");
            setMessage({ text: "Connected: flights you finish from now on go to FlySto by themselves.", bad: false });
        } catch (e) {
            setMessage({ text: String(e), bad: true });
        } finally {
            setBusy(false);
        }
    };

    const disconnect = async () => {
        setBusy(true);
        setMessage(null);
        try {
            await invoke("clear_flysto_token");
            setMessage({ text: "Disconnected: flights no longer go to FlySto.", bad: false });
        } catch (e) {
            setMessage({ text: String(e), bad: true });
        } finally {
            setBusy(false);
        }
    };

    return (
        <section>
            <h4>FlySto</h4>
            <div className="settings-stack">
                {!signedIn ? (
                    <span className="setting-hint">Sign in on the Service page first: the service sends your flights to FlySto.</span>
                ) : (
                    <>
                        <span className="setting-hint">
                            {connected
                                ? "Connected: each flight you finish goes to your FlySto account by itself, and its Discord post links it. Paste a new token to replace the one kept."
                                : "Send each flight you finish to your FlySto account by itself, with a link to it in its Discord post. Flights from before you connect aren't sent."}
                        </span>
                        {!connected && (
                            <ol className="setting-hint steps">
                                <li>
                                    Sign in to FlySto and open your account's API page:{" "}
                                    <button className="quiet link-button" onClick={() => openUrl(FLYSTO_API_PAGE)}>flysto.net/account/api ↗</button>
                                </li>
                                <li>Create an API token, and copy it.</li>
                                <li>Paste it below and press Connect. FlySto checks it before it is kept.</li>
                            </ol>
                        )}
                        <div className="row">
                            <input
                                type="password"
                                className="mono"
                                style={{ flex: 1 }}
                                value={token}
                                onChange={(e) => setToken(e.target.value)}
                                onKeyDown={(e) => { if (e.key === "Enter" && token.trim() && !busy) connect(); }}
                                placeholder="FlySto API token"
                                autoComplete="off"
                                spellCheck={false}
                            />
                            <button className="primary" disabled={busy || !token.trim()} onClick={connect}>
                                {busy ? "Checking…" : connected ? "Replace" : "Connect"}
                            </button>
                            {connected && <button disabled={busy} onClick={disconnect}>Disconnect</button>}
                        </div>
                        <span className="setting-hint">
                            The token goes to the ButterLog service, which keeps it only to upload your flights; this app doesn't store it.
                        </span>
                        {message && <span className={`setting-hint ${message.bad ? "bad" : "ok"}`}>{message.text}</span>}
                    </>
                )}
            </div>
        </section>
    );
}
