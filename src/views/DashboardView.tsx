import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Sparkles, Trash2, Copy, ChevronRight, ScanSearch, X } from "lucide-react";
import {
  scan,
  sizePaths,
  scanDuplicates,
  cancelScan,
  formatBytes,
  errMsg,
  type ScanProgress,
} from "../lib/api";
import { useToast } from "../lib/toast";

type Finding = {
  junk: number; // caches, logs, dev caches…
  trash: number;
  dupes: number;
};

const MB = 1024 * 1024;

export default function DashboardView({
  onNavigate,
}: {
  onNavigate: (tab: "clean" | "duplicates") => void;
}) {
  const [scanning, setScanning] = useState(false);
  const [found, setFound] = useState<Finding | null>(null);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const cancelled = useRef(false);
  const toast = useToast();

  useEffect(() => {
    const un = listen<ScanProgress>("scan-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  function cancel() {
    cancelled.current = true;
    cancelScan();
    setProgress(null);
  }

  async function smartScan() {
    cancelled.current = false;
    setProgress(null);
    setScanning(true);
    setFound(null);
    try {
      const [cleanTotals, dupes] = await Promise.all([
        scanAndSize(),
        scanDuplicates([], MB).catch(() => ({ total_wasted: 0 })),
      ]);
      if (cancelled.current) {
        toast.push("Smart Scan cancelled.", "info");
        return;
      }
      setFound({ ...cleanTotals, dupes: dupes.total_wasted });
    } catch (e) {
      toast.err(`Smart Scan failed: ${errMsg(e)}`);
    } finally {
      setScanning(false);
      setProgress(null);
    }
  }

  async function scanAndSize(): Promise<{ junk: number; trash: number }> {
    const cats = await scan();
    const paths = cats.flatMap((c) => c.entries.map((e) => e.path));
    const sizes = await sizePaths(paths);
    const map = new Map(sizes.map((s) => [s.path, s.size]));
    let junk = 0;
    let trash = 0;
    for (const c of cats) {
      const total = c.entries.reduce((s, e) => s + (map.get(e.path) ?? 0), 0);
      if (c.id === "trash") trash += total;
      else junk += total;
    }
    return { junk, trash };
  }

  const total = found ? found.junk + found.trash + found.dupes : 0;

  const cards = found
    ? [
        { icon: Sparkles, label: "Caches & junk", size: found.junk, tab: "clean" as const, hint: "Caches, logs, developer junk" },
        { icon: Trash2, label: "Trash", size: found.trash, tab: "clean" as const, hint: "Empty the Trash & iCloud trash" },
        { icon: Copy, label: "Duplicate files", size: found.dupes, tab: "duplicates" as const, hint: "Identical files by content" },
      ]
    : [];

  return (
    <div className="view">
      <header className="view-head" data-tauri-drag-region>
        <div data-tauri-drag-region>
          <h1>Dashboard</h1>
          <p className="muted">One scan across caches, Trash and duplicates.</p>
        </div>
      </header>

      {!found && (
        <div className="dash-hero">
          <div className="dash-orb">
            <ScanSearch size={40} />
          </div>
          <h2>{scanning ? "Scanning your Mac…" : "Run a Smart Scan"}</h2>
          <p className="muted">
            {scanning
              ? progress
                ? progress.phase === "walk"
                  ? `Scanning files… ${progress.done.toLocaleString()}`
                  : `Hashing ${progress.done.toLocaleString()} / ${progress.total.toLocaleString()}`
                : "Sizing caches and hashing files…"
              : "Find reclaimable space across every module in one go."}
          </p>
          {scanning ? (
            <button className="btn ghost lg" onClick={cancel}>
              <X size={16} /> Cancel
            </button>
          ) : (
            <button className="btn primary lg" onClick={smartScan}>
              <Sparkles size={16} /> Smart Scan
            </button>
          )}
        </div>
      )}

      {found && (
        <div className="dash-result">
          <div className="dash-total">
            <span className="dash-total-num">{formatBytes(total)}</span>
            <span className="muted">reclaimable</span>
            <button className="btn ghost" onClick={smartScan} disabled={scanning}>
              Rescan
            </button>
          </div>

          <div className="dash-cards">
            {cards.map((c) => (
              <button key={c.label} className="dash-card" onClick={() => onNavigate(c.tab)}>
                <div className="dash-card-ico">
                  <c.icon size={20} />
                </div>
                <div className="dash-card-body">
                  <span className="dash-card-size">{formatBytes(c.size)}</span>
                  <span className="dash-card-label">{c.label}</span>
                  <span className="muted small">{c.hint}</span>
                </div>
                <ChevronRight size={18} className="dash-card-arrow" />
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
