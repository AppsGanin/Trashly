import type { ReactNode } from "react";

/** Base modal: dimmed overlay + centered dialog. Clicking the overlay closes
 *  it (unless disabled); clicks inside the dialog don't propagate. */
export default function Modal({
  open,
  onClose,
  className,
  closeOnOverlay = true,
  children,
}: {
  open: boolean;
  onClose: () => void;
  className?: string;
  closeOnOverlay?: boolean;
  children: ReactNode;
}) {
  if (!open) return null;
  return (
    <div
      className="modal-overlay"
      onClick={closeOnOverlay ? onClose : undefined}
    >
      <div
        className={`modal ${className ?? ""}`.trim()}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
