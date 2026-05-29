import { useCallback, useEffect, useState } from "react";
import { api, ApiError } from "../api";
import type { FolderEntry } from "../types";

interface Props {
  initialPath: string;
  onChoose: (path: string) => void;
  onClose: () => void;
}

export default function FolderPicker({ initialPath, onChoose, onClose }: Props) {
  const [path, setPath] = useState(initialPath || "/mnt");
  const [parent, setParent] = useState<string | null>(null);
  const [folders, setFolders] = useState<FolderEntry[]>([]);
  const [roots, setRoots] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback((target: string) => {
    setError(null);
    api
      .getFolders(target || "/")
      .then((response) => {
        setPath(response.path);
        setParent(response.parent);
        setFolders(response.folders);
        setRoots(response.roots);
      })
      .catch((e) => setError(e instanceof ApiError ? e.message : "Could not open folder"));
  }, []);

  useEffect(() => {
    load(initialPath || "/mnt");
  }, [initialPath, load]);

  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div className="folder-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="folder-dialog-header">
          <h2>Select Folder</h2>
          <button className="icon-button" type="button" onClick={onClose} aria-label="Close">
            ×
          </button>
        </div>
        <div className="folder-dialog-body">
          <label>
            Current path
            <input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  load(path);
                }
              }}
            />
          </label>
          <div className="toolbar compact-toolbar">
            <button className="secondary-button" type="button" onClick={() => load(path)}>
              Go
            </button>
            <button
              className="secondary-button"
              type="button"
              disabled={!parent}
              onClick={() => parent && load(parent)}
            >
              Up
            </button>
            <button type="button" onClick={() => onChoose(path)}>
              Choose This Folder
            </button>
          </div>
          {roots.length > 0 && (
            <div className="folder-roots">
              {roots.map((root) => (
                <button key={root} className="folder-root-button" type="button" onClick={() => load(root)}>
                  {root}
                </button>
              ))}
            </div>
          )}
          {error && <p className="error">{error}</p>}
          <div className="folder-list">
            {folders.length === 0 ? (
              <p className="muted">No folders found.</p>
            ) : (
              folders.map((folder) => (
                <button
                  key={folder.path}
                  className="folder-entry"
                  type="button"
                  onClick={() => load(folder.path)}
                >
                  <span>{folder.name}</span>
                  <span className="muted">{folder.path}</span>
                </button>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
