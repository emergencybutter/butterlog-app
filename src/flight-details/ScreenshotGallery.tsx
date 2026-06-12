import { convertFileSrc } from "@tauri-apps/api/core";
import { Screenshot } from "../models";

export function ScreenshotGallery({ screenshots, canUpload, uploadingIds, onUpload }: {
    screenshots: Screenshot[],
    canUpload: boolean,
    uploadingIds: Set<number>,
    onUpload: (screenshotId: number) => void,
}) {
    if (screenshots.length === 0) return null;

    return (
        <div style={{ marginBottom: "2rem" }}>
            <h3 style={{ color: "#888", marginBottom: "1rem" }}>Screenshots</h3>
            <div style={{ display: "flex", gap: "15px", overflowX: "auto", paddingBottom: "10px" }}>
                {screenshots.map((s, i) => (
                    <div key={i} style={{ flex: "0 0 auto", width: "280px", background: "#0e1113", borderRadius: "4px", overflow: "hidden", border: "1px solid #333", position: "relative" }}>
                        <img src={convertFileSrc(s.path)} alt="Flight Screenshot" style={{ width: "100%", height: "170px", objectFit: "cover" }} />

                        {canUpload && (
                            <div
                                onClick={s.remoteHash || uploadingIds.has(s.id) ? undefined : () => onUpload(s.id)}
                                style={{
                                    position: "absolute",
                                    bottom: "35px",
                                    right: "5px",
                                    background: s.remoteHash ? "rgba(76, 175, 80, 0.9)" : "rgba(0,0,0,0.6)",
                                    border: s.remoteHash ? "none" : "1px solid #555",
                                    color: "white",
                                    padding: "4px",
                                    borderRadius: "4px",
                                    cursor: s.remoteHash ? "default" : "pointer",
                                    display: "flex",
                                    alignItems: "center",
                                    justifyContent: "center",
                                    boxShadow: s.remoteHash ? "0 2px 4px rgba(0,0,0,0.3)" : "none"
                                }}
                                title={s.remoteHash ? "Uploaded to Butterlog" : "Upload to Butterlog"}
                            >
                                {uploadingIds.has(s.id) ? (
                                    <div className="spinner" style={{ width: "14px", height: "14px", border: "2px solid #fff", borderTop: "2px solid transparent", borderRadius: "50%", animation: "spin 1s linear infinite" }}></div>
                                ) : s.remoteHash ? (
                                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12" /></svg>
                                ) : (
                                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v4a2 2 0 0 1 2-2h4" /><polyline points="17 8 12 3 7 8" /><line x1="12" y1="3" x2="12" y2="15" /></svg>
                                )}
                            </div>
                        )}

                        <div style={{ padding: "5px", fontSize: "0.7rem", color: "#888", textAlign: "center" }}>
                            {s.timestamp.includes(' ') ? s.timestamp.split(' ')[1] : s.timestamp}
                        </div>
                    </div>
                ))}
            </div>
        </div>
    );
}
