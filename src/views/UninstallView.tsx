import { useEffect, useMemo, useRef, useState } from "react";
import {
  listApps,
  appLeftovers,
  appIcon,
  sizePaths,
  uninstall,
  formatBytes,
  errMsg,
  openFullDiskAccess,
  isAppRunning,
  type AppInfo,
  type Leftover,
} from "../lib/api";
import { AlertTriangle, Package } from "lucide-react";
import { toggleInSet } from "../lib/util";
import { useToast } from "../lib/toast";
import ConfirmDeleteModal from "./ConfirmDeleteModal";

export default function UninstallView() {
  const [apps, setApps] = useState<AppInfo[]>([]);
  // Start loading so the spinner shows immediately on tab entry.
  const [loading, setLoading] = useState(true);
  const [sizing, setSizing] = useState(false);
  const [query, setQuery] = useState("");
  // Store only the selected path; the active app is derived from the live list
  // so its size updates as sizing streams in (instead of staying a stale 0).
  const [activePath, setActivePath] = useState<string | null>(null);
  const [icon, setIcon] = useState<string | null>(null);
  const [leftovers, setLeftovers] = useState<Leftover[]>([]);
  const [loadingLeft, setLoadingLeft] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  // Whether to remove the app bundle itself (vs. keeping it and only cleaning
  // leftovers). Forced off for non-removable system apps.
  const [removeBundle, setRemoveBundle] = useState(true);
  const [appRunning, setAppRunning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const toast = useToast();
  // Monotonic token to ignore stale async responses when switching apps fast.
  const reqId = useRef(0);

  async function loadApps() {
    setLoading(true);
    try {
      const list = await listApps();
      setApps(list); // shows instantly, sorted by name
      setLoading(false);
      setSizing(true);
      const sizes = await sizePaths(list.map((a) => a.path));
      const map = new Map(sizes.map((s) => [s.path, s.size]));
      setApps((prev) =>
        prev.map((a) => ({ ...a, size: map.get(a.path) ?? a.size })),
      );
    } catch (e) {
      toast.err(`Failed to load apps: ${errMsg(e)}`);
    } finally {
      setLoading(false);
      setSizing(false);
    }
  }
  useEffect(() => {
    loadApps();
  }, []);

  async function openApp(app: AppInfo) {
    const token = ++reqId.current;
    setActivePath(app.path);
    setRemoveBundle(true); // default to removing the bundle for a fresh app
    setIcon(null);
    setLeftovers([]);
    setAppRunning(false);
    setLoadingLeft(true);
    isAppRunning(app.path)
      .then((r) => {
        if (token === reqId.current) setAppRunning(r);
      })
      .catch(() => {});
    // Icon and leftovers load independently; both guarded by the token.
    appIcon(app.path)
      .then((dataUrl) => {
        if (token === reqId.current) setIcon(dataUrl);
      })
      .catch((e) => console.error("Icon load failed:", e));
    try {
      const left = await appLeftovers(app.bundle_id, app.name);
      if (token !== reqId.current) return; // a newer app was clicked
      setLeftovers(left);
      // Default-select only confident (bundle-id) matches; name matches are
      // left unchecked for the user to verify.
      setSelected(new Set(left.filter((l) => !l.from_name).map((l) => l.path)));
    } catch (e) {
      if (token === reqId.current) toast.err(`Scan failed: ${errMsg(e)}`);
    } finally {
      if (token === reqId.current) setLoadingLeft(false);
    }
  }

  const filtered = useMemo(
    () =>
      apps
        .filter((a) => a.name.toLowerCase().includes(query.toLowerCase()))
        .sort((a, b) => b.size - a.size), // largest first
    [apps, query],
  );

  // Derived from the live list so the bundle size reflects streamed sizing.
  const active = activePath
    ? (apps.find((a) => a.path === activePath) ?? null)
    : null;

  // System apps (e.g. Safari) can't have their bundle removed — only data.
  const removable = active?.removable ?? true;
  const willRemoveBundle = removable && removeBundle;
  const bundleSize = active && willRemoveBundle ? active.size : 0;
  const leftoverSize = leftovers
    .filter((l) => selected.has(l.path))
    .reduce((s, l) => s + l.size, 0);

  async function runUninstall(toTrash: boolean) {
    if (!active) return;
    setBusy(true);
    try {
      const res = await uninstall(
        active.path,
        [...selected],
        toTrash,
        active.removable && removeBundle,
      );
      toast.push(
        `Removed ${res.removed} items · freed ${formatBytes(res.freed)}` +
          (res.failed.length ? ` · ${res.failed.length} failed` : ""),
        res.failed.length ? "err" : "ok",
      );
      if (res.needs_full_disk_access) {
        toast.err(
          "Some items need Full Disk Access — opening Settings; enable Trashly, then retry.",
        );
        openFullDiskAccess();
      }
      setConfirm(false);
      setActivePath(null);
      setLeftovers([]);
      setIcon(null);
      await loadApps();
    } catch (e) {
      toast.err(`Uninstall failed: ${errMsg(e)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="view">
      <header className="view-head" data-tauri-drag-region>
        <div data-tauri-drag-region>
          <h1>
            Uninstall
            {(loading || sizing) && <span className="spinner title-spin" />}
          </h1>
          <p className="muted">
            {apps.length} apps installed{sizing ? " · sizing…" : ""}
          </p>
        </div>
        <input
          className="search"
          placeholder="Search apps…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </header>

      <div className="split">
        <div className="app-list">
          {loading && (
            <div className="empty">
              <span className="spinner" /> Loading apps…
            </div>
          )}
          {filtered.map((a) => (
            <button
              key={a.path}
              className={`app-row ${active?.path === a.path ? "active" : ""}`}
              onClick={() => openApp(a)}
            >
              <span className="app-name">{a.name}</span>
              <span className="app-size">
                {a.size > 0 ? formatBytes(a.size) : sizing ? "…" : "—"}
              </span>
            </button>
          ))}
        </div>

        <div className="app-detail">
          {!active && <div className="empty">Select an app to uninstall.</div>}
          {active && (
            <>
              <div className="detail-title">
                {icon ? (
                  <img className="app-icon" src={icon} alt="" />
                ) : (
                  <div className="app-icon placeholder"><Package size={26} /></div>
                )}
                <div>
                  <h2>{active.name}</h2>
                  <p className="muted mono small">
                    {active.bundle_id || "no bundle id"}
                  </p>
                </div>
              </div>
              {appRunning && (
                <p className="uninstall-warn">
                  <AlertTriangle size={15} />
                  <span>
                    <strong>{active.name}</strong> is running — quit it first, or
                    you may leave it in a broken state.
                  </span>
                </p>
              )}
              <p className="muted small uninstall-hint">
                {!removable ? (
                  <>
                    <strong>{active.name}</strong> is a macOS system app — it
                    can't be removed, but you can still clear its{" "}
                    <strong>checked data</strong> below.
                  </>
                ) : (
                  <>
                    Tick everything you want removed. Untick the{" "}
                    <strong>app bundle</strong> to keep the app and only clean
                    its data.
                  </>
                )}
              </p>
              <div className="detail-section">
                <label className="detail-row strong">
                  <span className="left">
                    <input
                      type="checkbox"
                      checked={willRemoveBundle}
                      disabled={!removable}
                      onChange={() => setRemoveBundle((v) => !v)}
                      title={
                        removable
                          ? "Include the app bundle"
                          : "System app — bundle can't be removed"
                      }
                    />
                    <span>Application bundle</span>
                    {!removable && (
                      <span className="badge">system</span>
                    )}
                  </span>
                  <span>{formatBytes(active.size)}</span>
                </label>
                <p className="section-title">
                  Leftovers{" "}
                  {loadingLeft ? "(scanning…)" : `(${leftovers.length})`}
                </p>
                {leftovers.map((l) => (
                  <label className="detail-row" key={l.path}>
                    <span className="left">
                      <input
                        type="checkbox"
                        checked={selected.has(l.path)}
                        onChange={() =>
                          setSelected((prev) => toggleInSet(prev, l.path))
                        }
                      />
                      <span className="badge">{l.label}</span>
                      {l.from_name && (
                        <span className="badge caution">verify</span>
                      )}
                      <span className="mono small" title={l.path}>
                        {l.path.replace(/^\/Users\/[^/]+/, "~")}
                      </span>
                    </span>
                    <span>{formatBytes(l.size)}</span>
                  </label>
                ))}
                {!loadingLeft && leftovers.length === 0 && (
                  <p className="muted">No leftover files found.</p>
                )}
              </div>

              <footer className="action-bar">
                <span>
                  {willRemoveBundle ? "App bundle + " : ""}
                  <strong>{selected.size}</strong> selected ·{" "}
                  <strong>{formatBytes(bundleSize + leftoverSize)}</strong>
                </span>
                <button
                  className="btn primary danger"
                  onClick={() => setConfirm(true)}
                  disabled={busy || (!willRemoveBundle && selected.size === 0)}
                >
                  {willRemoveBundle ? "Uninstall" : "Clean data"}
                </button>
              </footer>
            </>
          )}
        </div>
      </div>

      <ConfirmDeleteModal
        open={confirm}
        busy={busy}
        title={
          active
            ? willRemoveBundle
              ? `Uninstall ${active.name}?`
              : `Clear ${active.name} data?`
            : "Uninstall?"
        }
        message={
          active
            ? willRemoveBundle
              ? `The app and ${selected.size} selected item${selected.size === 1 ? "" : "s"} (${formatBytes(bundleSize + leftoverSize)}) will be removed.`
              : `${selected.size} selected item${selected.size === 1 ? "" : "s"} (${formatBytes(leftoverSize)}) will be removed. The app itself stays.`
            : undefined
        }
        onTrash={() => runUninstall(true)}
        onDirect={() => runUninstall(false)}
        onCancel={() => setConfirm(false)}
      />
    </div>
  );
}
