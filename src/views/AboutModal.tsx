import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useEffect, useState } from "react";
import Modal from "./Modal";
import { useToast } from "../lib/toast";
import { errMsg } from "../lib/api";
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
  const [pendingUpdate, setPendingUpdate] = useState<Update | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [checked, setChecked] = useState(false);
  const toast = useToast();

  useEffect(() => {
    if (open) {
      getVersion().then(setVersion).catch(() => setVersion(""));
      // Reset the update state each time the dialog opens.
      setPendingUpdate(null);
      setChecked(false);
    }
  }, [open]);

  async function checkUpdates() {
    setChecking(true);
    try {
      const u = await check();
      setPendingUpdate(u);
      setChecked(true);
    } catch (e) {
      toast.err(`Update check failed: ${errMsg(e)}`);
    } finally {
      setChecking(false);
    }
  }

  async function installUpdate() {
    if (!pendingUpdate) return;
    setInstalling(true);
    try {
      await pendingUpdate.downloadAndInstall();
      await relaunch();
    } catch (e) {
      toast.err(`Update failed: ${errMsg(e)}`);
      setInstalling(false);
    }
  }

  return (
    <Modal open={open} onClose={onClose} className="about-modal">
      <div className="about-mark">
        <img src={logo} alt="Trashly" width={72} height={72} />
      </div>
      <h2 className="modal-title">Trashly</h2>
      <p className="muted">{version ? `Version ${version}` : " "}</p>
      <p className="muted small about-desc">
        A Mac cleaner — clean caches, uninstall apps, optimize and monitor your
        system.
      </p>
      <div className="about-actions">
        {pendingUpdate ? (
          <button
            key="install"
            className="btn primary update-btn"
            onClick={installUpdate}
            disabled={installing}
          >
            {installing ? "Installing…" : `Install ${pendingUpdate.version}`}
          </button>
        ) : (
          <button
            key="check"
            className="btn ghost update-btn"
            onClick={checkUpdates}
            disabled={checking}
          >
            {checking ? "Checking…" : checked ? "Up to date ✓" : "Check for updates"}
          </button>
        )}
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
