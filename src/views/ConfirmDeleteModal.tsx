import { Flame, Trash2 } from "lucide-react";
import Modal from "./Modal";

// Confirmation modal shown when the user triggers a deletion. Lets them pick
// between moving to the Trash (recoverable) or deleting permanently.
export default function ConfirmDeleteModal({
  open,
  title,
  message,
  busy,
  directOnly,
  onTrash,
  onDirect,
  onCancel,
}: {
  open: boolean;
  title: string;
  message?: string;
  busy?: boolean;
  /** Hide the "Move to Trash" option (e.g. items already in the Trash). */
  directOnly?: boolean;
  onTrash: () => void;
  onDirect: () => void;
  onCancel: () => void;
}) {
  return (
    <Modal open={open} onClose={onCancel} closeOnOverlay={!busy}>
      <h2 className="modal-title">{title}</h2>
      {message && <p className="modal-msg">{message}</p>}
      <div className="modal-choices">
        {!directOnly && (
          <button
            className="modal-choice trash"
            onClick={onTrash}
            disabled={busy}
          >
            <Trash2 className="modal-choice-icon" size={22} />
            <span className="modal-choice-text">
              <strong>Move to Trash</strong>
              <small>Recoverable — restore from Trash later</small>
            </span>
          </button>
        )}
        <button
          className="modal-choice direct"
          onClick={onDirect}
          disabled={busy}
        >
          <Flame className="modal-choice-icon" size={22} />
          <span className="modal-choice-text">
            <strong>Delete permanently</strong>
            <small>Frees space now — cannot be undone</small>
          </span>
        </button>
      </div>
      <div className="modal-footer">
        <button className="btn ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        {busy && <span className="muted small">Working…</span>}
      </div>
    </Modal>
  );
}
