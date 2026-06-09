import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

type ToastKind = "ok" | "err" | "info";
type ToastItem = { id: number; kind: ToastKind; text: string; leaving?: boolean };

const EXIT_MS = 180;

interface ToastApi {
  push: (text: string, kind?: ToastKind) => void;
  ok: (text: string) => void;
  err: (text: string) => void;
}

const ToastCtx = createContext<ToastApi | null>(null);

let nextId = 1;

/** App-wide toast stack: notifications pile up bottom-right, each auto-dismisses
 *  (and can be clicked away). */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);

  const remove = useCallback((id: number) => {
    // Play the exit animation first, then drop the item from the DOM.
    setItems((prev) =>
      prev.map((t) => (t.id === id ? { ...t, leaving: true } : t)),
    );
    window.setTimeout(
      () => setItems((prev) => prev.filter((t) => t.id !== id)),
      EXIT_MS,
    );
  }, []);

  const push = useCallback(
    (text: string, kind: ToastKind = "info") => {
      const id = nextId++;
      setItems((prev) => [...prev, { id, kind, text }]);
      window.setTimeout(() => remove(id), 5000);
    },
    [remove],
  );

  // Memoised so `useToast()` returns a stable reference — consumers can safely
  // list it in effect deps without re-running on every render.
  const api = useMemo<ToastApi>(
    () => ({
      push,
      ok: (text) => push(text, "ok"),
      err: (text) => push(text, "err"),
    }),
    [push],
  );

  return (
    <ToastCtx.Provider value={api}>
      {children}
      <div className="toast-stack">
        {items.map((t) => (
          <div
            key={t.id}
            className={`toast toast-${t.kind}${t.leaving ? " leaving" : ""}`}
            onClick={() => remove(t.id)}
          >
            {t.kind === "ok" ? "✓ " : t.kind === "err" ? "✕ " : ""}
            {t.text}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export function useToast(): ToastApi {
  const ctx = useContext(ToastCtx);
  if (!ctx) throw new Error("useToast must be used within ToastProvider");
  return ctx;
}
