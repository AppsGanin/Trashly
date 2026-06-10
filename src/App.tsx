import { useEffect, useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import {
  Activity,
  Copy,
  Info,
  LayoutDashboard,
  PackageMinus,
  Settings,
  Sparkles,
  Zap,
  type LucideIcon,
} from "lucide-react";
import "./styles.css";
import { useToast } from "./lib/toast";
import AboutModal from "./views/AboutModal";
import CleanView from "./views/CleanView";
import DashboardView from "./views/DashboardView";
import DuplicatesView from "./views/DuplicatesView";
import OptimizeView from "./views/OptimizeView";
import SettingsView from "./views/SettingsView";
import StatusView from "./views/StatusView";
import UninstallView from "./views/UninstallView";

type Tab =
  | "dashboard"
  | "clean"
  | "uninstall"
  | "duplicates"
  | "optimize"
  | "status"
  | "settings";

const TABS: { id: Tab; label: string; icon: LucideIcon }[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "clean", label: "Clean", icon: Sparkles },
  { id: "uninstall", label: "Uninstall", icon: PackageMinus },
  { id: "duplicates", label: "Duplicates", icon: Copy },
  { id: "optimize", label: "Optimize", icon: Zap },
  { id: "status", label: "Status", icon: Activity },
  { id: "settings", label: "Settings", icon: Settings },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [aboutOpen, setAboutOpen] = useState(false);
  const toast = useToast();

  // Quietly check GitHub releases on launch; nudge to Settings if newer.
  useEffect(() => {
    check()
      .then((u) => {
        if (u) toast.push(`Update ${u.version} available — open About to install.`, "info");
      })
      .catch(() => {});
  }, [toast]);

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
        {tab === "dashboard" && <DashboardView onNavigate={setTab} />}
        {tab === "clean" && <CleanView />}
        {tab === "uninstall" && <UninstallView />}
        {tab === "duplicates" && <DuplicatesView />}
        {tab === "optimize" && <OptimizeView />}
        {tab === "status" && <StatusView />}
        {tab === "settings" && <SettingsView />}
      </main>

      <AboutModal open={aboutOpen} onClose={() => setAboutOpen(false)} />
    </div>
  );
}
