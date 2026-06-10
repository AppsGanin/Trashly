import { useEffect, useState } from "react";
import { ShieldAlert } from "lucide-react";
import {
  listOptimizations,
  runOptimization,
  type OptimizationInfo,
} from "../lib/api";
import { useToast } from "../lib/toast";
import Modal from "./Modal";

export default function OptimizeView() {
  const [tasks, setTasks] = useState<OptimizationInfo[]>([]);
  const [running, setRunning] = useState<string | null>(null);
  const [pending, setPending] = useState<OptimizationInfo | null>(null);
  const toast = useToast();

  useEffect(() => {
    listOptimizations().then(setTasks);
  }, []);

  async function run(task: OptimizationInfo) {
    setPending(null);
    setRunning(task.id);
    try {
      const res = await runOptimization(task.id);
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
                onClick={() => setPending(t)}
                disabled={running !== null}
              >
                {running === t.id ? "Running…" : "Run"}
              </button>
            </div>
          </div>
        ))}
      </div>

      <Modal open={!!pending} onClose={() => setPending(null)}>
        <h2 className="modal-title">Run “{pending?.label}”?</h2>
        <p className="modal-msg">{pending?.description}</p>
        {pending?.needs_admin && (
          <p className="modal-warn">
            <ShieldAlert size={16} />
            Needs administrator rights — macOS will ask for your password. This
            affects the whole system.
          </p>
        )}
        <div className="modal-footer end">
          <button className="btn ghost" onClick={() => setPending(null)}>
            Cancel
          </button>
          <button
            className={`btn primary ${pending?.needs_admin ? "danger" : ""}`}
            onClick={() => pending && run(pending)}
          >
            Run
          </button>
        </div>
      </Modal>
    </div>
  );
}
