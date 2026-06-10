import { useEffect, useMemo, useRef, useState } from "react";
import {
  scan,
  scanProjects,
  sizePaths,
  clean,
  formatBytes,
  errMsg,
  openFullDiskAccess,
  type CategoryResult,
} from "../lib/api";
import { RotateCw } from "lucide-react";
import { toggleInSet } from "../lib/util";
import { useToast } from "../lib/toast";
import ConfirmDeleteModal from "./ConfirmDeleteModal";
import FdaBanner from "./FdaBanner";

export default function CleanView() {
  const [cats, setCats] = useState<CategoryResult[]>([]);
  // Start in the loading state so the spinner shows on the very first frame
  // (we always scan on mount) instead of one frame later.
  const [loading, setLoading] = useState(true);
  const [sizing, setSizing] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [projectsLoading, setProjectsLoading] = useState(false);
  const toast = useToast();
  const reqId = useRef(0);

  async function runScan(autoSelect = true) {
    const token = ++reqId.current;
    setLoading(true);
    setCats([]);
    try {
      const shells = await scan();
      if (token !== reqId.current) return;
      setCats(shells);
      // On a fresh scan pre-select everything; after a clean we leave it all
      // unchecked so the user doesn't have to re-clear what they just removed.
      if (autoSelect) {
        const all = new Set<string>();
        shells.forEach((c) => c.entries.forEach((e) => all.add(e.id)));
        setSelected(all);
      } else {
        setSelected(new Set());
      }
      setSizing(new Set(shells.map((c) => c.id)));
      setLoading(false);

      // Stream sizes per category so a 3 GB cache doesn't block the view.
      shells.forEach(async (c) => {
        try {
          const sizes = await sizePaths(c.entries.map((e) => e.path));
          if (token !== reqId.current) return;
          const map = new Map(sizes.map((s) => [s.path, s.size]));
          setCats((prev) =>
            prev.map((pc) => {
              if (pc.id !== c.id) return pc;
              const entries = pc.entries
                .map((e) => ({ ...e, size: map.get(e.path) ?? 0 }))
                .filter((e) => e.size > 0)
                .sort((a, b) => b.size - a.size);
              return {
                ...pc,
                entries,
                total_size: entries.reduce((s, e) => s + e.size, 0),
              };
            }),
          );
          setSelected((prev) => {
            const next = new Set(prev);
            sizes.filter((s) => s.size === 0).forEach((s) => next.delete(s.path));
            return next;
          });
        } finally {
          if (token === reqId.current)
            setSizing((prev) => {
              const n = new Set(prev);
              n.delete(c.id);
              return n;
            });
        }
      });

      // Project build artifacts need a slower recursive walk — load in parallel
      // and append when ready (left unchecked; it's a "caution" category).
      setProjectsLoading(true);
      scanProjects()
        .then((cat) => {
          if (token !== reqId.current || cat.entries.length === 0) return;
          setCats((prev) => [...prev.filter((c) => c.id !== cat.id), cat]);
        })
        .catch((e) => {
          if (token === reqId.current)
            toast.err(`Project scan failed: ${errMsg(e)}`);
        })
        .finally(() => {
          if (token === reqId.current) setProjectsLoading(false);
        });
    } catch (e) {
      if (token === reqId.current) {
        toast.err(`Scan failed: ${errMsg(e)}`);
        setLoading(false);
      }
    }
  }

  useEffect(() => {
    runScan();
  }, []);

  const selectedSize = useMemo(() => {
    let total = 0;
    for (const c of cats)
      for (const e of c.entries) if (selected.has(e.id)) total += e.size;
    return total;
  }, [cats, selected]);

  const selectedCount = useMemo(() => {
    let n = 0;
    for (const c of cats) for (const e of c.entries) if (selected.has(e.id)) n++;
    return n;
  }, [cats, selected]);

  function toggleEntry(id: string) {
    setSelected((prev) => toggleInSet(prev, id));
  }

  function toggleCategory(c: CategoryResult) {
    const ids = c.entries.map((e) => e.id);
    const allOn = ids.every((id) => selected.has(id));
    setSelected((prev) => {
      const next = new Set(prev);
      ids.forEach((id) => (allOn ? next.delete(id) : next.add(id)));
      return next;
    });
  }

  function toggleExpand(id: string) {
    setExpanded((prev) => toggleInSet(prev, id));
  }

  async function runClean(toTrash: boolean) {
    if (selectedCount === 0) return;
    const paths = [...selected];
    setBusy(true);
    try {
      const res = await clean(paths, toTrash);
      const verb = toTrash ? "Moved to Trash" : "Deleted";
      let msg = `${verb} ${res.removed} items · freed ${formatBytes(res.freed)}`;
      if (res.failed.length) msg += ` · ${res.failed.length} failed`;
      toast.push(msg, res.failed.length ? "err" : "ok");
      if (res.needs_full_disk_access) {
        toast.err(
          "Some items need Full Disk Access — opening Settings; enable Trashly, then rescan.",
        );
        openFullDiskAccess();
      }
      setConfirm(false);
      await runScan(false); // don't re-check everything after a clean
    } catch (e) {
      toast.err(`Clean failed: ${errMsg(e)}`);
    } finally {
      setBusy(false);
    }
  }

  const grandTotal = cats.reduce((s, c) => s + c.total_size, 0);
  const stillSizing = sizing.size > 0;
  const totalCats = cats.length;

  // If the selection is entirely items that can only be deleted directly (the
  // Trash category), "Move to Trash" makes no sense — offer only permanent.
  const directOnly =
    selectedCount > 0 &&
    !cats.some(
      (c) => !c.always_direct && c.entries.some((e) => selected.has(e.id)),
    );

  return (
    <div className="view">
      <header className="view-head" data-tauri-drag-region>
        <div data-tauri-drag-region>
          <h1>
            Clean
            {(loading || stillSizing || projectsLoading) && (
              <span className="spinner title-spin" />
            )}
          </h1>
          <p className="muted">
            {loading
              ? "Scanning…"
              : stillSizing
                ? `${formatBytes(grandTotal)}+ · sizing ${totalCats - sizing.size}/${totalCats}…`
                : `${formatBytes(grandTotal)} reclaimable across ${totalCats} categories`}
          </p>
        </div>
        <button
          className="btn ghost"
          onClick={() => runScan(true)}
          disabled={loading || busy}
        >
          {loading ? (
            "Scanning…"
          ) : (
            <>
              <RotateCw size={14} /> Rescan
            </>
          )}
        </button>
      </header>

      <FdaBanner note="Grant Full Disk Access for complete results — the Trash and other apps data may be hidden." />

      <div className="cat-list">
        {cats.map((c) => {
          const ids = c.entries.map((e) => e.id);
          const allOn = ids.length > 0 && ids.every((id) => selected.has(id));
          const someOn = ids.some((id) => selected.has(id));
          const isOpen = expanded.has(c.id);
          const catSizing = sizing.has(c.id);
          return (
            <div className="cat" key={c.id}>
              <div className="cat-head">
                <input
                  type="checkbox"
                  checked={allOn}
                  ref={(el) => {
                    if (el) el.indeterminate = !allOn && someOn;
                  }}
                  onChange={() => toggleCategory(c)}
                  disabled={ids.length === 0}
                />
                <button className="cat-title" onClick={() => toggleExpand(c.id)}>
                  <span className={`chev ${isOpen ? "open" : ""}`}>▸</span>
                  <span className="cat-label">{c.label}</span>
                  <span className="count">
                    {catSizing ? "…" : c.entries.length}
                  </span>
                  {c.risk === "caution" && (
                    <span className="badge caution">caution</span>
                  )}
                </button>
                <span className="cat-size">
                  {catSizing ? (
                    <span className="spinner small" />
                  ) : (
                    formatBytes(c.total_size)
                  )}
                </span>
              </div>
              <p className="cat-desc">{c.description}</p>
              {isOpen && !catSizing && (
                <ul className="entries">
                  {c.entries.map((e) => (
                    <li key={e.id}>
                      <input
                        type="checkbox"
                        checked={selected.has(e.id)}
                        onChange={() => toggleEntry(e.id)}
                      />
                      <span className="entry-name" title={e.path}>
                        {e.name}
                      </span>
                      <span className="entry-size">{formatBytes(e.size)}</span>
                    </li>
                  ))}
                  {c.entries.length === 0 && (
                    <li className="muted">Nothing to clean here.</li>
                  )}
                </ul>
              )}
            </div>
          );
        })}
      </div>

      <footer className="action-bar">
        <span>
          <strong>{selectedCount}</strong> selected ·{" "}
          <strong>{formatBytes(selectedSize)}</strong>
        </span>
        <button
          className="btn primary"
          onClick={() => setConfirm(true)}
          disabled={busy || selectedCount === 0}
        >
          Clean {selectedCount > 0 && `(${formatBytes(selectedSize)})`}
        </button>
      </footer>

      <ConfirmDeleteModal
        open={confirm}
        busy={busy}
        directOnly={directOnly}
        title={`Clean ${selectedCount} item${selectedCount === 1 ? "" : "s"}?`}
        message={
          directOnly
            ? `${formatBytes(selectedSize)} in the Trash will be permanently deleted.`
            : `${formatBytes(selectedSize)} will be removed. How do you want to remove it?`
        }
        onTrash={() => runClean(true)}
        onDirect={() => runClean(false)}
        onCancel={() => setConfirm(false)}
      />
    </div>
  );
}
