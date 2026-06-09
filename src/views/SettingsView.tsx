import { useEffect, useState } from "react";
import {
  getTraySettings,
  setTraySettings,
  trayHasBattery,
  errMsg,
  type Metric,
  type TraySettings,
} from "../lib/api";
import { useToast } from "../lib/toast";

const METRICS: { id: Metric; label: string }[] = [
  { id: "cpu", label: "CPU" },
  { id: "memory", label: "Memory" },
  { id: "disk", label: "Disk" },
  { id: "battery", label: "Battery" },
];

export default function SettingsView() {
  const [settings, setSettings] = useState<TraySettings | null>(null);
  const [hasBattery, setHasBattery] = useState(true);
  const toast = useToast();

  useEffect(() => {
    getTraySettings()
      .then(setSettings)
      .catch((e) => toast.err(`Failed to load settings: ${errMsg(e)}`));
    trayHasBattery()
      .then(setHasBattery)
      .catch(() => setHasBattery(true));
  }, [toast]);

  // Desktop Macs have no battery → don't offer it as a tray metric.
  const metrics = METRICS.filter((m) => m.id !== "battery" || hasBattery);

  function update(next: TraySettings) {
    setSettings(next);
    setTraySettings(next).catch((e) => toast.err(`Failed to save: ${errMsg(e)}`));
  }

  function toggle(group: "title" | "menu", m: Metric) {
    if (!settings) return;
    const enabled = new Set(settings[group]);
    enabled.has(m) ? enabled.delete(m) : enabled.add(m);
    // Keep a stable, fixed order.
    const ordered = metrics.map((x) => x.id).filter((id) => enabled.has(id));
    update({ ...settings, [group]: ordered });
  }

  function group(
    key: "title" | "menu",
    title: string,
    description: string,
  ) {
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

  return (
    <div className="view">
      <header className="view-head" data-tauri-drag-region>
        <div data-tauri-drag-region>
          <h1>Settings</h1>
          <p className="muted">Choose what the menu-bar tray shows</p>
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
        </div>
      )}
    </div>
  );
}
