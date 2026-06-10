import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Copy, Eye, FolderOpen, Images, RotateCw, X } from "lucide-react";
import {
  dupeRoots,
  scanDuplicates,
  scanSimilarPhotos,
  removeFiles,
  revealInFinder,
  quickLook,
  cancelScan,
  formatBytes,
  errMsg,
  openFullDiskAccess,
  type RootInfo,
  type ScanProgress,
} from "../lib/api";
import { toggleInSet } from "../lib/util";
import { useToast } from "../lib/toast";
import ConfirmDeleteModal from "./ConfirmDeleteModal";

type Mode = "files" | "photos";

type VMFile = {
  path: string;
  name: string;
  size: number;
  modified: number;
  thumb?: string;
};
type VMGroup = { key: string; sublabel: string; wasted: number; files: VMFile[] };

const MIN_SIZES = [
  { label: "≥ 10 KB", value: 10 * 1024 },
  { label: "≥ 100 KB", value: 100 * 1024 },
  { label: "≥ 1 MB", value: 1024 * 1024 },
  { label: "≥ 10 MB", value: 10 * 1024 * 1024 },
  { label: "≥ 100 MB", value: 100 * 1024 * 1024 },
];
// Photos vary a lot in size (screenshots can be tiny), so default low; exact
// files default higher to avoid a flood of small matches.
const PHOTOS_MIN = 10 * 1024;
const FILES_MIN = 1024 * 1024;
// Fixed "Balanced" similarity threshold (Hamming distance) for photo matching.
const PHOTO_THRESHOLD = 10;

const tilde = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");

export default function DuplicatesView() {
  const [mode, setMode] = useState<Mode>("files");
  const [roots, setRoots] = useState<RootInfo[]>([]);
  const [pickedRoots, setPickedRoots] = useState<Set<string>>(new Set());
  const [minSize, setMinSize] = useState(FILES_MIN);
  const [groups, setGroups] = useState<VMGroup[] | null>(null);
  const [scannedNote, setScannedNote] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const cancelled = useRef(false);
  const toast = useToast();

  // Live progress from the Rust scan.
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

  useEffect(() => {
    dupeRoots()
      .then((r) => {
        setRoots(r);
        setPickedRoots(new Set(r.map((x) => x.key)));
      })
      .catch((e) => toast.err(`Failed to read folders: ${errMsg(e)}`));
  }, [toast]);

  function switchMode(m: Mode) {
    if (m === mode) return;
    setMode(m);
    setGroups(null);
    setSelected(new Set());
    setScannedNote("");
    setMinSize(m === "photos" ? PHOTOS_MIN : FILES_MIN);
  }

  async function runScan() {
    cancelled.current = false;
    setProgress(null);
    setLoading(true);
    setGroups(null);
    setSelected(new Set());
    try {
      const paths = roots.filter((r) => pickedRoots.has(r.key)).map((r) => r.path);
      let vm: VMGroup[];
      const sel = new Set<string>();
      let unreadable: string[] = [];
      let skipped = 0;
      if (mode === "files") {
        const r = await scanDuplicates(paths, minSize);
        unreadable = r.unreadable;
        skipped = r.skipped_icloud;
        vm = r.groups.map((g) => ({
          key: g.hash,
          sublabel: `${g.count} copies · ${formatBytes(g.size)} each`,
          wasted: g.wasted,
          files: g.files.map((f) => ({ ...f, size: g.size })),
        }));
        // keep the oldest copy, mark the rest
        vm.forEach((g) =>
          [...g.files]
            .sort((a, b) => a.modified - b.modified)
            .slice(1)
            .forEach((f) => sel.add(f.path)),
        );
        setScannedNote(`${formatBytes(r.total_wasted)} · ${vm.length} sets · ${r.scanned} files`);
      } else {
        const r = await scanSimilarPhotos(paths, minSize, PHOTO_THRESHOLD);
        unreadable = r.unreadable;
        skipped = r.skipped_icloud;
        vm = r.groups.map((g, i) => ({
          key: `p${i}`,
          sublabel: `${g.count} similar · save ${formatBytes(g.wasted)}`,
          wasted: g.wasted,
          files: g.files,
        }));
        // backend sorts largest-first; keep the best, mark the rest
        vm.forEach((g) => g.files.slice(1).forEach((f) => sel.add(f.path)));
        setScannedNote(
          `${formatBytes(r.total_wasted)} · ${vm.length} sets · ${r.scanned} photos` +
            (r.truncated ? " (first 6000)" : ""),
        );
      }
      if (cancelled.current) {
        toast.push("Scan cancelled.", "info");
        return;
      }
      setGroups(vm);
      setSelected(sel);
      if (unreadable.length) {
        toast.err(
          `Can't read ${unreadable.join(", ")} — grant Trashly access in Privacy & Security, then rescan.`,
        );
        openFullDiskAccess();
      } else if (skipped > 0) {
        toast.push(
          `${skipped} item${skipped === 1 ? "" : "s"} skipped (offloaded to iCloud) — download to include.`,
          "info",
        );
      }
    } catch (e) {
      toast.err(`Scan failed: ${errMsg(e)}`);
    } finally {
      setLoading(false);
      setProgress(null);
    }
  }

  // Toggle a file for removal, but never let the user select the *last* copy in
  // a set — that would delete the file outright, not de-duplicate it.
  function toggleFile(g: VMGroup, path: string) {
    if (!selected.has(path)) {
      const unselected = g.files.filter((f) => !selected.has(f.path)).length;
      if (unselected <= 1) {
        toast.push("Keep at least one copy in each set.", "info");
        return;
      }
    }
    setSelected((s) => toggleInSet(s, path));
  }

  const sizeByPath = useMemo(() => {
    const m = new Map<string, number>();
    groups?.forEach((g) => g.files.forEach((f) => m.set(f.path, f.size)));
    return m;
  }, [groups]);

  const selectedSize = useMemo(() => {
    let t = 0;
    for (const p of selected) t += sizeByPath.get(p) ?? 0;
    return t;
  }, [selected, sizeByPath]);

  async function runRemove(toTrash: boolean) {
    if (selected.size === 0) return;
    setBusy(true);
    try {
      const r = await removeFiles([...selected], toTrash);
      toast.push(
        `Removed ${r.removed} files · freed ${formatBytes(r.freed)}` +
          (r.failed.length ? ` · ${r.failed.length} failed` : ""),
        r.failed.length ? "err" : "ok",
      );
      if (r.needs_full_disk_access) {
        toast.err("Some files need Full Disk Access — opening Settings; enable Trashly, then retry.");
        openFullDiskAccess();
      }
      setConfirm(false);
      await runScan();
    } catch (e) {
      toast.err(`Remove failed: ${errMsg(e)}`);
    } finally {
      setBusy(false);
    }
  }

  const list = groups ?? [];
  const progLabel = progress
    ? progress.phase === "walk"
      ? `Scanning files… ${progress.done.toLocaleString()}`
      : `Hashing ${progress.done.toLocaleString()} / ${progress.total.toLocaleString()}`
    : mode === "photos"
      ? "Decoding & comparing photos…"
      : "Hashing files…";

  return (
    <div className="view">
      <header className="view-head" data-tauri-drag-region>
        <div data-tauri-drag-region>
          <h1>
            Duplicates
            {loading && <span className="spinner title-spin" />}
          </h1>
          <p className="muted">
            {loading
              ? progLabel
              : groups
                ? scannedNote
                : mode === "photos"
                  ? "Find look-alike photos & screenshots (perceptual hash)."
                  : "Find identical files by content (not by name)."}
          </p>
        </div>
        {loading ? (
          <button className="btn ghost" onClick={cancel}>
            <X size={14} /> Cancel
          </button>
        ) : (
          <button className="btn ghost" onClick={runScan} disabled={busy}>
            <RotateCw size={14} /> Scan
          </button>
        )}
      </header>

      <div className="dupe-controls">
        <div className="seg">
          <button
            className={`seg-btn ${mode === "files" ? "on" : ""}`}
            onClick={() => switchMode("files")}
          >
            <Copy size={14} /> Files
          </button>
          <button
            className={`seg-btn ${mode === "photos" ? "on" : ""}`}
            onClick={() => switchMode("photos")}
          >
            <Images size={14} /> Photos
          </button>
        </div>
        {mode === "files" && (
          <select className="select" value={minSize} onChange={(e) => setMinSize(Number(e.target.value))}>
            {MIN_SIZES.map((m) => (
              <option key={m.value} value={m.value}>{m.label}</option>
            ))}
          </select>
        )}
        <div className="root-chips">
          {roots.map((r) => (
            <label key={r.key} className={`chip ${pickedRoots.has(r.key) ? "on" : ""}`}>
              <input
                type="checkbox"
                checked={pickedRoots.has(r.key)}
                onChange={() => setPickedRoots((p) => toggleInSet(p, r.key))}
              />
              {r.label}
            </label>
          ))}
        </div>
      </div>

      <div className="cat-list">
        {!groups && !loading && (
          <div className="empty">
            {mode === "photos" ? <Images size={26} className="empty-ico" /> : <Copy size={26} className="empty-ico" />}
            Pick folders and hit <b>Scan</b>.
          </div>
        )}
        {groups && list.length === 0 && !loading && (
          <div className="empty">Nothing found 🎉</div>
        )}
        {list.map((g) => (
          <div className="cat dupe-group" key={g.key}>
            <div className="cat-head">
              <span className="cat-label">{g.sublabel}</span>
              <span className="badge caution">save {formatBytes(g.wasted)}</span>
            </div>
            {mode === "photos" ? (
              <div className="photo-grid">
                {g.files.map((f, i) => {
                  const sel = selected.has(f.path);
                  return (
                    <div
                      key={f.path}
                      className={`photo ${sel ? "sel" : ""}`}
                      role="button"
                      onClick={() => toggleFile(g, f.path)}
                      title={f.path}
                    >
                      <img src={f.thumb} alt={f.name} />
                      <span className="photo-meta">{formatBytes(f.size)}</span>
                      {i === 0 && !sel && <span className="photo-keep">keep</span>}
                      {sel && <span className="photo-check">✓</span>}
                      <div className="photo-actions">
                        <button
                          className="photo-act"
                          title="Reveal in Finder"
                          onClick={(e) => { e.stopPropagation(); revealInFinder(f.path); }}
                        >
                          <FolderOpen size={13} />
                        </button>
                        <button
                          className="photo-act"
                          title="Quick Look"
                          onClick={(e) => { e.stopPropagation(); quickLook(f.path); }}
                        >
                          <Eye size={13} />
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            ) : (
              <ul className="entries">
                {[...g.files]
                  .sort((a, b) => a.modified - b.modified)
                  .map((f, i) => (
                    <li key={f.path}>
                      <input
                        type="checkbox"
                        checked={selected.has(f.path)}
                        onChange={() => toggleFile(g, f.path)}
                      />
                      <span className="entry-name mono small" title={f.path}>{tilde(f.path)}</span>
                      <span className="row-actions">
                        <button className="row-act" title="Reveal in Finder" onClick={() => revealInFinder(f.path)}>
                          <FolderOpen size={13} />
                        </button>
                        <button className="row-act" title="Quick Look" onClick={() => quickLook(f.path)}>
                          <Eye size={13} />
                        </button>
                      </span>
                      <span className="entry-size">
                        {i === 0 && !selected.has(f.path) ? (
                          <span className="badge">keep · oldest</span>
                        ) : (
                          new Date(f.modified * 1000).toLocaleDateString()
                        )}
                      </span>
                    </li>
                  ))}
              </ul>
            )}
          </div>
        ))}
      </div>

      <footer className="action-bar">
        <span>
          <strong>{selected.size}</strong> to remove ·{" "}
          <strong>{formatBytes(selectedSize)}</strong>
        </span>
        <button
          className="btn primary danger"
          onClick={() => setConfirm(true)}
          disabled={busy || selected.size === 0}
        >
          Remove {selected.size > 0 && `(${formatBytes(selectedSize)})`}
        </button>
      </footer>

      <ConfirmDeleteModal
        open={confirm}
        busy={busy}
        title={`Remove ${selected.size} file${selected.size === 1 ? "" : "s"}?`}
        message={`${formatBytes(selectedSize)} will be removed. One copy of each is kept.`}
        onTrash={() => runRemove(true)}
        onDirect={() => runRemove(false)}
        onCancel={() => setConfirm(false)}
      />
    </div>
  );
}
