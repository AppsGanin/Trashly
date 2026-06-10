import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

/** Open System Settings → Privacy & Security → Full Disk Access. */
export const openFullDiskAccess = () =>
  openUrl(
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles",
  );

// ---- Clean ----
export interface ScanEntry {
  id: string;
  path: string;
  name: string;
  size: number;
}
export interface CategoryResult {
  id: string;
  label: string;
  description: string;
  risk: "safe" | "caution";
  always_direct: boolean;
  total_size: number;
  entries: ScanEntry[];
}
export interface CleanResult {
  removed: number;
  freed: number;
  failed: { path: string; error: string }[];
  needs_full_disk_access: boolean;
}

export const scan = () => invoke<CategoryResult[]>("scan");
export const scanProjects = () => invoke<CategoryResult>("scan_projects");
export const sizePaths = (paths: string[]) =>
  invoke<{ path: string; size: number }[]>("size_paths", { paths });
export const clean = (paths: string[], to_trash: boolean) =>
  invoke<CleanResult>("clean", { req: { paths, to_trash } });

// ---- Status ----
export interface DiskInfo {
  name: string;
  mount: string;
  fs: string;
  total: number;
  available: number;
}
export interface ProcInfo {
  pid: number;
  name: string;
  cpu: number;
  memory: number;
}
export interface NetIface {
  name: string;
  ip: string;
  rx_bps: number;
  tx_bps: number;
}
export interface Battery {
  percent: number;
  status: string;
  time_remaining: string;
  health_pct: number;
  cycle_count: number;
  temp_c: number;
  adapter_w: number;
}
export interface Health {
  score: number;
  band: string;
  diagnosis: string;
}
export interface StatusSnapshot {
  uptime_secs: number;
  cpu_usage: number;
  per_core: number[];
  load_avg: [number, number, number];
  mem_total: number;
  mem_used: number;
  mem_available: number;
  mem_cached: number;
  mem_pressure: string;
  swap_total: number;
  swap_used: number;
  disks: DiskInfo[];
  nets: NetIface[];
  net_rx_bps: number;
  net_tx_bps: number;
  top_cpu: ProcInfo[];
  top_mem: ProcInfo[];
  battery: Battery | null;
  wifi: Wifi;
  ethernet: EthLink;
  bt_on: boolean;
  bluetooth: BtDevice[];
  health: Health;
}
export interface BtDevice {
  name: string;
  connected: boolean;
  battery: string;
}
export interface Wifi {
  on: boolean;
  connected: boolean;
  ip: string;
  ssid: string;
}
export interface EthLink {
  connected: boolean;
  ip: string;
  name: string;
}
export interface SystemInfo {
  host_name: string;
  os: string;
  model: string;
  chip: string;
  cpu_logical: number;
  cpu_physical: number;
  p_cores: number;
  e_cores: number;
  gpu_name: string;
  gpu_cores: number;
  metal: string;
  external_ip: string;
}
export const status = () => invoke<StatusSnapshot>("status");
export const systemInfo = () => invoke<SystemInfo>("system_info");

// ---- Uninstall ----
export interface AppInfo {
  name: string;
  path: string;
  bundle_id: string;
  size: number;
  removable: boolean;
}
export interface Leftover {
  path: string;
  label: string;
  size: number;
  from_name: boolean;
}
export interface UninstallResult {
  removed: number;
  freed: number;
  failed: string[];
  needs_full_disk_access: boolean;
}
export const listApps = () => invoke<AppInfo[]>("list_apps");
export const appIcon = (app_path: string) =>
  invoke<string | null>("app_icon", { appPath: app_path });
export const appLeftovers = (bundle_id: string, name: string) =>
  invoke<Leftover[]>("app_leftovers", { bundleId: bundle_id, name });
export const uninstall = (
  app_path: string,
  leftover_paths: string[],
  to_trash: boolean,
  remove_bundle: boolean,
) =>
  invoke<UninstallResult>("uninstall", {
    req: { app_path, leftover_paths, to_trash, remove_bundle },
  });

// ---- Optimize ----
export interface OptimizationInfo {
  id: string;
  label: string;
  description: string;
  needs_admin: boolean;
}
export interface RunResult {
  success: boolean;
  output: string;
}
export const listOptimizations = () =>
  invoke<OptimizationInfo[]>("list_optimizations");
export const runOptimization = (id: string) =>
  invoke<RunResult>("run_optimization", { id });

// ---- Settings (menu-bar tray) ----
export type Metric = "cpu" | "memory" | "disk" | "battery";
export interface TraySettings {
  title: Metric[]; // shown in the menu-bar title
  menu: Metric[]; // shown in the dropdown
}
export const getTraySettings = () => invoke<TraySettings>("get_tray_settings");
export const setTraySettings = (settings: TraySettings) =>
  invoke<void>("set_tray_settings", { settings });
export const trayHasBattery = () => invoke<boolean>("tray_has_battery");

// ---- Duplicates ----
export interface DupeFile {
  path: string;
  name: string;
  modified: number;
}
export interface DupeGroup {
  hash: string;
  size: number;
  count: number;
  wasted: number;
  files: DupeFile[];
}
export interface DupeResult {
  groups: DupeGroup[];
  total_wasted: number;
  scanned: number;
  unreadable: string[];
  skipped_icloud: number;
}
export interface RootInfo {
  key: string;
  label: string;
  path: string;
}
export const dupeRoots = () => invoke<RootInfo[]>("dupe_roots");
export const scanDuplicates = (roots: string[], min_size: number) =>
  invoke<DupeResult>("scan_duplicates", { req: { roots, min_size } });

// ---- Similar photos (perceptual hash) ----
export interface PhotoFile {
  path: string;
  name: string;
  size: number;
  modified: number;
  thumb: string;
}
export interface PhotoGroup {
  count: number;
  wasted: number;
  files: PhotoFile[];
}
export interface PhotoResult {
  groups: PhotoGroup[];
  total_wasted: number;
  scanned: number;
  truncated: boolean;
  unreadable: string[];
  skipped_icloud: number;
}
export const scanSimilarPhotos = (
  roots: string[],
  min_size: number,
  threshold: number,
) => invoke<PhotoResult>("scan_similar_photos", { req: { roots, min_size, threshold } });

// Progress event payload emitted during a scan (listen on "scan-progress").
export interface ScanProgress {
  phase: "walk" | "hash";
  done: number;
  total: number;
}
export const cancelScan = () => invoke<void>("cancel_scan");

// ---- Shared user-file removal (Duplicates) ----
export interface RemoveResult {
  removed: number;
  freed: number;
  failed: string[];
  needs_full_disk_access: boolean;
}
export const removeFiles = (paths: string[], to_trash: boolean) =>
  invoke<RemoveResult>("remove_files", { paths, toTrash: to_trash });

// ---- Protected folders (whitelist) ----
export const getProtectedPaths = () => invoke<string[]>("get_protected_paths");
export const setProtectedPaths = (paths: string[]) =>
  invoke<void>("set_protected_paths", { paths });

// ---- Cleanup log ----
export interface CleanupEntry {
  time: number;
  source: string;
  path: string;
  size: number;
  to_trash: boolean;
}
export const getCleanupLog = (limit: number) =>
  invoke<CleanupEntry[]>("get_cleanup_log", { limit });
export const clearCleanupLog = () => invoke<void>("clear_cleanup_log");

// ---- Finder helpers ----
export const revealInFinder = (path: string) =>
  invoke<void>("reveal_in_finder", { path });
export const quickLook = (path: string) => invoke<void>("quick_look", { path });
export const isAppRunning = (app_path: string) =>
  invoke<boolean>("is_app_running", { appPath: app_path });

// ---- helpers ----
export function formatBytes(n: number): string {
  if (n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
  const v = n / Math.pow(1024, i);
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

export function formatRate(bps: number): string {
  return `${formatBytes(bps)}/s`;
}

export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

export function formatUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}
