import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { api, ApiError } from "../api";
import { useProgress } from "../components/Progress";
import { formatBytes } from "../theme";
import type {
  EpisodeCandidate,
  ImportBatch,
  JobSnapshot,
  MatchResponse,
  MediaType,
  PreviewResult,
  ShowCandidate,
} from "../types";

interface LocationState {
  rootId: number;
  mediaType: MediaType;
  selected: string[];
}

interface EditableRow {
  source_path: string;
  source_name: string;
  show_title: string;
  show_year: string;
  season_number: number;
  episode_number: number;
  episode_title: string;
  quality: string;
  provider: string;
  provider_show_id: string;
  candidates: ShowCandidate[];
  metadata_error: string | null;
}

interface RowUi {
  query: string;
  searchResults: ShowCandidate[];
  selectedSearchId: string;
  searchStatus: string;
  episodes: EpisodeCandidate[];
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export default function MatchPage() {
  const location = useLocation();
  const navigate = useNavigate();
  const progress = useProgress();
  const state = location.state as LocationState | null;

  const [response, setResponse] = useState<MatchResponse | null>(null);
  const [rows, setRows] = useState<EditableRow[]>([]);
  const [rowUi, setRowUi] = useState<RowUi[]>([]);
  const [action, setAction] = useState("hardlink");
  const [conflict, setConflict] = useState("skip");
  const [preview, setPreview] = useState<PreviewResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const loaded = useRef(false);

  useEffect(() => {
    if (!state || loaded.current) return;
    loaded.current = true;
    progress.startDelayed("Matching metadata...");
    api
      .postMatch(state.rootId, state.mediaType, state.selected)
      .then((res) => {
        setResponse(res);
        setRows(
          res.rows.map((row) => ({
            source_path: row.source_path,
            source_name: row.source_name,
            show_title: row.show_title,
            show_year: row.show_year != null ? String(row.show_year) : "",
            season_number: row.season_number,
            episode_number: row.episode_number,
            episode_title: row.episode_title,
            quality: row.quality,
            provider: row.provider,
            provider_show_id: row.provider_show_id,
            candidates: row.candidates,
            metadata_error: row.metadata_error,
          })),
        );
        setRowUi(
          res.rows.map((row) => ({
            query: row.show_title,
            searchResults: [],
            selectedSearchId: "",
            searchStatus: "",
            episodes: [],
          })),
        );
      })
      .catch((e) => setError(e instanceof ApiError ? e.message : String(e)))
      .finally(() => progress.hide());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  if (!state) {
    return (
      <section className="panel">
        <h1>Match Queue</h1>
        <p>No selection. Start from the browser.</p>
        <button onClick={() => navigate("/browse")}>Back to Browse</button>
      </section>
    );
  }

  const mediaType = response?.media_type ?? state.mediaType;
  const isFilm = mediaType === "film";

  function updateRow(index: number, partial: Partial<EditableRow>) {
    setRows((prev) => prev.map((row, i) => (i === index ? { ...row, ...partial } : row)));
  }

  function updateUi(index: number, partial: Partial<RowUi>) {
    setRowUi((prev) => prev.map((ui, i) => (i === index ? { ...ui, ...partial } : ui)));
  }

  function applyCandidate(index: number, candidate: ShowCandidate) {
    updateRow(index, {
      provider: candidate.provider,
      provider_show_id: candidate.provider_id,
      show_title: candidate.title,
      show_year: candidate.year != null ? String(candidate.year) : "",
    });
  }

  async function searchCandidates(index: number) {
    const query = rowUi[index].query.trim();
    if (!query) {
      updateUi(index, { searchStatus: "Enter a title to search." });
      return;
    }
    updateUi(index, { searchStatus: "Searching..." });
    progress.startDelayed("Searching metadata...");
    try {
      const { results } = await api.search(mediaType, query);
      updateUi(index, {
        searchResults: results,
        selectedSearchId: results[0]?.provider_id ?? "",
        searchStatus: results.length
          ? `${results.length} match${results.length === 1 ? "" : "es"} found.`
          : "No matches found.",
      });
    } catch (e) {
      updateUi(index, { searchStatus: e instanceof ApiError ? e.message : "Search failed." });
    } finally {
      progress.hide();
    }
  }

  async function applyManualMatch(index: number, applyAll: boolean) {
    const ui = rowUi[index];
    const candidate = ui.searchResults.find((c) => c.provider_id === ui.selectedSearchId);
    if (!candidate) {
      updateUi(index, { searchStatus: "Choose a match first." });
      return;
    }
    const targets = applyAll ? rows.map((_, i) => i) : [index];
    targets.forEach((i) => applyCandidate(i, candidate));
    updateUi(index, {
      searchStatus: applyAll ? `Applied to ${targets.length} rows.` : "Applied to this row.",
    });

    if (!isFilm && candidate.provider_id) {
      await applyEpisodeTitles(targets, candidate.provider_id);
    }
  }

  async function applyEpisodeTitles(indices: number[], providerShowId: string) {
    progress.startDelayed("Loading episodes...");
    try {
      const { results } = await api.episodes(mediaType, providerShowId);
      setRows((prev) =>
        prev.map((row, i) => {
          if (!indices.includes(i)) return row;
          const match = results.find(
            (e) => e.season === row.season_number && e.episode === row.episode_number,
          );
          return match?.title ? { ...row, episode_title: match.title } : row;
        }),
      );
    } catch {
      // ignore episode lookup failures
    } finally {
      progress.hide();
    }
  }

  async function loadEpisodes(index: number) {
    const providerShowId = rows[index].provider_show_id;
    if (!providerShowId) {
      updateUi(index, { searchStatus: "Select a show first to load episodes." });
      return;
    }
    progress.startDelayed("Loading episodes...");
    try {
      const { results } = await api.episodes(mediaType, providerShowId);
      updateUi(index, { episodes: results });
    } catch (e) {
      updateUi(index, { searchStatus: e instanceof ApiError ? e.message : "Could not load episodes." });
    } finally {
      progress.hide();
    }
  }

  function buildBatch(): ImportBatch {
    return {
      media_type: mediaType,
      action,
      conflict_policy: conflict,
      items: rows.map((row) => ({
        source_path: row.source_path,
        show_title: row.show_title,
        show_year: row.show_year.trim() ? Number(row.show_year) : null,
        season_number: isFilm ? 0 : row.season_number,
        episode_number: isFilm ? 0 : row.episode_number,
        episode_title: isFilm ? "Film" : row.episode_title,
        quality: row.quality,
        provider: row.provider || null,
        provider_show_id: row.provider_show_id || null,
      })),
    };
  }

  async function runPreview() {
    progress.startDelayed("Building preview...");
    try {
      const { results } = await api.postPreview(buildBatch());
      setPreview(results);
    } catch (e) {
      alert(e instanceof ApiError ? e.message : "Preview failed.");
    } finally {
      progress.hide();
    }
  }

  function updateProgressFromJob(job: JobSnapshot) {
    const itemPos =
      job.current_item_index && job.total_items
        ? `item ${job.current_item_index} of ${job.total_items}`
        : "";
    const prefix = job.cancel_requested ? "Cancelling" : "Importing";
    progress.update({
      percent: job.percent,
      label: itemPos ? `${prefix} ${itemPos}` : `${prefix}...`,
      currentItem: job.current_item,
      detail: progressDetail(job),
      cancellable: job.state === "running",
      cancelRequested: job.cancel_requested,
    });
  }

  async function runImport() {
    progress.startNow("Starting import...", true);
    let job: JobSnapshot;
    try {
      job = await api.startImportJob(buildBatch());
    } catch (e) {
      progress.hide();
      alert(e instanceof ApiError ? e.message : "Could not start import.");
      return;
    }
    progress.setCancelHandler(() => {
      api.cancelImportJob(job.id).then(updateProgressFromJob).catch(() => undefined);
    });
    updateProgressFromJob(job);
    while (true) {
      let snap: JobSnapshot;
      try {
        snap = await api.getImportJob(job.id);
      } catch {
        progress.hide();
        alert("Could not read import progress.");
        return;
      }
      updateProgressFromJob(snap);
      if (snap.state === "done" || snap.state === "cancelled") {
        progress.hide();
        navigate(`/results/${job.id}`);
        return;
      }
      if (snap.state === "failed") {
        progress.hide();
        alert(snap.error ?? "Import failed.");
        return;
      }
      await sleep(250);
    }
  }

  if (error) {
    return (
      <section className="panel">
        <h1>Match Queue</h1>
        <p className="error">{error}</p>
        <button onClick={() => navigate("/browse")}>Back to Browse</button>
      </section>
    );
  }

  return (
    <section className="panel">
      <h1>Match Queue</h1>
      {response && !response.output_root && (
        <p className="error">No {mediaType} output root configured. Configure it before importing.</p>
      )}
      {rows.length === 0 ? (
        <p>No video files selected.</p>
      ) : (
        <>
          <div className="toolbar sticky-toolbar">
            <label>
              Action
              <select value={action} onChange={(e) => setAction(e.target.value)}>
                <option value="hardlink">Hardlink</option>
                <option value="copy">Copy</option>
                <option value="test">Test</option>
              </select>
            </label>
            <label>
              Conflict
              <select value={conflict} onChange={(e) => setConflict(e.target.value)}>
                <option value="skip">Skip</option>
                <option value="replace">Replace</option>
                <option value="index">Keep Both</option>
                <option value="fail">Fail</option>
              </select>
            </label>
            <button type="button" onClick={runPreview}>
              Preview
            </button>
            <button type="button" onClick={runImport} disabled={!response?.output_root}>
              Import
            </button>
          </div>

          {preview && (
            <div className="panel inner-panel">
              <h2>Preview</h2>
              <table className="result-table">
                <thead>
                  <tr>
                    <th>Source</th>
                    <th>Destination</th>
                    <th>Status</th>
                  </tr>
                </thead>
                <tbody>
                  {preview.map((result, i) => (
                    <tr key={i} className={`state-row state-${result.result}`}>
                      <td className="path-cell">{result.source_path}</td>
                      <td className="path-cell">{result.final_path}</td>
                      <td>
                        <span className={`state-badge state-${result.result}`}>{result.result}</span>
                        {result.error ? ` ${result.error}` : ""}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {rows.map((row, index) => (
            <fieldset className="match-row" key={row.source_path}>
              <legend>{row.source_name}</legend>
              {row.metadata_error && (
                <p className="error">Metadata lookup failed: {row.metadata_error}</p>
              )}
              <div className="grid">
                <label>
                  {isFilm ? "Film title" : "Show title"}
                  <input
                    value={row.show_title}
                    onChange={(e) => updateRow(index, { show_title: e.target.value })}
                  />
                </label>
                <label>
                  Year
                  <input
                    value={row.show_year}
                    onChange={(e) => updateRow(index, { show_year: e.target.value })}
                  />
                </label>
                {!isFilm && (
                  <>
                    <label>
                      Season
                      <input
                        type="number"
                        min={0}
                        value={row.season_number}
                        onChange={(e) => updateRow(index, { season_number: Number(e.target.value) })}
                      />
                    </label>
                    <label>
                      Episode
                      <input
                        type="number"
                        min={0}
                        value={row.episode_number}
                        onChange={(e) => updateRow(index, { episode_number: Number(e.target.value) })}
                      />
                    </label>
                    <label>
                      Episode title
                      <input
                        value={row.episode_title}
                        onChange={(e) => updateRow(index, { episode_title: e.target.value })}
                      />
                    </label>
                  </>
                )}
                <label>
                  Quality
                  <input
                    value={row.quality}
                    onChange={(e) => updateRow(index, { quality: e.target.value })}
                  />
                </label>
              </div>

              <div className="manual-match">
                <label>
                  Search metadata
                  <input
                    type="search"
                    value={rowUi[index]?.query ?? ""}
                    onChange={(e) => updateUi(index, { query: e.target.value })}
                  />
                </label>
                <button className="secondary-button" type="button" onClick={() => searchCandidates(index)}>
                  Search
                </button>
                {rowUi[index]?.searchResults.length > 0 && (
                  <>
                    <select
                      value={rowUi[index].selectedSearchId}
                      onChange={(e) => updateUi(index, { selectedSearchId: e.target.value })}
                    >
                      {rowUi[index].searchResults.map((candidate) => (
                        <option key={candidate.provider_id} value={candidate.provider_id}>
                          {candidate.title}
                          {candidate.year ? ` (${candidate.year})` : ""} - {candidate.provider}
                        </option>
                      ))}
                    </select>
                    <button className="secondary-button" type="button" onClick={() => applyManualMatch(index, false)}>
                      Apply to row
                    </button>
                    <button className="secondary-button" type="button" onClick={() => applyManualMatch(index, true)}>
                      Apply to all rows
                    </button>
                  </>
                )}
                <span className="muted">{rowUi[index]?.searchStatus}</span>
              </div>

              {!isFilm && (
                <div className="manual-match">
                  <button className="secondary-button" type="button" onClick={() => loadEpisodes(index)}>
                    Load episodes
                  </button>
                  {rowUi[index]?.episodes.length > 0 && (
                    <select
                      defaultValue=""
                      onChange={(e) => {
                        const ep = rowUi[index].episodes[Number(e.target.value)];
                        if (ep) {
                          updateRow(index, {
                            season_number: ep.season,
                            episode_number: ep.episode,
                            episode_title: ep.title,
                          });
                        }
                      }}
                    >
                      <option value="" disabled>
                        Pick an episode...
                      </option>
                      {rowUi[index].episodes.map((ep, i) => (
                        <option key={i} value={i}>
                          S{String(ep.season).padStart(2, "0")}E{String(ep.episode).padStart(2, "0")} - {ep.title}
                        </option>
                      ))}
                    </select>
                  )}
                </div>
              )}

              {row.candidates.length > 0 && (
                <details>
                  <summary>Metadata candidates</summary>
                  <label>
                    Selected show
                    <select
                      onChange={(e) => {
                        const candidate = row.candidates[Number(e.target.value)];
                        if (candidate) applyCandidate(index, candidate);
                      }}
                    >
                      {row.candidates.map((candidate, i) => (
                        <option key={candidate.provider_id} value={i}>
                          {candidate.title}
                          {candidate.year ? ` (${candidate.year})` : ""} - {candidate.provider}
                        </option>
                      ))}
                    </select>
                  </label>
                  <ul className="muted">
                    {row.candidates.map((candidate, i) => (
                      <li key={i}>
                        {candidate.title}
                        {candidate.year ? ` (${candidate.year})` : ""} - {candidate.summary}
                      </li>
                    ))}
                  </ul>
                </details>
              )}
              <p className="muted path-line">{row.source_path}</p>
            </fieldset>
          ))}
        </>
      )}
    </section>
  );
}

function progressDetail(job: JobSnapshot): string {
  const parts: string[] = [];
  if (job.cancel_requested) parts.push("Cancelling after the current copy stops");
  if (typeof job.completed_items === "number" && typeof job.total_items === "number") {
    parts.push(`${job.completed_items}/${job.total_items} items done`);
  }
  if (job.current_action === "copy" && job.current_item_total > 0) {
    parts.push(
      `${job.current_item_percent}% of current file (${formatBytes(job.current_item_bytes)} / ${formatBytes(
        job.current_item_total,
      )})`,
    );
  } else if (job.current_action) {
    parts.push(`${job.current_action} in progress`);
  }
  if (job.current_action === "copy" && job.total > 0) {
    parts.push(`${formatBytes(job.completed)} / ${formatBytes(job.total)} total`);
  }
  return parts.join(" - ");
}
