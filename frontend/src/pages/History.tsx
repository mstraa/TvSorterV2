import { useEffect, useState } from "react";
import { api, ApiError } from "../api";
import type { ImportHistoryRow } from "../types";

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

export default function HistoryPage() {
  const [imports, setImports] = useState<ImportHistoryRow[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .getHistory()
      .then(({ imports }) => setImports(imports))
      .catch((e) => setError(e instanceof ApiError ? e.message : String(e)));
  }, []);

  return (
    <section className="panel">
      <h1>History</h1>
      {error && <p className="error">{error}</p>}
      <table>
        <thead>
          <tr>
            <th>When</th>
            <th>Type</th>
            <th>Show</th>
            <th>Episode</th>
            <th>Action</th>
            <th>Result</th>
          </tr>
        </thead>
        <tbody>
          {imports.map((item) => (
            <tr key={item.id} className={`state-row state-${item.result}`}>
              <td className="nowrap-cell">{item.imported_at}</td>
              <td>{item.media_type}</td>
              <td>
                {item.show_title}
                {item.show_year ? ` (${item.show_year})` : ""}
              </td>
              <td>
                {item.media_type === "film"
                  ? "Film"
                  : `S${pad(item.season_number)}E${pad(item.episode_number)} - ${item.episode_title}`}
              </td>
              <td>{item.action}</td>
              <td>
                {item.result}
                {item.error ? `: ${item.error}` : ""}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
