import { createContext, useCallback, useContext, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";

interface ProgressState {
  visible: boolean;
  label: string;
  percent: number | null;
  currentItem: string;
  detail: string;
  cancellable: boolean;
  cancelRequested: boolean;
}

interface ProgressUpdate {
  label?: string;
  percent?: number | null;
  currentItem?: string;
  detail?: string;
  cancellable?: boolean;
  cancelRequested?: boolean;
}

interface ProgressContextValue {
  /** Show the overlay after a 2s delay (for operations that may be slow). */
  startDelayed: (label: string, determinate?: boolean) => void;
  /** Show the overlay immediately. */
  startNow: (label: string, determinate?: boolean) => void;
  update: (partial: ProgressUpdate) => void;
  hide: () => void;
  setCancelHandler: (handler: (() => void) | null) => void;
}

const ProgressContext = createContext<ProgressContextValue | null>(null);

const INITIAL: ProgressState = {
  visible: false,
  label: "Working...",
  percent: null,
  currentItem: "",
  detail: "",
  cancellable: false,
  cancelRequested: false,
};

export function ProgressProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<ProgressState>(INITIAL);
  const timerRef = useRef<number | null>(null);
  const cancelHandlerRef = useRef<(() => void) | null>(null);

  const clearTimer = useCallback(() => {
    if (timerRef.current != null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const startDelayed = useCallback(
    (label: string, determinate = false) => {
      clearTimer();
      setState({ ...INITIAL, label, percent: determinate ? 0 : null });
      timerRef.current = window.setTimeout(() => {
        setState((prev) => ({ ...prev, visible: true }));
      }, 2000);
    },
    [clearTimer],
  );

  const startNow = useCallback(
    (label: string, determinate = false) => {
      clearTimer();
      setState({ ...INITIAL, label, percent: determinate ? 0 : null, visible: true });
    },
    [clearTimer],
  );

  const update = useCallback((partial: ProgressUpdate) => {
    setState((prev) => ({ ...prev, ...partial }));
  }, []);

  const hide = useCallback(() => {
    clearTimer();
    cancelHandlerRef.current = null;
    setState(INITIAL);
  }, [clearTimer]);

  const setCancelHandler = useCallback((handler: (() => void) | null) => {
    cancelHandlerRef.current = handler;
  }, []);

  const value = useMemo(
    () => ({ startDelayed, startNow, update, hide, setCancelHandler }),
    [startDelayed, startNow, update, hide, setCancelHandler],
  );

  const onCancel = () => {
    cancelHandlerRef.current?.();
  };

  return (
    <ProgressContext.Provider value={value}>
      {children}
      {state.visible && (
        <div className="progress-overlay">
          <div className="progress-panel" role="status" aria-live="polite">
            <div className="progress-label">{state.label}</div>
            {state.currentItem && <div className="progress-item">{state.currentItem}</div>}
            <div className="progress-track" aria-hidden="true">
              <div
                className={`progress-bar ${state.percent == null ? "indeterminate" : ""}`}
                style={state.percent == null ? undefined : { width: `${state.percent}%` }}
              />
            </div>
            {state.percent != null && <div className="progress-percent">{state.percent}%</div>}
            {state.detail && <div className="progress-detail">{state.detail}</div>}
            {state.cancellable && (
              <button
                className="secondary-button"
                type="button"
                disabled={state.cancelRequested}
                onClick={onCancel}
              >
                {state.cancelRequested ? "Cancelling..." : "Cancel import"}
              </button>
            )}
          </div>
        </div>
      )}
    </ProgressContext.Provider>
  );
}

export function useProgress(): ProgressContextValue {
  const ctx = useContext(ProgressContext);
  if (!ctx) {
    throw new Error("useProgress must be used within ProgressProvider");
  }
  return ctx;
}
