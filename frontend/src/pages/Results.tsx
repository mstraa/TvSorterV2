import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api, ApiError } from "../api";
import type { PreviewResult } from "../types";

export default function ResultsPage() {
  const { jobId } = useParams<{ jobId: string }>();
  const [results, setResults] = useState<PreviewResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [enabledStates, setEnabledStates] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (!jobId) return;
    api
      .getImportJobResults(jobId)
      .then(({ results }) => {
        setResults(results);
        setEnabledStates(new Set(results.map((r) => r.result)));
      })
      .catch((e) => setError(e instanceof ApiError ? e.message : String(e)));
  }, [jobId]);

  const states = useMemo(() => {
    if (!results) return [];
    return [...new Set(results.map((r) => r.result))];
  }, [results]);

  const hasErrors = useMemo(() => (results ?? []).some((r) => r.error), [results]);

  function toggleState(state: string) {
    setEnabledStates((prev) => {
      const next = new Set(prev);
      if (next.has(state)) next.delete(state);
      else next.add(state);
      return next;
    });
  }

  if (error) {
    return (
      <section className="panel">
        <h1>Import Results</h1>
        <p className="error">{error}</p>
        <Link to="/browse">Back to Browse</Link>
      </section>
    );
  }

  if (!results) {
    return (
      <section className="panel">
        <h1>Import Results</h1>
        <p className="muted">Loading...</p>
      </section>
    );
  }

  return (
    <section className="panel">
      <h1>Import Results</h1>
      <div className="state-filter">
        {states.map((state) => (
          <label key={state} className={`state-toggle state-${state}`}>
            <input
              type="checkbox"
              checked={enabledStates.has(state)}
              onChange={() => toggleState(state)}
            />
            {state}
          </label>
        ))}
      </div>
      <table className={`result-table ${hasErrors ? "has-errors" : ""}`}>
        <thead>
          <tr>
            <th>Source</th>
            <th>Destination</th>
            <th>State</th>
            {hasErrors && <th>Error</th>}
          </tr>
        </thead>
        <tbody>
          {results
            .filter((result) => enabledStates.has(result.result))
            .map((result, i) => (
              <tr key={i} className={`state-row state-${result.result}`}>
                <td className="path-cell">{result.source_path}</td>
                <td className="path-cell">{result.final_path}</td>
                <td>
                  <span className={`state-badge state-${result.result}`}>{result.result}</span>
                </td>
                {hasErrors && <td className="error-cell">{result.error ?? ""}</td>}
              </tr>
            ))}
        </tbody>
      </table>
      <Link className="button" to="/browse">
        Back to Browse
      </Link>
    </section>
  );
}
