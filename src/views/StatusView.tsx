import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  formatBytes,
  formatRate,
  formatUptime,
  status,
  systemInfo,
  type StatusSnapshot,
  type SystemInfo,
} from "../lib/api";

// ── small visual primitives ─────────────────────────────────────────

function hueFor(pct: number) {
  return pct > 85 ? 0 : pct > 60 ? 40 : 150;
}

function Bar({ pct, hue }: { pct: number; hue?: number }) {
  const h = hue ?? hueFor(pct);
  return (
    <div className="bar">
      <div
        className="bar-fill"
        style={{
          width: `${Math.min(100, pct)}%`,
          background: `hsl(${h} 70% 50%)`,
        }}
      />
    </div>
  );
}

function Ring({
  value,
  label,
  sub,
  color,
}: {
  value: number;
  label: string;
  sub?: string;
  color: string;
}) {
  const r = 34;
  const c = 2 * Math.PI * r;
  const off = c * (1 - Math.min(100, Math.max(0, value)) / 100);
  return (
    <div className="ring">
      <svg width="84" height="84" viewBox="0 0 84 84">
        <circle cx="42" cy="42" r={r} className="ring-track" />
        <circle
          cx="42"
          cy="42"
          r={r}
          className="ring-val"
          stroke={color}
          strokeDasharray={c}
          strokeDashoffset={off}
          transform="rotate(-90 42 42)"
        />
      </svg>
      <div className="ring-center">
        <span className="ring-num">{label}</span>
        {sub && <span className="ring-sub">{sub}</span>}
      </div>
    </div>
  );
}

function Spark({
  data,
  color,
  h = 30,
}: {
  data: number[];
  color: string;
  h?: number;
}) {
  const w = 240;
  const pad = 2;
  if (data.length < 2)
    return (
      <svg
        viewBox={`0 0 ${w} ${h}`}
        preserveAspectRatio="none"
        className="spark"
      />
    );
  const max = Math.max(...data, 1);
  const step = w / (data.length - 1);
  const y = (v: number) => pad + (h - 2 * pad) * (1 - v / max);
  const line = data.map(
    (v, i) => `${(i * step).toFixed(1)},${y(v).toFixed(1)}`,
  );
  const area = `0,${h} ${line.join(" ")} ${w},${h}`;
  return (
    <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" className="spark">
      <polygon points={area} fill={color} opacity="0.12" />
      <polyline
        points={line.join(" ")}
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

function Card({
  title,
  badge,
  className,
  children,
}: {
  title: string;
  badge?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={`card ${className ?? ""}`}>
      <div className="card-head">
        <span className="card-title">{title}</span>
        {badge}
      </div>
      {children}
    </div>
  );
}

function ProcTable({
  rows,
  kind,
}: {
  rows: { pid: number; name: string; cpu: number; memory: number }[];
  kind: "cpu" | "mem";
}) {
  return (
    <table className="proc">
      <tbody>
        {rows.map((p) => (
          <tr key={p.pid}>
            <td className="proc-name" title={p.name}>
              {p.name}
            </td>
            <td className="proc-val">
              {kind === "cpu" ? `${p.cpu.toFixed(1)}%` : formatBytes(p.memory)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

// ── view ─────────────────────────────────────────────────────────────

const pressureColor: Record<string, string> = {
  normal: "var(--ok)",
  warn: "#FFD75F",
  critical: "var(--danger)",
};

export default function StatusView() {
  const [s, setS] = useState<StatusSnapshot | null>(null);
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const timer = useRef<number | null>(null);
  const rx = useRef<number[]>([]);
  const tx = useRef<number[]>([]);

  useEffect(() => {
    systemInfo()
      .then(setInfo)
      .catch((e) => console.error("System info failed:", e));
    let alive = true;
    const tick = async () => {
      try {
        const snap = await status();
        if (!alive) return;
        rx.current = [...rx.current, snap.net_rx_bps].slice(-40);
        tx.current = [...tx.current, snap.net_tx_bps].slice(-40);
        setS(snap);
      } catch {
        /* ignore a dropped tick */
      }
      timer.current = window.setTimeout(tick, 1000);
    };
    tick();
    return () => {
      alive = false;
      if (timer.current) clearTimeout(timer.current);
    };
  }, []);

  if (!s)
    return (
      <div className="view">
        <div className="empty">
          <span className="spinner" /> Reading system…
        </div>
      </div>
    );

  const memPct = (s.mem_used / s.mem_total) * 100;
  const swapPct = s.swap_total ? (s.swap_used / s.swap_total) * 100 : 0;
  const scoreColor =
    s.health.score >= 65
      ? "var(--ok)"
      : s.health.score >= 45
        ? "#FFD75F"
        : "var(--danger)";
  const b = s.battery;
  const battColor = !b
    ? "var(--muted)"
    : b.percent < 20
      ? "var(--danger)"
      : b.percent < 50
        ? "#FFD75F"
        : "var(--ok)";

  return (
    <div className="view status-view">
      {/* hero header */}
      <header className="status-hero" data-tauri-drag-region>
        <Ring
          value={s.health.score}
          label={String(s.health.score)}
          sub="/ 100"
          color={scoreColor}
        />
        <div className="hero-info">
          <div className="hero-band" style={{ color: scoreColor }}>
            {s.health.band}
          </div>
          <div className="hero-diag">{s.health.diagnosis}</div>
          <div className="hero-specs">
            {info ? (
              <>
                {info.model} · {info.chip} · {info.cpu_logical} cores
                {info.p_cores > 0 &&
                  ` (${info.p_cores}P/${info.e_cores}E)`} · {info.gpu_name}{" "}
                {info.gpu_cores > 0 && `${info.gpu_cores}-core GPU`} · {info.os}{" "}
                · up {formatUptime(s.uptime_secs)}
              </>
            ) : (
              `up ${formatUptime(s.uptime_secs)}`
            )}
          </div>
        </div>
      </header>

      <div className="status-grid">
        <div className="status-col">
        {/* CPU */}
        <Card
          title="CPU"
          badge={<span className="metric">{s.cpu_usage.toFixed(0)}%</span>}
        >
          <Bar pct={s.cpu_usage} />
          <div className="cores">
            {s.per_core.map((c, i) => (
              <div
                className="core"
                key={i}
                title={`Core ${i}: ${c.toFixed(0)}%`}
              >
                <div
                  className="core-fill"
                  style={{
                    height: `${c}%`,
                    background: `hsl(${hueFor(c)} 70% 50%)`,
                  }}
                />
              </div>
            ))}
          </div>
          <div className="kv muted small">
            <span title="System load average over 1, 5 and 15 minutes">
              load avg · 1m {s.load_avg[0].toFixed(2)} · 5m{" "}
              {s.load_avg[1].toFixed(2)} · 15m {s.load_avg[2].toFixed(2)}
            </span>
          </div>
        </Card>

        {/* Memory */}
        <Card
          title="Memory"
          badge={
            <span
              className="pill"
              style={{ color: pressureColor[s.mem_pressure] }}
            >
              {s.mem_pressure}
            </span>
          }
        >
          <div className="metric-line">
            {formatBytes(s.mem_used)}{" "}
            <span className="muted">/ {formatBytes(s.mem_total)}</span>
          </div>
          <Bar pct={memPct} />
          <div className="kv small muted">
            <span>cached {formatBytes(s.mem_cached)}</span>
            <span>available {formatBytes(s.mem_available)}</span>
          </div>
          {s.swap_total > 0 && (
            <>
              <div className="kv small muted swap-label">
                <span>swap</span>
                <span>
                  {formatBytes(s.swap_used)} / {formatBytes(s.swap_total)}
                </span>
              </div>
              <Bar pct={swapPct} hue={260} />
            </>
          )}
        </Card>

        {/* Top processes */}
        <Card title="Top CPU">
          <ProcTable rows={s.top_cpu} kind="cpu" />
        </Card>

        <Card title="Top Memory">
          <ProcTable rows={s.top_mem} kind="mem" />
        </Card>
        </div>

        <div className="status-col">
        {/* Battery */}
        {b && (
          <Card
            title="Battery"
            badge={<span className="metric">{b.percent}%</span>}
          >
            <div className="batt-row">
              <Ring
                value={b.percent}
                label={`${b.percent}`}
                sub="%"
                color={battColor}
              />
              <div className="batt-stats">
                <div className="kv small">
                  <span className="muted">Status</span>
                  <span>{b.status || "—"}</span>
                </div>
                {b.time_remaining && (
                  <div className="kv small">
                    <span className="muted">Remaining</span>
                    <span>{b.time_remaining}</span>
                  </div>
                )}
                <div className="kv small">
                  <span className="muted">Health</span>
                  <span>{b.health_pct}%</span>
                </div>
                <div className="kv small">
                  <span className="muted">Cycles</span>
                  <span>{b.cycle_count}</span>
                </div>
                <div className="kv small">
                  <span className="muted">Temp</span>
                  <span>{b.temp_c.toFixed(1)}°C</span>
                </div>
                {b.adapter_w > 0 && (
                  <div className="kv small">
                    <span className="muted">Adapter</span>
                    <span>{b.adapter_w} W</span>
                  </div>
                )}
              </div>
            </div>
          </Card>
        )}

        {/* Disks */}
        <Card title="Storage">
          {s.disks.map((d) => {
            const used = d.total - d.available;
            const pct = d.total ? (used / d.total) * 100 : 0;
            return (
              <div key={d.mount} className="disk">
                <div className="disk-head">
                  <span className="mono small">
                    {d.mount} <span className="muted">{d.fs}</span>
                  </span>
                  <span className="muted small">
                    {formatBytes(d.available)} free
                  </span>
                </div>
                <Bar pct={pct} hue={200} />
              </div>
            );
          })}
        </Card>

        {/* Network */}
        <Card
          title="Network"
          badge={
            <div className="net-totals">
              <span className="net-dn">↓ {formatRate(s.net_rx_bps)}</span>
              <span className="net-up">↑ {formatRate(s.net_tx_bps)}</span>
            </div>
          }
        >
          <div className="net-spark">
            <div className="net-spark-row">
              <span className="net-dn small">Download</span>
              <Spark data={rx.current} color="#4f8cff" />
            </div>
            <div className="net-spark-row">
              <span className="net-up small">Upload</span>
              <Spark data={tx.current} color="#43c463" />
            </div>
          </div>
          {info?.external_ip && (
            <div className="net-meta">
              <div className="kv small">
                <span>External IP</span>
                <span className="mono">{info.external_ip}</span>
              </div>
            </div>
          )}
        </Card>

        {/* Wi-Fi */}
        <Card
          title="Wi-Fi"
          badge={
            <span
              className="pill"
              style={{ color: s.wifi.on ? "var(--ok)" : "var(--muted)" }}
            >
              {s.wifi.on ? "on" : "off"}
            </span>
          }
        >
          <div className="kv small">
            <span className="muted">Status</span>
            <span>
              {s.wifi.connected
                ? "Connected"
                : s.wifi.on
                  ? "Not connected"
                  : "Off"}
            </span>
          </div>
          {s.wifi.ssid && (
            <div className="kv small">
              <span className="muted">Network</span>
              <span>{s.wifi.ssid}</span>
            </div>
          )}
          {s.wifi.ip && (
            <div className="kv small">
              <span className="muted">IP</span>
              <span className="mono">{s.wifi.ip}</span>
            </div>
          )}
        </Card>

        {/* Ethernet — only when a wired link is connected */}
        {s.ethernet.connected && (
          <Card
            title="Ethernet"
            badge={
              <span className="pill" style={{ color: "var(--ok)" }}>
                connected
              </span>
            }
          >
            <div className="kv small">
              <span className="muted">Port</span>
              <span>{s.ethernet.name}</span>
            </div>
            <div className="kv small">
              <span className="muted">IP</span>
              <span className="mono">{s.ethernet.ip}</span>
            </div>
          </Card>
        )}

        {/* Bluetooth — shown whenever the controller is on */}
        {s.bt_on && (
          <Card
            title="Bluetooth"
            badge={
              <span className="pill" style={{ color: "var(--ok)" }}>
                on
              </span>
            }
          >
            {s.bluetooth.length > 0 ? (
              s.bluetooth.map((d) => (
                <div className="kv small" key={d.name}>
                  <span>{d.name}</span>
                  <span className="muted">{d.battery || "connected"}</span>
                </div>
              ))
            ) : (
              <div className="muted small">No devices connected</div>
            )}
          </Card>
        )}
        </div>
      </div>
    </div>
  );
}
