import { useState } from "react";
import {
  Activity,
  Info,
  PackageMinus,
  Settings,
  Sparkles,
  Zap,
  type LucideIcon,
} from "lucide-react";
import "./styles.css";
import AboutModal from "./views/AboutModal";
import CleanView from "./views/CleanView";
import OptimizeView from "./views/OptimizeView";
import SettingsView from "./views/SettingsView";
import StatusView from "./views/StatusView";
import UninstallView from "./views/UninstallView";

type Tab = "clean" | "uninstall" | "optimize" | "status" | "settings";

const TABS: { id: Tab; label: string; icon: LucideIcon }[] = [
  { id: "clean", label: "Clean", icon: Sparkles },
  { id: "uninstall", label: "Uninstall", icon: PackageMinus },
  { id: "optimize", label: "Optimize", icon: Zap },
  { id: "status", label: "Status", icon: Activity },
  { id: "settings", label: "Settings", icon: Settings },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("clean");
  const [aboutOpen, setAboutOpen] = useState(false);

  return (
    <div className="app">
      <aside className="sidebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <span className="brand-name">Trashly</span>
        </div>
        <nav>
          {TABS.map((t) => (
            <button
              key={t.id}
              className={`nav-item ${tab === t.id ? "active" : ""}`}
              onClick={() => setTab(t.id)}
            >
              <t.icon className="nav-icon" size={17} />
              {t.label}
            </button>
          ))}
        </nav>
        <button className="nav-item about-btn" onClick={() => setAboutOpen(true)}>
          <Info className="nav-icon" size={17} />
          About
        </button>
      </aside>

      <main className="content">
        {tab === "clean" && <CleanView />}
        {tab === "uninstall" && <UninstallView />}
        {tab === "optimize" && <OptimizeView />}
        {tab === "status" && <StatusView />}
        {tab === "settings" && <SettingsView />}
      </main>

      <AboutModal open={aboutOpen} onClose={() => setAboutOpen(false)} />
    </div>
  );
}
