import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderPlus, X } from "lucide-react";
import {
  getTraySettings,
  setTraySettings,
  trayHasBattery,
  getProtectedPaths,
  setProtectedPaths,
  getCleanupLog,
  clearCleanupLog,
  formatBytes,
  errMsg,
  type Metric,
  type TraySettings,
  type CleanupEntry,
} from "../lib/api";
import { useToast } from "../lib/toast";

const METRICS: { id: Metric; label: string }[] = [
  { id: "cpu", label: "CPU" },
  { id: "memory", label: "Memory" },
  { id: "disk", label: "Disk" },
  { id: "battery", label: "Battery" },
];

const tilde = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");

export default function SettingsView() {
  const [settings, setSettings] = useState<TraySettings | null>(null);
  const [hasBattery, setHasBattery] = useState(true);
  const [protect, setProtect] = useState<string[]>([]);
  const [log, setLog] = useState<CleanupEntry[]>([]);
  const toast = useToast();

  useEffect(() => {
    getTraySettings()
      .then(setSettings)
      .catch((e) => toast.err(`Failed to load settings: ${errMsg(e)}`));
    trayHasBattery().then(setHasBattery).catch(() => setHasBattery(true));
    getProtectedPaths().then(setProtect).catch(() => {});
    getCleanupLog(50).then(setLog).catch(() => {});
  }, [toast]);

  const metrics = METRICS.filter((m) => m.id !== "battery" || hasBattery);

  function update(next: TraySettings) {
    setSettings(next);
    setTraySettings(next).catch((e) => toast.err(`Failed to save: ${errMsg(e)}`));
  }

  function toggle(group: "title" | "menu", m: Metric) {
    if (!settings) return;
    const enabled = new Set(settings[group]);
    enabled.has(m) ? enabled.delete(m) : enabled.add(m);
    const ordered = metrics.map((x) => x.id).filter((id) => enabled.has(id));
    update({ ...settings, [group]: ordered });
  }

  function group(key: "title" | "menu", title: string, description: string) {
    return (
      <div className="settings-group">
        <div className="settings-group-head">
          <span className="settings-group-title">{title}</span>
          <span className="muted small">{description}</span>
        </div>
        <div className="settings-metrics">
          {metrics.map((m) => (
            <label className="settings-metric" key={m.id}>
              <input
                type="checkbox"
                checked={settings?.[key].includes(m.id) ?? false}
                onChange={() => toggle(key, m.id)}
              />
              {m.label}
            </label>
          ))}
        </div>
      </div>
    );
  }

  function saveProtect(next: string[]) {
    setProtect(next);
    setProtectedPaths(next).catch((e) => toast.err(`Failed to save: ${errMsg(e)}`));
  }

  async function addFolder() {
    try {
      const sel = await openDialog({ directory: true, multiple: false, title: "Protect a folder" });
      if (typeof sel === "string" && !protect.includes(sel)) {
        saveProtect([...protect, sel]);
      }
    } catch (e) {
      toast.err(`Couldn't open picker: ${errMsg(e)}`);
    }
  }

  async function clearLog() {
    await clearCleanupLog().catch(() => {});
    setLog([]);
  }

  return (
    <div className="view">
      <header className="view-head" data-tauri-drag-region>
        <div data-tauri-drag-region>
          <h1>Settings</h1>
          <p className="muted">Menu bar, protected folders & cleanup history</p>
        </div>
      </header>

      {!settings ? (
        <div className="empty">
          <span className="spinner" /> Loading…
        </div>
      ) : (
        <div className="settings-list">
          {group("title", "Menu bar", "Stats shown next to the tray icon")}
          {group("menu", "Tray dropdown", "Rows shown when you click the icon")}

          <div className="settings-group">
            <div className="settings-group-head">
              <span className="settings-group-title">Protected folders</span>
              <span className="muted small">
                Trashly will never remove anything inside these — on top of all built-in guards.
              </span>
            </div>
            <div className="protect-list">
              {protect.map((p) => (
                <span className="protect-item" key={p}>
                  <span className="mono small" title={p}>{tilde(p)}</span>
                  <button className="protect-x" onClick={() => saveProtect(protect.filter((x) => x !== p))}>
                    <X size={13} />
                  </button>
                </span>
              ))}
              {protect.length === 0 && <span className="muted small">No protected folders yet.</span>}
            </div>
            <button className="btn ghost" onClick={addFolder}>
              <FolderPlus size={14} /> Add folder…
            </button>
          </div>

          <div className="settings-group">
            <div className="settings-group-head row">
              <span className="settings-group-title">Recent cleanups</span>
              {log.length > 0 && (
                <button className="btn ghost sm" onClick={clearLog}>Clear</button>
              )}
            </div>
            {log.length === 0 ? (
              <span className="muted small">No cleanups logged yet.</span>
            ) : (
              <ul className="log-list">
                {log.map((e, i) => (
                  <li key={i}>
                    <span className={`badge ${e.to_trash ? "" : "caution"}`}>
                      {e.to_trash ? "trash" : "perm"}
                    </span>
                    <span className="mono small log-path" title={e.path}>{tilde(e.path)}</span>
                    <span className="log-size">{formatBytes(e.size)}</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
