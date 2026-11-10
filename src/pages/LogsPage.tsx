import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const Icons = {
  Copy: () => (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
    </svg>
  )
};

/// Backend logs: what it was as a Status tab. The lines come from the window, which keeps
/// them from its start.
export function LogsPage({ logs }: { logs: string[] }) {
  const [copiedLogs, setCopiedLogs] = useState(false);

  const handleCopyLogs = useCallback(() => {
    const text = logs.join("\n");
    navigator.clipboard.writeText(text)
      .then(() => {
        setCopiedLogs(true);
        setTimeout(() => setCopiedLogs(false), 2000);
      })
      .catch((err) => console.error("Failed to copy logs:", err));
  }, [logs]);

  return (
    <div className="page">
    <div className="logs-container" style={{ textAlign: "left" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "1rem" }}>
        <h3 style={{ margin: 0 }}>Backend Logs</h3>
        <div style={{ display: "flex", gap: "8px" }}>
        <button
          onClick={() => invoke("reveal_log_file").catch(console.error)}
          title="Show the log file, which keeps the last 7 days"
          style={{
            background: "#14181a",
            color: "#eee",
            border: "1px solid #444",
            padding: "6px 12px",
            borderRadius: "6px",
            fontSize: "0.85rem",
            fontWeight: "bold",
            cursor: "pointer"
          }}
        >
          Open Log File
        </button>
        <button
          onClick={handleCopyLogs}
          style={{
            display: "flex",
            alignItems: "center",
            gap: "6px",
            background: copiedLogs ? "#4caf50" : "#14181a",
            color: copiedLogs ? "white" : "#eee",
            border: "1px solid #444",
            padding: "6px 12px",
            borderRadius: "6px",
            fontSize: "0.85rem",
            fontWeight: "bold",
            cursor: "pointer",
            transition: "all 0.25s ease"
          }}
        >
          {copiedLogs ? (
            <>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <polyline points="20 6 9 17 4 12"></polyline>
              </svg>
              Copied!
            </>
          ) : (
            <>
              <Icons.Copy />
              Copy Logs
            </>
          )}
        </button>
        </div>
      </div>
      <div style={{ background: "#0e1113", padding: "1rem", borderRadius: "8px", maxHeight: "400px", overflowY: "auto" }}>
        {logs.length === 0 ? <p style={{ color: "#888" }}>No logs yet...</p> : null}
        {logs.map((log, index) => (
          <div key={index} style={{ fontFamily: "monospace", fontSize: "0.9rem", color: "#4caf50", marginBottom: "4px" }}>{log}</div>
        ))}
      </div>
    </div>
    </div>
  );
}
