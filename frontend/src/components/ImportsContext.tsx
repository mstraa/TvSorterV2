import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { api } from "../api";
import type { JobSnapshot } from "../types";

interface ImportsContextValue {
  jobs: JobSnapshot[];
  activeCount: number;
  refresh: () => void;
}

const ImportsContext = createContext<ImportsContextValue>({
  jobs: [],
  activeCount: 0,
  refresh: () => undefined,
});

const ACTIVE_INTERVAL = 1000;
const IDLE_INTERVAL = 4000;

export function ImportsProvider({ children }: { children: ReactNode }) {
  const [jobs, setJobs] = useState<JobSnapshot[]>([]);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mounted = useRef(true);

  const poll = useCallback(async () => {
    try {
      const { jobs: next } = await api.listImportJobs();
      if (mounted.current) setJobs(next);
      return next.some((job) => job.active);
    } catch {
      return false;
    }
  }, []);

  const schedule = useCallback(
    (delay: number) => {
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(async () => {
        const anyActive = await poll();
        schedule(anyActive ? ACTIVE_INTERVAL : IDLE_INTERVAL);
      }, delay);
    },
    [poll],
  );

  const refresh = useCallback(() => {
    void poll().then((anyActive) => schedule(anyActive ? ACTIVE_INTERVAL : IDLE_INTERVAL));
  }, [poll, schedule]);

  useEffect(() => {
    mounted.current = true;
    refresh();
    return () => {
      mounted.current = false;
      if (timer.current) clearTimeout(timer.current);
    };
  }, [refresh]);

  const activeCount = jobs.filter((job) => job.active).length;

  return (
    <ImportsContext.Provider value={{ jobs, activeCount, refresh }}>
      {children}
    </ImportsContext.Provider>
  );
}

export function useImports() {
  return useContext(ImportsContext);
}
