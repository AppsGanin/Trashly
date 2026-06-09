import { useEffect, useState } from "react";
import {
  listOptimizations,
  runOptimization,
  type OptimizationInfo,
} from "../lib/api";
import { useToast } from "../lib/toast";

export default function OptimizeView() {
  const [tasks, setTasks] = useState<OptimizationInfo[]>([]);
  const [running, setRunning] = useState<string | null>(null);
  const toast = useToast();

  useEffect(() => {
    listOptimizations().then(setTasks);
  }, []);

  async function run(task: OptimizationInfo) {
    setRunning(task.id);
    try {
      const res = await runOptimization(task.id);
      // The full command output (e.g. brew's warnings) is noisy, so the toast
      // stays concise: just the outcome, with the error detail on failure.
      if (res.success) toast.ok(`${task.label} — done`);
      else toast.err(`${task.label} failed: ${res.output.slice(0, 200)}`);
    } catch {
      toast.err(`${task.label} failed`);
    } finally {
      setRunning(null);
    }
  }

  return (
    <div className="view">
      <header className="view-head" data-tauri-drag-region>
        <div data-tauri-drag-region>
          <h1>Optimize</h1>
          <p className="muted">One-shot macOS maintenance tasks</p>
        </div>
      </header>

      <div className="task-list">
        {tasks.map((t) => (
          <div className="task" key={t.id}>
            <div className="task-main">
              <div className="task-info">
                <span className="task-label">
                  {t.label}
                  {t.needs_admin && <span className="badge">admin</span>}
                </span>
                <span className="muted">{t.description}</span>
              </div>
              <button
                className="btn ghost"
                onClick={() => run(t)}
                disabled={running !== null}
              >
                {running === t.id ? "Running…" : "Run"}
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
