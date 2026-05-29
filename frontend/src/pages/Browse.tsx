import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { api, ApiError } from "../api";
import { useProgress } from "../components/Progress";
import Select from "../components/Select";
import type { BrowseResponse, MediaType } from "../types";

const STATUS_FILTERS = [
  { value: "none", label: "Only no status" },
  { value: "all", label: "All statuses" },
  { value: "imported", label: "Imported" },
  { value: "failed", label: "Failed" },
  { value: "skipped", label: "Skipped" },
  { value: "cancelled", label: "Cancelled" },
  { value: "preview", label: "Preview" },
  { value: "conflict", label: "Conflict" },
  { value: "mixed", label: "Mixed" },
];

const MANUAL_STATUSES = ["imported", "failed", "skipped", "preview", "conflict"];

export default function BrowsePage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const navigate = useNavigate();
  const progress = useProgress();

  const rootIdParam = searchParams.get("root_id");
  const rootId = rootIdParam ? Number(rootIdParam) : null;
  const path = searchParams.get("path") ?? "";

  const [data, setData] = useState<BrowseResponse | null>(null);
  const [mediaType, setMediaType] = useState<MediaType>("tv");
  const [statusFilter, setStatusFilter] = useState("none");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [selectedStatus, setSelectedStatus] = useState("auto");
  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(() => {
    progress.startDelayed("Loading folder...");
    api
      .getBrowse(rootId, path)
      .then((response) => {
        setData(response);
        setLoadError(null);
      })
      .catch((error) => setLoadError(error instanceof ApiError ? error.message : String(error)))
      .finally(() => progress.hide());
  }, [rootId, path, progress]);

  useEffect(() => {
    setSelected(new Set());
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rootId, path]);

  const visibleEntries = useMemo(() => {
    if (!data) return [];
    return data.entries.filter(
      (entry) => statusFilter === "all" || entry.status_key === statusFilter,
    );
  }, [data, statusFilter]);

  function toggle(relativePath: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(relativePath)) next.delete(relativePath);
      else next.add(relativePath);
      return next;
    });
  }

  function openFolder(relativePath: string) {
    const params = new URLSearchParams();
    if (data?.active_root) params.set("root_id", String(data.active_root.id));
    params.set("path", relativePath);
    setSearchParams(params);
  }

  function changeRoot(newRootId: number) {
    setSearchParams({ root_id: String(newRootId), path: "" });
  }

  function matchSelected() {
    if (!data?.active_root) return;
    if (selected.size === 0) {
      alert("Select one or more files or folders first.");
      return;
    }
    navigate("/match", {
      state: {
        rootId: data.active_root.id,
        mediaType,
        selected: [...selected],
      },
    });
  }

  async function applyStatus() {
    if (!data?.active_root) return;
    if (selected.size === 0) {
      alert("Select one or more files or folders first.");
      return;
    }
    progress.startDelayed("Updating status...");
    try {
      await api.setSourceStatus({
        root_id: data.active_root.id,
        selected: [...selected],
        status: selectedStatus,
      });
      load();
    } catch (error) {
      alert(error instanceof ApiError ? error.message : "Could not update status.");
    } finally {
      progress.hide();
    }
  }

  if (!data) {
    return (
      <section className="panel">
        <h1>Input Browser</h1>
        {loadError && <p className="error">{loadError}</p>}
      </section>
    );
  }

  if (data.roots.length === 0) {
    return (
      <section className="panel">
        <h1>Input Browser</h1>
        <p>No input roots are configured.</p>
        <button onClick={() => navigate("/settings")}>Open Settings</button>
      </section>
    );
  }

  return (
    <section className="panel">
      <h1>Input Browser</h1>
      {data.error && <p className="error">{data.error}</p>}

      <div className="sticky-controls">
        <div className="toolbar status-toolbar">
          <label>
            Set selected status
            <Select
              value={selectedStatus}
              onChange={setSelectedStatus}
              options={[
                { value: "auto", label: "Auto" },
                { value: "none", label: "No status" },
                ...MANUAL_STATUSES.map((status) => ({ value: status, label: status })),
              ]}
            />
          </label>
          <button className="secondary-button" type="button" onClick={applyStatus}>
            Apply Status
          </button>
        </div>

        <div className="toolbar browse-toolbar">
          <label>
            Input root
            <Select
              value={data.active_root?.id != null ? String(data.active_root.id) : ""}
              onChange={(v) => changeRoot(Number(v))}
              options={data.roots.map((root) => ({ value: String(root.id), label: root.path }))}
            />
          </label>
          <label>
            Type
            <Select
              value={mediaType}
              onChange={(v) => setMediaType(v as MediaType)}
              options={[
                { value: "tv", label: "TV" },
                { value: "anime", label: "Anime" },
                { value: "film", label: "Film" },
              ]}
            />
          </label>
          <button type="button" onClick={matchSelected}>
            Match Selected
          </button>
          <label className="browse-filter">
            Show
            <Select
              value={statusFilter}
              onChange={setStatusFilter}
              options={STATUS_FILTERS.map((filter) => ({
                value: filter.value,
                label: filter.label,
              }))}
            />
          </label>
        </div>

        <div className="pathbar">
          <span className="muted">Current path: /{data.current_path}</span>
          {data.current_path && (
            <button className="link-button" type="button" onClick={() => openFolder(data.parent_path)}>
              Up one folder
            </button>
          )}
        </div>
      </div>

      <table className="browse-table">
        <colgroup>
          <col className="select-column" />
          <col className="name-column" />
          <col className="status-column" />
          <col className="type-column" />
          <col className="size-column" />
        </colgroup>
        <thead>
          <tr>
            <th></th>
            <th>Name</th>
            <th>Status</th>
            <th>Type</th>
            <th>Size</th>
          </tr>
        </thead>
        <tbody>
          {visibleEntries.map((entry) => (
            <tr
              key={entry.relative_path}
              className={`browse-row ${entry.status ? `state-row state-${entry.status}` : ""} ${
                selected.has(entry.relative_path) ? "selected-row" : ""
              }`}
              onClick={(e) => {
                const target = e.target as HTMLElement;
                if (target.closest("a, button, input, select, label")) return;
                toggle(entry.relative_path);
              }}
            >
              <td>
                <input
                  type="checkbox"
                  checked={selected.has(entry.relative_path)}
                  onChange={() => toggle(entry.relative_path)}
                />
              </td>
              <td className="browse-name-cell">
                {entry.is_dir ? (
                  <button
                    className="link-button"
                    type="button"
                    onClick={() => openFolder(entry.relative_path)}
                  >
                    {entry.name}
                  </button>
                ) : (
                  entry.name
                )}
              </td>
              <td className="nowrap-cell">
                {entry.status && <span className={`state-badge state-${entry.status}`}>{entry.status}</span>}
              </td>
              <td className="nowrap-cell">
                {entry.is_dir ? "folder" : entry.is_video ? "video" : "file"}
              </td>
              <td className="nowrap-cell">{entry.size_human}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
