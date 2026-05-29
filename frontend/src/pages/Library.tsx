import { useCallback, useEffect, useState } from "react";
import { api, ApiError } from "../api";
import { useProgress } from "../components/Progress";
import type { LibraryFile } from "../types";

export default function LibraryPage() {
  const progress = useProgress();
  const [files, setFiles] = useState<LibraryFile[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .getLibrary()
      .then(({ files }) => setFiles(files))
      .catch((e) => setError(e instanceof ApiError ? e.message : String(e)));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  async function rescan() {
    progress.startDelayed("Rescanning output folders...");
    try {
      await api.rescanLibrary();
      load();
    } catch (e) {
      alert(e instanceof ApiError ? e.message : "Rescan failed.");
    } finally {
      progress.hide();
    }
  }

  return (
    <section className="panel">
      <h1>Library</h1>
      {error && <p className="error">{error}</p>}
      <div className="toolbar">
        <button type="button" onClick={rescan}>
          Rescan Output Folders
        </button>
      </div>
      <table>
        <thead>
          <tr>
            <th>Type</th>
            <th>Path</th>
            <th>Present</th>
            <th>Size</th>
          </tr>
        </thead>
        <tbody>
          {files.map((file) => (
            <tr key={file.output_path}>
              <td>{file.media_type}</td>
              <td className="path-cell">{file.output_path}</td>
              <td>{file.present ? "yes" : "missing"}</td>
              <td>{file.size_human}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
