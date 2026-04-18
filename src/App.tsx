import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

function App() {
  const [logs, setLogs] = useState<string[]>([]);

  useEffect(() => {
    // Fetch existing logs on mount
    invoke<string[]>("get_logs").then(setLogs).catch(console.error);

    // Listen for new log events from the backend
    const unlisten = listen<string>("log-update", (event) => {
      setLogs((prevLogs) => [...prevLogs, event.payload]);
    });

    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  return (
    <main className="container">
      <div className="logs-container" style={{ marginTop: "2rem", textAlign: "left" }}>
        <h3>Backend Logs</h3>
        <div style={{ background: "#1a1a1a", padding: "1rem", borderRadius: "8px", maxHeight: "200px", overflowY: "auto" }}>
          {logs.length === 0 ? <p style={{ color: "#888" }}>No logs yet...</p> : null}
          {logs.map((log, index) => (
            <div key={index} style={{ fontFamily: "monospace", fontSize: "0.9rem", color: "#4caf50" }}>{log}</div>
          ))}
        </div>
      </div>
    </main>
  );
}

export default App;
