import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import Modal from "./Modal";
import logo from "../assets/logo.svg";

const GITHUB_URL = "https://github.com/AppsGanin/Trashly";

export default function AboutModal({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [version, setVersion] = useState("");

  useEffect(() => {
    if (open) {
      getVersion()
        .then(setVersion)
        .catch(() => setVersion(""));
    }
  }, [open]);

  return (
    <Modal open={open} onClose={onClose} className="about-modal">
      <div className="about-mark">
        <img src={logo} alt="Trashly" width={72} height={72} />
      </div>
      <h2 className="modal-title">Trashly</h2>
      <p className="muted">{version ? `Version ${version}` : " "}</p>
      <p className="muted small about-desc">
        A Mac cleaner — clean caches, uninstall apps, optimize and monitor your
        system.
      </p>
      <div className="about-actions">
        <button className="btn primary" onClick={() => openUrl(GITHUB_URL)}>
          View on GitHub ↗
        </button>
        <button className="btn ghost" onClick={onClose}>
          Close
        </button>
      </div>
    </Modal>
  );
}
