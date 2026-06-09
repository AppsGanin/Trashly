//! System status dashboard, modelled on Mole's `status` command.
//!
//! Two commands:
//! * `status()` — fast, called on a ~2s loop: CPU, memory detail, disks,
//!   network rates, processes, battery, health score.
//! * `system_info()` — slow/static, called once: hardware model, chip, CPU
//!   core topology, GPU, OS, Bluetooth. Cached by the frontend.

use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde::Serialize;
use sysinfo::{Disks, System};

/// Persistent state for network rate calculations: previous cumulative
/// (rx, tx) bytes per interface and when we last sampled.
struct RateState {
    prev: std::collections::HashMap<String, (u64, u64)>,
    last: Option<Instant>,
}
static RATE: OnceLock<Mutex<RateState>> = OnceLock::new();

fn run(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

// ─────────────────────────── data types ───────────────────────────

#[derive(Serialize, Clone)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub memory: u64,
}

#[derive(Serialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub available: u64,
}

#[derive(Serialize)]
pub struct NetIface {
    pub name: String,
    pub ip: String,
    pub rx_bps: u64,
    pub tx_bps: u64,
}

#[derive(Serialize)]
pub struct Battery {
    pub percent: u8,
    pub status: String,
    pub time_remaining: String,
    pub health_pct: u8,
    pub cycle_count: u32,
    pub temp_c: f32,
    pub adapter_w: u32,
}

#[derive(Serialize)]
pub struct Health {
    pub score: u8,
    pub band: String,
    pub diagnosis: String,
}

#[derive(Serialize)]
pub struct StatusSnapshot {
    pub uptime_secs: u64,
    // CPU
    pub cpu_usage: f32,
    pub per_core: Vec<f32>,
    pub load_avg: [f64; 3],
    // memory
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_available: u64,
    pub mem_cached: u64,
    pub mem_pressure: String,
    pub swap_total: u64,
    pub swap_used: u64,
    // disks
    pub disks: Vec<DiskInfo>,
    // network
    pub nets: Vec<NetIface>,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    // processes
    pub top_cpu: Vec<ProcInfo>,
    pub top_mem: Vec<ProcInfo>,
    // battery (None on desktop Macs)
    pub battery: Option<Battery>,
    // connectivity — live so cards appear/disappear on toggle
    pub wifi: Wifi,
    pub ethernet: EthLink,
    pub bt_on: bool,
    pub bluetooth: Vec<BtDevice>,
    // health
    pub health: Health,
}

// ─────────────────────────── status() ───────────────────────────

/// Async wrapper so the ~500ms sampling sleep runs off Tauri's main thread
/// (sync commands block it, which janks the whole UI every tick).
#[tauri::command]
pub async fn status() -> StatusSnapshot {
    tauri::async_runtime::spawn_blocking(status_impl)
        .await
        .expect("status task panicked")
}

fn status_impl() -> StatusSnapshot {
    // Single ~500ms sample window shared by CPU and per-process CPU.
    let mut sys = System::new();
    sys.refresh_cpu_all();
    let snap1 = ps_cpu_times();
    let t = Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let dt = t.elapsed().as_secs_f64();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
    let load = System::load_average();

    // ---- memory ----
    let mem_total = sys.total_memory();
    let mem_used = sys.used_memory();
    // macOS keeps truly-"free" memory near zero (it's all used for cache), so
    // sysinfo's available_memory() reads ~0 and is misleading. Report what apps
    // can actually claim: total minus in-use (the rest is free + reclaimable).
    let mem_available = mem_total.saturating_sub(mem_used);
    let mem_cached = vm_stat_file_backed();
    let swap_total = sys.total_swap();
    let swap_used = sys.used_swap();
    let used_pct = if mem_total > 0 {
        mem_used as f64 / mem_total as f64 * 100.0
    } else {
        0.0
    };
    let mem_pressure = memory_pressure(used_pct);

    // ---- disks (dedup APFS container volumes) ----
    let mut seen = std::collections::HashSet::new();
    let disks: Vec<DiskInfo> = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|d| {
            let m = d.mount_point().to_string_lossy();
            m == "/" || m.starts_with("/Volumes/")
        })
        .filter(|d| seen.insert((d.total_space(), d.available_space())))
        .map(|d| DiskInfo {
            name: d.name().to_string_lossy().to_string(),
            mount: d.mount_point().to_string_lossy().to_string(),
            fs: d.file_system().to_string_lossy().to_string(),
            total: d.total_space(),
            available: d.available_space(),
        })
        .collect();

    // ---- network rates ----
    let (nets, net_rx_bps, net_tx_bps) = network_rates();

    // ---- processes ----
    // Normalize per-process CPU to the whole-machine scale (divide by logical
    // core count) so it matches the global CPU gauge: a process pinning one
    // core reads 100/ncores %, not 100%.
    let ncpu = per_core.len().max(1) as f32;
    let mut procs = processes_via_ps(&snap1, dt, ncpu);
    // `ps` RSS badly undercounts on macOS; use `top`'s phys-footprint (what
    // Activity Monitor's "Memory" column shows) per pid instead.
    let footprint = phys_footprint_by_pid();
    for p in &mut procs {
        if let Some(m) = footprint.get(&p.pid) {
            p.memory = *m;
        }
    }
    let mut top_cpu = procs.clone();
    top_cpu.sort_by(|a, b| {
        b.cpu
            .partial_cmp(&a.cpu)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    top_cpu.truncate(6);
    procs.sort_by(|a, b| b.memory.cmp(&a.memory));
    procs.truncate(6);

    // ---- battery ----
    let battery = read_battery();

    // ---- connectivity (all fast; polled live) ----
    let wifi = read_wifi();
    let ethernet = read_ethernet();
    let (bt_on, bluetooth) = read_bluetooth();

    // ---- health ----
    let cpu_usage = sys.global_cpu_usage();
    let disk_pct = disks
        .iter()
        .map(|d| {
            if d.total > 0 {
                (d.total - d.available) as f64 / d.total as f64 * 100.0
            } else {
                0.0
            }
        })
        .fold(0.0_f64, f64::max);
    let uptime = System::uptime();
    let health = compute_health(
        cpu_usage as f64,
        used_pct,
        &mem_pressure,
        disk_pct,
        uptime,
        battery.as_ref(),
        &top_cpu,
    );

    StatusSnapshot {
        uptime_secs: uptime,
        cpu_usage,
        per_core,
        load_avg: [load.one, load.five, load.fifteen],
        mem_total,
        mem_used,
        mem_available,
        mem_cached,
        mem_pressure,
        swap_total,
        swap_used,
        disks,
        nets,
        net_rx_bps,
        net_tx_bps,
        top_cpu,
        top_mem: procs,
        battery,
        wifi,
        ethernet,
        bt_on,
        bluetooth,
        health,
    }
}

// ─────────────────────────── processes ───────────────────────────

fn processes_via_ps(
    snap1: &std::collections::HashMap<u32, f64>,
    dt: f64,
    ncpu: f32,
) -> Vec<ProcInfo> {
    let text = run("/bin/ps", &["-axo", "pid=,time=,rss=,comm="]);
    let mut result = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(pid_s), Some(time_s), Some(rss_s)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let rest: String = it.collect::<Vec<_>>().join(" ");
        let name = rest
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&rest)
            .to_string();
        let pid: u32 = pid_s.parse().unwrap_or(0);
        let now = parse_cputime(time_s);
        let prev = snap1.get(&pid).copied().unwrap_or(now);
        let cpu = if dt > 0.0 {
            ((((now - prev) / dt) * 100.0).max(0.0) as f32) / ncpu
        } else {
            0.0
        };
        result.push(ProcInfo {
            pid,
            name,
            cpu,
            memory: rss_s.parse::<u64>().unwrap_or(0) * 1024,
        });
    }
    result
}

fn ps_cpu_times() -> std::collections::HashMap<u32, f64> {
    let mut map = std::collections::HashMap::new();
    for line in run("/bin/ps", &["-axo", "pid=,time="]).lines() {
        let mut it = line.split_whitespace();
        if let (Some(p), Some(t)) = (it.next(), it.next()) {
            if let Ok(pid) = p.parse::<u32>() {
                map.insert(pid, parse_cputime(t));
            }
        }
    }
    map
}

/// Per-pid physical memory footprint (bytes) from `top -l 1`, matching Activity
/// Monitor's "Memory" column far better than `ps` RSS.
fn phys_footprint_by_pid() -> std::collections::HashMap<u32, u64> {
    let mut map = std::collections::HashMap::new();
    let out = run(
        "/usr/bin/top",
        &["-l", "1", "-o", "mem", "-n", "40", "-stats", "pid,mem"],
    );
    let mut in_rows = false;
    for line in out.lines() {
        let t = line.trim_start();
        if t.starts_with("PID") {
            in_rows = true; // column header reached
            continue;
        }
        if !in_rows {
            continue;
        }
        let mut it = t.split_whitespace();
        if let (Some(pid_s), Some(mem_s)) = (it.next(), it.next()) {
            if let Ok(pid) = pid_s.parse::<u32>() {
                map.insert(pid, parse_top_mem(mem_s));
            }
        }
    }
    map
}

/// Parse a `top` memory value like "1679M", "744M", "512K", "2G" into bytes.
fn parse_top_mem(s: &str) -> u64 {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('K') => (&s[..s.len() - 1], 1024u64),
        Some('M') => (&s[..s.len() - 1], 1024 * 1024),
        Some('G') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        Some('B') => (&s[..s.len() - 1], 1),
        _ => (s, 1),
    };
    num.trim()
        .parse::<f64>()
        .map(|v| (v * mult as f64) as u64)
        .unwrap_or(0)
}

/// Parse `[[dd-]hh:]mm:ss[.frac]` into seconds.
fn parse_cputime(s: &str) -> f64 {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<f64>().unwrap_or(0.0), r),
        None => (0.0, s),
    };
    let mut secs = days * 86400.0;
    let parts: Vec<&str> = rest.split(':').collect();
    let n = parts.len();
    for (i, part) in parts.iter().enumerate() {
        let v = part.parse::<f64>().unwrap_or(0.0);
        secs += v * 60f64.powi((n - 1 - i) as i32);
    }
    secs
}

// ─────────────────────────── memory ───────────────────────────

/// Memory pressure straight from the kernel signal macOS itself uses
/// (`kern.memorystatus_vm_pressure_level`: 1 normal, 2 warn, 4 critical) — far
/// more accurate than guessing from used% + swap. Falls back to a lenient
/// used%-based heuristic if the sysctl is unavailable.
fn memory_pressure(used_pct: f64) -> String {
    match run(
        "/usr/sbin/sysctl",
        &["-n", "kern.memorystatus_vm_pressure_level"],
    )
    .trim()
    .parse::<u32>()
    {
        Ok(4) => "critical",
        Ok(2) => "warn",
        Ok(1) => "normal",
        _ if used_pct > 95.0 => "critical",
        _ if used_pct > 88.0 => "warn",
        _ => "normal",
    }
    .to_string()
}

/// File-backed (cached) memory in bytes, parsed from `vm_stat`.
fn vm_stat_file_backed() -> u64 {
    let text = run("/usr/bin/vm_stat", &[]);
    let page_size = text
        .lines()
        .next()
        .and_then(|l| {
            l.split("page size of ")
                .nth(1)
                .and_then(|s| s.split(' ').next())
                .and_then(|n| n.parse::<u64>().ok())
        })
        .unwrap_or(4096);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("File-backed pages:") {
            let n: u64 = rest.trim().trim_end_matches('.').parse().unwrap_or(0);
            return n * page_size;
        }
    }
    0
}

// ─────────────────────────── network ───────────────────────────

/// Per-interface and total RX/TX rates in bytes/sec.
///
/// Reads cumulative byte counters straight from `netstat -ib` (the kernel's
/// own counters, same source as Activity Monitor) and diffs them against the
/// previous sample. We do NOT use sysinfo here — its network counters are
/// unreliable on recent macOS, same as its per-process CPU.
fn network_rates() -> (Vec<NetIface>, u64, u64) {
    let cell = RATE.get_or_init(|| {
        Mutex::new(RateState {
            prev: std::collections::HashMap::new(),
            last: None,
        })
    });
    let Ok(mut state) = cell.lock() else {
        return (Vec::new(), 0, 0);
    };
    let elapsed = state.last.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
    state.last = Some(Instant::now());

    // Parse the first (Link-level) row per physical interface: its Ibytes (col
    // 7) and Obytes (col 10) are the cumulative counters.
    let out = run("/usr/sbin/netstat", &["-ib"]);
    let mut current: Vec<(String, u64, u64)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in out.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }
        let name = cols[0];
        // Skip loopback / virtual / VPN — they aren't real network throughput.
        let noisy = [
            "lo", "awdl", "utun", "llw", "bridge", "gif", "stf", "anpi", "ap",
        ]
        .iter()
        .any(|p| name.starts_with(p));
        if noisy || seen.contains(name) {
            continue;
        }
        let (Ok(rx), Ok(tx)) = (cols[6].parse::<u64>(), cols[9].parse::<u64>()) else {
            continue; // header or a row without numeric counters
        };
        seen.insert(name.to_string());
        current.push((name.to_string(), rx, tx));
    }

    let ips = interface_ips();
    let mut nets = Vec::new();
    let (mut total_rx, mut total_tx) = (0u64, 0u64);
    let mut next_prev = std::collections::HashMap::new();
    for (name, rx_total, tx_total) in current {
        let (prx, ptx) = state
            .prev
            .get(&name)
            .copied()
            .unwrap_or((rx_total, tx_total));
        let rx = if elapsed > 0.0 {
            (rx_total.saturating_sub(prx) as f64 / elapsed) as u64
        } else {
            0
        };
        let tx = if elapsed > 0.0 {
            (tx_total.saturating_sub(ptx) as f64 / elapsed) as u64
        } else {
            0
        };
        total_rx += rx;
        total_tx += tx;
        nets.push(NetIface {
            name: name.clone(),
            ip: ips.get(&name).cloned().unwrap_or_default(),
            rx_bps: rx,
            tx_bps: tx,
        });
        next_prev.insert(name, (rx_total, tx_total));
    }
    state.prev = next_prev;

    nets.retain(|n| !n.ip.is_empty() || n.rx_bps + n.tx_bps > 0);
    nets.sort_by(|a, b| (b.rx_bps + b.tx_bps).cmp(&(a.rx_bps + a.tx_bps)));
    nets.truncate(3);
    (nets, total_rx, total_tx)
}

fn interface_ips() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    // `ifconfig -a` lines: "en0: ..." then "\tinet 192.168.x.x ..."
    let text = run("/sbin/ifconfig", &["-a"]);
    let mut current = String::new();
    for line in text.lines() {
        if !line.starts_with('\t') && !line.starts_with(' ') {
            current = line.split(':').next().unwrap_or("").to_string();
        } else if let Some(rest) = line.trim().strip_prefix("inet ") {
            if let Some(ip) = rest.split_whitespace().next() {
                if !ip.starts_with("127.") && !map.contains_key(&current) {
                    map.insert(current.clone(), ip.to_string());
                }
            }
        }
    }
    map
}

// ─────────────────────────── battery ───────────────────────────

fn read_battery() -> Option<Battery> {
    let batt = run("/usr/bin/pmset", &["-g", "batt"]);
    // The percent line only exists when a battery is present.
    let pct_line = batt.lines().find(|l| l.contains('%'))?;
    let percent = pct_line
        .split('%')
        .next()
        .and_then(|s| s.rsplit(|c: char| !c.is_ascii_digit()).next())
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    // status word after the first ';'
    let status = pct_line
        .split(';')
        .nth(1)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let time_remaining = pct_line
        .split(';')
        .nth(2)
        .map(|s| {
            s.trim()
                .replace("remaining present: true", "")
                .trim()
                .to_string()
        })
        .filter(|s| s.contains(':'))
        .unwrap_or_default();

    let ioreg = run("/usr/sbin/ioreg", &["-rn", "AppleSmartBattery"]);
    let cycle_count = ioreg_int(&ioreg, "CycleCount").unwrap_or(0) as u32;
    let temp_raw = ioreg_int(&ioreg, "Temperature").unwrap_or(0) as f32;
    let temp_c = if temp_raw > 1000.0 {
        temp_raw / 100.0
    } else {
        temp_raw
    };
    let design = ioreg_int(&ioreg, "DesignCapacity").unwrap_or(0) as f64;
    let raw_max = ioreg_int(&ioreg, "AppleRawMaxCapacity").unwrap_or(0);
    let nominal = ioreg_int(&ioreg, "NominalChargeCapacity").unwrap_or(0);
    let max_cap = raw_max.max(nominal) as f64;
    let health_pct = if design > 0.0 {
        ((max_cap / design * 100.0).clamp(0.0, 100.0)) as u8
    } else {
        0
    };
    let adapter_w = ioreg_watts(&ioreg);

    Some(Battery {
        percent,
        status,
        time_remaining,
        health_pct,
        cycle_count,
        temp_c,
        adapter_w,
    })
}

/// Find `"<key>" = <int>` in ioreg output.
fn ioreg_int(text: &str, key: &str) -> Option<i64> {
    let re = regex::Regex::new(&format!(r#""{}"\s*=\s*(-?\d+)"#, regex::escape(key))).ok()?;
    re.captures(text)?.get(1)?.as_str().parse().ok()
}

/// Adapter wattage from AdapterDetails ("Watts"=NN), skipping the raw variant.
fn ioreg_watts(text: &str) -> u32 {
    let re = regex::Regex::new(r#""Watts"\s*=\s*(\d+)"#).unwrap();
    re.captures(text)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0)
}

// ─────────────────────────── health ───────────────────────────

fn compute_health(
    cpu: f64,
    mem: f64,
    pressure: &str,
    disk: f64,
    uptime: u64,
    battery: Option<&Battery>,
    top_cpu: &[ProcInfo],
) -> Health {
    let mut score = 100.0_f64;
    // CPU (weight ~30)
    if cpu > 85.0 {
        score -= 30.0;
    } else if cpu > 50.0 {
        score -= 15.0 * (cpu - 50.0) / 35.0;
    }
    // Memory (weight ~25)
    if mem > 88.0 {
        score -= 25.0;
    } else if mem > 70.0 {
        score -= 12.0 * (mem - 70.0) / 18.0;
    }
    match pressure {
        "warn" => score -= 5.0,
        "critical" => score -= 15.0,
        _ => {}
    }
    // Disk (weight ~20)
    if disk > 93.0 {
        score -= 20.0;
    } else if disk > 80.0 {
        score -= 10.0 * (disk - 80.0) / 13.0;
    }
    // Battery wear
    if let Some(b) = battery {
        if b.cycle_count > 900 || b.health_pct < 60 {
            score -= 5.0;
        } else if b.cycle_count > 800 || b.health_pct < 80 {
            score -= 2.0;
        }
    }
    // Uptime
    let days = uptime / 86400;
    if days > 14 {
        score -= 3.0;
    } else if days > 7 {
        score -= 1.0;
    }
    let score = score.clamp(0.0, 100.0) as u8;
    let band = match score {
        85..=100 => "Excellent",
        65..=84 => "Good",
        45..=64 => "Fair",
        _ => "Needs Attention",
    }
    .to_string();

    // Diagnosis, priority order.
    let diagnosis = if cpu > 85.0 {
        let hot = top_cpu.first().map(|p| p.name.as_str()).unwrap_or("");
        if hot.is_empty() {
            "CPU load high".into()
        } else {
            format!("CPU load high · {hot}")
        }
    } else if pressure == "critical" || mem > 88.0 {
        "Memory pressure high".into()
    } else if disk > 93.0 {
        "Disk almost full".into()
    } else if battery
        .map(|b| b.health_pct < 80 && b.health_pct > 0)
        .unwrap_or(false)
    {
        "Battery health low".into()
    } else if battery.map(|b| b.cycle_count > 800).unwrap_or(false) {
        "Battery cycles high".into()
    } else if days > 7 {
        "Restart recommended".into()
    } else {
        "All clear".into()
    };

    Health {
        score,
        band,
        diagnosis,
    }
}

// ─────────────────────────── system_info() ───────────────────────────

#[derive(Serialize)]
pub struct BtDevice {
    pub name: String,
    pub connected: bool,
    pub battery: String,
}

#[derive(Serialize)]
pub struct Wifi {
    pub on: bool,
    pub connected: bool,
    pub ip: String,
    pub ssid: String,
}

#[derive(Serialize)]
pub struct EthLink {
    pub connected: bool,
    pub ip: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct SystemInfo {
    pub host_name: String,
    pub os: String,
    pub model: String,
    pub chip: String,
    pub cpu_logical: u32,
    pub cpu_physical: u32,
    pub p_cores: u32,
    pub e_cores: u32,
    pub gpu_name: String,
    pub gpu_cores: u32,
    pub metal: String,
    pub external_ip: String,
}

#[tauri::command]
pub async fn system_info() -> SystemInfo {
    tauri::async_runtime::spawn_blocking(system_info_impl)
        .await
        .expect("system_info task panicked")
}

fn system_info_impl() -> SystemInfo {
    let hw = run("/usr/sbin/system_profiler", &["SPHardwareDataType"]);
    let model = sp_field(&hw, "Model Name").unwrap_or_default();
    let chip = sp_field(&hw, "Chip")
        .or_else(|| sp_field(&hw, "Processor Name"))
        .unwrap_or_default();

    let cpu_logical = sysctl_u32("hw.logicalcpu");
    let cpu_physical = sysctl_u32("hw.physicalcpu");
    // Perf-level topology (Apple Silicon).
    let (mut p_cores, mut e_cores) = (0u32, 0u32);
    let l0 = sysctl_u32("hw.perflevel0.logicalcpu");
    let l1 = sysctl_u32("hw.perflevel1.logicalcpu");
    let n0 = run("/usr/sbin/sysctl", &["-n", "hw.perflevel0.name"]);
    if n0.to_lowercase().contains("performance") {
        p_cores = l0;
        e_cores = l1;
    } else if n0.to_lowercase().contains("efficiency") {
        e_cores = l0;
        p_cores = l1;
    }

    let disp = run("/usr/sbin/system_profiler", &["SPDisplaysDataType"]);
    let gpu_name = sp_field(&disp, "Chipset Model").unwrap_or_else(|| chip.clone());
    let gpu_cores = sp_field(&disp, "Total Number of Cores")
        .and_then(|s| s.split_whitespace().next().map(String::from))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let metal = sp_field(&disp, "Metal Support")
        .or_else(|| sp_field(&disp, "Metal"))
        .unwrap_or_default();

    let os = format!(
        "{} {}",
        System::name().unwrap_or_default(),
        System::os_version().unwrap_or_default()
    );

    SystemInfo {
        host_name: System::host_name().unwrap_or_default(),
        os,
        model,
        chip,
        cpu_logical,
        cpu_physical,
        p_cores,
        e_cores,
        gpu_name,
        gpu_cores,
        metal,
        external_ip: external_ip(),
    }
}

fn sysctl_u32(key: &str) -> u32 {
    run("/usr/sbin/sysctl", &["-n", key])
        .trim()
        .parse()
        .unwrap_or(0)
}

/// Parse a `Key: Value` field from system_profiler text output.
fn sp_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("{}:", key.to_lowercase());
    for line in text.lines() {
        let l = line.trim();
        if l.to_lowercase().starts_with(&needle) {
            let v = l[needle.len()..].trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Returns (controller powered on, connected devices).
fn read_bluetooth() -> (bool, Vec<BtDevice>) {
    let text = run("/usr/sbin/system_profiler", &["SPBluetoothDataType"]);
    // Controller power: a "State: On" line under the controller section.
    let on = text.lines().any(|l| {
        let t = l.trim();
        t.starts_with("State:") && t.to_lowercase().contains("on")
    });
    let mut devices = Vec::new();
    let mut current: Option<(String, bool, String)> = None;
    let indent_of = |l: &str| l.len() - l.trim_start().len();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = indent_of(line);
        let trimmed = line.trim();
        // Device headers sit at 8-space indent and end with ':' with no value.
        if indent == 8 && trimmed.ends_with(':') {
            if let Some(d) = current.take() {
                if d.1 {
                    devices.push(BtDevice {
                        name: d.0,
                        connected: d.1,
                        battery: d.2,
                    });
                }
            }
            current = Some((
                trimmed.trim_end_matches(':').to_string(),
                false,
                String::new(),
            ));
        } else if let Some(cur) = current.as_mut() {
            if let Some(v) = trimmed.strip_prefix("Connected:") {
                cur.1 = v.trim().eq_ignore_ascii_case("yes");
            } else if let Some(v) = trimmed.strip_prefix("Battery Level:") {
                cur.2 = v.trim().to_string();
            }
        }
    }
    if let Some(d) = current.take() {
        if d.1 {
            devices.push(BtDevice {
                name: d.0,
                connected: d.1,
                battery: d.2,
            });
        }
    }
    (on, devices)
}

/// Best-effort external (public) IP, via a quick HTTPS call.
fn external_ip() -> String {
    let out = run(
        "/usr/bin/curl",
        &["-s", "--max-time", "4", "https://api.ipify.org"],
    );
    let ip = out.trim();
    // Sanity-check it looks like an IPv4 address.
    if ip.split('.').count() == 4 && ip.chars().all(|c| c.is_ascii_digit() || c == '.') {
        ip.to_string()
    } else {
        String::new()
    }
}

/// Parse `networksetup -listallhardwareports` into (port name, device) pairs.
fn hardware_ports() -> Vec<(String, String)> {
    let text = run("/usr/sbin/networksetup", &["-listallhardwareports"]);
    let mut out = Vec::new();
    let mut port: Option<String> = None;
    for l in text.lines() {
        let t = l.trim();
        if let Some(p) = t.strip_prefix("Hardware Port:") {
            port = Some(p.trim().to_string());
        } else if let Some(d) = t.strip_prefix("Device:") {
            if let Some(p) = port.take() {
                out.push((p, d.trim().to_string()));
            }
        }
    }
    out
}

/// First connected wired link (Ethernet / Thunderbolt / USB LAN) with an IP.
fn read_ethernet() -> EthLink {
    for (port, device) in hardware_ports() {
        let lc = port.to_lowercase();
        let is_wired = lc.contains("ethernet") || lc.contains("lan");
        if !is_wired || lc.contains("wi-fi") {
            continue;
        }
        let ip = run("/usr/sbin/ipconfig", &["getifaddr", &device])
            .trim()
            .to_string();
        if !ip.is_empty() {
            return EthLink {
                connected: true,
                ip,
                name: port,
            };
        }
    }
    EthLink {
        connected: false,
        ip: String::new(),
        name: String::new(),
    }
}

/// Wi-Fi status. SSID is privacy-restricted on recent macOS (needs Location
/// access), so it is often empty — we still report power and connection.
fn read_wifi() -> Wifi {
    let device = hardware_ports()
        .into_iter()
        .find(|(p, _)| p.to_lowercase().contains("wi-fi"))
        .map(|(_, d)| d)
        .unwrap_or_default();
    if device.is_empty() {
        return Wifi {
            on: false,
            connected: false,
            ip: String::new(),
            ssid: String::new(),
        };
    }
    let power = run("/usr/sbin/networksetup", &["-getairportpower", &device]);
    let on = power.to_lowercase().contains(": on");
    let ip = run("/usr/sbin/ipconfig", &["getifaddr", &device])
        .trim()
        .to_string();
    let net = run("/usr/sbin/networksetup", &["-getairportnetwork", &device]);
    let ssid = net
        .split("Current Wi-Fi Network:")
        .nth(1)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    Wifi {
        on,
        connected: !ip.is_empty(),
        ip,
        ssid,
    }
}
