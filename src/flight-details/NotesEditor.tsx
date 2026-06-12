export const NOTES_MAX_LEN = 500;

export function NotesEditor({ notes, onChange, onSave, saving, dirty, status }: {
    notes: string,
    onChange: (value: string) => void,
    onSave: () => void,
    saving: boolean,
    dirty: boolean,
    status: string | null,
}) {
    return (
        <div className="panel-bordered" style={{ marginBottom: "2rem" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: "0.75rem" }}>
                <h3 style={{ margin: 0, color: "#888", fontSize: "1.1rem" }}>Notes</h3>
                <span style={{ fontSize: "0.8rem", color: notes.length >= NOTES_MAX_LEN ? "#f38ba8" : "#888" }}>
                    {notes.length}/{NOTES_MAX_LEN}
                </span>
            </div>
            <textarea
                value={notes}
                maxLength={NOTES_MAX_LEN}
                onChange={(e) => onChange(e.target.value)}
                placeholder="Add notes about this flight…"
                rows={4}
                style={{
                    width: "100%", boxSizing: "border-box", resize: "vertical",
                    background: "#1e1e2e", color: "#cdd6f4", border: "1px solid #45475a",
                    borderRadius: "8px", padding: "0.75rem", fontSize: "0.95rem", fontFamily: "inherit"
                }}
            />
            <div style={{ display: "flex", alignItems: "center", gap: "1rem", marginTop: "0.75rem" }}>
                <button
                    onClick={onSave}
                    disabled={saving || !dirty}
                    style={{ backgroundColor: dirty ? "#ffb000" : "#45475a", color: "#11111b", opacity: saving ? 0.7 : 1 }}
                >
                    {saving ? "Saving..." : "Save Notes"}
                </button>
                {status && (
                    <span style={{ fontSize: "0.85rem", color: status.startsWith("Error") ? "#f38ba8" : "#a6e3a1" }}>
                        {status}
                    </span>
                )}
            </div>
        </div>
    );
}
