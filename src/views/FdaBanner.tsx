import { useEffect, useState } from "react";
import { Lock } from "lucide-react";
import { hasFullDiskAccess, openFullDiskAccess } from "../lib/api";

/** A dismissable-by-granting nudge: shown only when Trashly lacks Full Disk
 *  Access, which several modules need to see everything. Re-checks on window
 *  focus, so it disappears once the user grants it and returns to the app. */
export default function FdaBanner({ note }: { note: string }) {
  const [fda, setFda] = useState(true);

  useEffect(() => {
    const recheck = () => hasFullDiskAccess().then(setFda).catch(() => {});
    recheck();
    window.addEventListener("focus", recheck);
    return () => window.removeEventListener("focus", recheck);
  }, []);

  if (fda) return null;
  return (
    <div className="fda-banner">
      <Lock size={15} />
      <span>{note}</span>
      <button className="btn ghost sm" onClick={openFullDiskAccess}>
        Open Settings
      </button>
    </div>
  );
}
