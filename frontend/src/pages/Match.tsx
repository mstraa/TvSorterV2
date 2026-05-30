import { Fragment, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { api, ApiError } from "../api";
import { useProgress } from "../components/Progress";
import { useImports } from "../components/ImportsContext";
import Select from "../components/Select";
import type {
  EpisodeCandidate,
  ImportBatch,
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

interface EpisodeUi {
  source_path: string;
  source_name: string;
  season_number: number;
  episode_number: number;
  episode_title: string;
  quality: string;
  expanded: boolean;
}

interface GroupUi {
  group_key: string;
  group_name: string;
  show_title: string;
  show_year: string;
  provider: string;
  provider_show_id: string;
  candidates: ShowCandidate[];
  metadata_error: string | null;
  episodes: EpisodeUi[];
  // manual-search UI state
  query: string;
  searchResults: ShowCandidate[];
  selectedSearchId: string;
  searchStatus: string;
  episodeOptions: EpisodeCandidate[];
}

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

function matchEpisodeTitle(
  episodes: EpisodeCandidate[],
  season: number,
  episode: number,
  isAnime: boolean,
): string | null {
  const exact = episodes.find((e) => e.season === season && e.episode === episode);
  if (exact) return exact.title;
  if (isAnime) {
    const byNumber = episodes.find((e) => e.episode === episode);
    if (byNumber) return byNumber.title;
  }
  return null;
}

export default function MatchPage() {
  const location = useLocation();
  const navigate = useNavigate();
  const progress = useProgress();
  const imports = useImports();
  const state = location.state as LocationState | null;

  const [response, setResponse] = useState<MatchResponse | null>(null);
  const [groups, setGroups] = useState<GroupUi[]>([]);
  const [action, setAction] = useState("copy");
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
        setGroups(
          res.groups.map((group) => ({
            group_key: group.group_key,
            group_name: group.group_name,
            show_title: group.show_title,
            show_year: group.show_year != null ? String(group.show_year) : "",
            provider: group.provider,
            provider_show_id: group.provider_show_id,
            candidates: group.candidates,
            metadata_error: group.metadata_error,
            episodes: group.episodes.map((ep) => ({
              source_path: ep.source_path,
              source_name: ep.source_name,
              season_number: ep.season_number,
              episode_number: ep.episode_number,
              episode_title: ep.episode_title,
              quality: ep.quality,
              expanded: false,
            })),
            query: group.show_title,
            searchResults: [],
            selectedSearchId: "",
            searchStatus: "",
            episodeOptions: [],
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
  const isAnime = mediaType === "anime";
  const episodeCount = groups.reduce((sum, group) => sum + group.episodes.length, 0);

  function updateGroup(gi: number, partial: Partial<GroupUi>) {
    setGroups((prev) => prev.map((group, i) => (i === gi ? { ...group, ...partial } : group)));
  }

  function updateEpisode(gi: number, ei: number, partial: Partial<EpisodeUi>) {
    setGroups((prev) =>
      prev.map((group, i) =>
        i === gi
          ? {
              ...group,
              episodes: group.episodes.map((ep, j) => (j === ei ? { ...ep, ...partial } : ep)),
            }
          : group,
      ),
    );
  }

  function toggleEpisode(gi: number, ei: number) {
    setGroups((prev) =>
      prev.map((group, i) =>
        i === gi
          ? {
              ...group,
              episodes: group.episodes.map((ep, j) =>
                j === ei ? { ...ep, expanded: !ep.expanded } : ep,
              ),
            }
          : group,
      ),
    );
  }

  async function searchGroup(gi: number) {
    const query = groups[gi].query.trim();
    if (!query) {
      updateGroup(gi, { searchStatus: "Enter a title to search." });
      return;
    }
    updateGroup(gi, { searchStatus: "Searching..." });
    progress.startDelayed("Searching metadata...");
    try {
      const { results } = await api.search(mediaType, query);
      updateGroup(gi, {
        searchResults: results,
        selectedSearchId: results[0]?.provider_id ?? "",
        searchStatus: results.length
          ? `${results.length} match${results.length === 1 ? "" : "es"} found.`
          : "No matches found.",
      });
    } catch (e) {
      updateGroup(gi, { searchStatus: e instanceof ApiError ? e.message : "Search failed." });
    } finally {
      progress.hide();
    }
  }

  // Apply a chosen show to the whole group, then refresh episode titles from the
  // provider so the compact list reflects the new match.
  async function selectShowForGroup(gi: number, candidate: ShowCandidate) {
    updateGroup(gi, {
      provider: candidate.provider,
      provider_show_id: candidate.provider_id,
      show_title: candidate.title,
      show_year: candidate.year != null ? String(candidate.year) : "",
      searchStatus: `Applied "${candidate.title}".`,
    });

    if (isFilm || !candidate.provider_id) return;

    progress.startDelayed("Loading episodes...");
    try {
      const { results } = await api.episodes(mediaType, candidate.provider_id);
      setGroups((prev) =>
        prev.map((group, i) => {
          if (i !== gi) return group;
          return {
            ...group,
            episodeOptions: results,
            episodes: group.episodes.map((ep) => {
              const title = matchEpisodeTitle(
                results,
                ep.season_number,
                ep.episode_number,
                isAnime,
              );
              return title ? { ...ep, episode_title: title } : ep;
            }),
          };
        }),
      );
    } catch {
      // Ignore episode lookup failures; titles fall back to parsed values.
    } finally {
      progress.hide();
    }
  }

  function applyGroupMatch(gi: number) {
    const group = groups[gi];
    const candidate = group.searchResults.find((c) => c.provider_id === group.selectedSearchId);
    if (!candidate) {
      updateGroup(gi, { searchStatus: "Choose a match first." });
      return;
    }
    void selectShowForGroup(gi, candidate);
  }

  async function loadGroupEpisodes(gi: number) {
    const providerShowId = groups[gi].provider_show_id;
    if (!providerShowId) {
      updateGroup(gi, { searchStatus: "Match a show first to load episodes." });
      return;
    }
    progress.startDelayed("Loading episodes...");
    try {
      const { results } = await api.episodes(mediaType, providerShowId);
      updateGroup(gi, { episodeOptions: results });
    } catch (e) {
      updateGroup(gi, {
        searchStatus: e instanceof ApiError ? e.message : "Could not load episodes.",
      });
    } finally {
      progress.hide();
    }
  }

  function buildBatch(): ImportBatch {
    return {
      media_type: mediaType,
      action,
      conflict_policy: conflict,
      items: groups.flatMap((group) =>
        group.episodes.map((ep) => ({
          source_path: ep.source_path,
          show_title: group.show_title,
          show_year: group.show_year.trim() ? Number(group.show_year) : null,
          season_number: isFilm ? 0 : ep.season_number,
          episode_number: isFilm ? 0 : ep.episode_number,
          episode_title: isFilm ? "Film" : ep.episode_title,
          quality: ep.quality,
          provider: group.provider || null,
          provider_show_id: group.provider_show_id || null,
        })),
      ),
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

  // Import runs in the background: start the job and hand off to the Imports
  // page, so the user can keep browsing and matching while it runs.
  async function runImport() {
    try {
      await api.startImportJob(buildBatch());
    } catch (e) {
      alert(e instanceof ApiError ? e.message : "Could not start import.");
      return;
    }
    imports.refresh();
    navigate("/imports");
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
      {groups.length === 0 ? (
        <p>No video files selected.</p>
      ) : (
        <>
          <div className="toolbar sticky-toolbar">
            <label>
              Action
              <Select
                value={action}
                onChange={setAction}
                options={[
                  { value: "copy", label: "Copy" },
                  { value: "hardlink", label: "Hardlink" },
                  { value: "move", label: "Move" },
                  { value: "test", label: "Test" },
                ]}
              />
            </label>
            <label>
              Conflict
              <Select
                value={conflict}
                onChange={setConflict}
                options={[
                  { value: "skip", label: "Skip" },
                  { value: "replace", label: "Replace" },
                  { value: "index", label: "Keep Both" },
                  { value: "fail", label: "Fail" },
                ]}
              />
            </label>
            <span className="muted match-count">
              {isFilm
                ? `${episodeCount} film${episodeCount === 1 ? "" : "s"}`
                : `${groups.length} folder${groups.length === 1 ? "" : "s"} · ${episodeCount} episode${
                    episodeCount === 1 ? "" : "s"
                  }`}
            </span>
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

          {groups.map((group, gi) =>
            isFilm ? (
              <FilmRow
                key={group.group_key}
                group={group}
                gi={gi}
                mediaType={mediaType}
                onUpdateGroup={updateGroup}
                onUpdateEpisode={updateEpisode}
                onToggle={() => toggleEpisode(gi, 0)}
                onSearch={() => searchGroup(gi)}
                onApply={() => applyGroupMatch(gi)}
              />
            ) : (
              <GroupCard
                key={group.group_key}
                group={group}
                gi={gi}
                isAnime={isAnime}
                onUpdateGroup={updateGroup}
                onUpdateEpisode={updateEpisode}
                onToggleEpisode={toggleEpisode}
                onSearch={() => searchGroup(gi)}
                onApply={() => applyGroupMatch(gi)}
                onLoadEpisodes={() => loadGroupEpisodes(gi)}
              />
            ),
          )}
        </>
      )}
    </section>
  );
}

interface GroupCardProps {
  group: GroupUi;
  gi: number;
  isAnime: boolean;
  onUpdateGroup: (gi: number, partial: Partial<GroupUi>) => void;
  onUpdateEpisode: (gi: number, ei: number, partial: Partial<EpisodeUi>) => void;
  onToggleEpisode: (gi: number, ei: number) => void;
  onSearch: () => void;
  onApply: () => void;
  onLoadEpisodes: () => void;
}

function GroupCard({
  group,
  gi,
  isAnime,
  onUpdateGroup,
  onUpdateEpisode,
  onToggleEpisode,
  onSearch,
  onApply,
  onLoadEpisodes,
}: GroupCardProps) {
  return (
    <div className="match-group">
      <div className="group-header">
        <div className="group-show">
          <input
            className="group-title-input"
            value={group.show_title}
            onChange={(e) => onUpdateGroup(gi, { show_title: e.target.value })}
            placeholder="Show title"
          />
          <input
            className="group-year-input"
            value={group.show_year}
            onChange={(e) => onUpdateGroup(gi, { show_year: e.target.value })}
            placeholder="Year"
          />
          {group.provider && <span className="provider-badge">{group.provider}</span>}
          <span className="muted group-count">
            {group.episodes.length} episode{group.episodes.length === 1 ? "" : "s"}
          </span>
        </div>
        <div className="group-folder muted">📁 {group.group_name}</div>
      </div>

      {group.metadata_error && (
        <p className="error group-error">Metadata lookup failed: {group.metadata_error}</p>
      )}

      <div className="manual-match group-search">
        <label>
          Re-match show
          <input
            type="search"
            value={group.query}
            onChange={(e) => onUpdateGroup(gi, { query: e.target.value })}
            placeholder="Search a different show..."
          />
        </label>
        <button className="secondary-button" type="button" onClick={onSearch}>
          Search
        </button>
        {group.searchResults.length > 0 && (
          <>
            <Select
              value={group.selectedSearchId}
              onChange={(v) => onUpdateGroup(gi, { selectedSearchId: v })}
              options={group.searchResults.map((candidate) => ({
                value: candidate.provider_id,
                label: `${candidate.title}${candidate.year ? ` (${candidate.year})` : ""} - ${candidate.provider}`,
              }))}
            />
            <button className="secondary-button" type="button" onClick={onApply}>
              Apply to folder
            </button>
          </>
        )}
        {group.provider_show_id && (
          <button className="secondary-button" type="button" onClick={onLoadEpisodes}>
            Load episode list
          </button>
        )}
        {group.searchStatus && <span className="muted">{group.searchStatus}</span>}
      </div>

      <table className="episode-table">
        <thead>
          <tr>
            <th className="col-code">Ep</th>
            <th className="col-title">Title</th>
            <th className="col-quality">Quality</th>
            <th className="col-edit" aria-label="Edit" />
          </tr>
        </thead>
        <tbody>
          {group.episodes.map((ep, ei) => (
            <Fragment key={ep.source_path}>
              <tr className="episode-row" onClick={() => onToggleEpisode(gi, ei)}>
                <td className="col-code">
                  S{pad2(ep.season_number)}E{pad2(ep.episode_number)}
                </td>
                <td className="col-title">{ep.episode_title}</td>
                <td className="col-quality">{ep.quality}</td>
                <td className="col-edit">{ep.expanded ? "▾" : "▸"}</td>
              </tr>
              {ep.expanded && (
                <tr className="episode-editor-row">
                  <td colSpan={4}>
                    <div className="episode-editor">
                      <div className="grid">
                        <label>
                          Season
                          <input
                            type="number"
                            min={0}
                            value={ep.season_number}
                            onChange={(e) =>
                              onUpdateEpisode(gi, ei, { season_number: Number(e.target.value) })
                            }
                          />
                        </label>
                        <label>
                          Episode
                          <input
                            type="number"
                            min={0}
                            value={ep.episode_number}
                            onChange={(e) =>
                              onUpdateEpisode(gi, ei, { episode_number: Number(e.target.value) })
                            }
                          />
                        </label>
                        <label>
                          Episode title
                          <input
                            value={ep.episode_title}
                            onChange={(e) =>
                              onUpdateEpisode(gi, ei, { episode_title: e.target.value })
                            }
                          />
                        </label>
                        <label>
                          Quality
                          <input
                            value={ep.quality}
                            onChange={(e) => onUpdateEpisode(gi, ei, { quality: e.target.value })}
                          />
                        </label>
                      </div>
                      {group.episodeOptions.length > 0 && (
                        <label className="episode-picker">
                          Pick from provider
                          <Select
                            value=""
                            placeholder="Pick an episode..."
                            onChange={(v) => {
                              const choice = group.episodeOptions[Number(v)];
                              if (choice) {
                                onUpdateEpisode(gi, ei, {
                                  season_number: choice.season,
                                  episode_number: choice.episode,
                                  episode_title: choice.title,
                                });
                              }
                            }}
                            options={group.episodeOptions.map((choice, ci) => ({
                              value: String(ci),
                              label: `S${pad2(choice.season)}E${pad2(choice.episode)} - ${choice.title}`,
                            }))}
                          />
                        </label>
                      )}
                      <p className="muted path-line">{ep.source_path}</p>
                    </div>
                  </td>
                </tr>
              )}
            </Fragment>
          ))}
        </tbody>
      </table>
      {isAnime && group.episodes.length > 1 && (
        <p className="muted hint-line">
          Anime episode titles are matched by episode number across the folder.
        </p>
      )}
    </div>
  );
}

interface FilmRowProps {
  group: GroupUi;
  gi: number;
  mediaType: MediaType;
  onUpdateGroup: (gi: number, partial: Partial<GroupUi>) => void;
  onUpdateEpisode: (gi: number, ei: number, partial: Partial<EpisodeUi>) => void;
  onToggle: () => void;
  onSearch: () => void;
  onApply: () => void;
}

function FilmRow({
  group,
  gi,
  onUpdateGroup,
  onUpdateEpisode,
  onToggle,
  onSearch,
  onApply,
}: FilmRowProps) {
  const ep = group.episodes[0];
  const expanded = ep?.expanded ?? false;
  return (
    <div className="match-group film-group">
      <div className="episode-row film-row" onClick={onToggle}>
        <span className="film-title">
          {group.show_title}
          {group.show_year ? ` (${group.show_year})` : ""}
        </span>
        {group.provider && <span className="provider-badge">{group.provider}</span>}
        <span className="col-quality">{ep?.quality}</span>
        <span className="col-edit">{expanded ? "▾" : "▸"}</span>
      </div>
      {expanded && (
        <div className="episode-editor">
          {group.metadata_error && (
            <p className="error group-error">Metadata lookup failed: {group.metadata_error}</p>
          )}
          <div className="grid">
            <label>
              Film title
              <input
                value={group.show_title}
                onChange={(e) => onUpdateGroup(gi, { show_title: e.target.value })}
              />
            </label>
            <label>
              Year
              <input
                value={group.show_year}
                onChange={(e) => onUpdateGroup(gi, { show_year: e.target.value })}
              />
            </label>
            <label>
              Quality
              <input
                value={ep?.quality ?? ""}
                onChange={(e) => onUpdateEpisode(gi, 0, { quality: e.target.value })}
              />
            </label>
          </div>
          <div className="manual-match group-search">
            <label>
              Re-match film
              <input
                type="search"
                value={group.query}
                onChange={(e) => onUpdateGroup(gi, { query: e.target.value })}
              />
            </label>
            <button className="secondary-button" type="button" onClick={onSearch}>
              Search
            </button>
            {group.searchResults.length > 0 && (
              <>
                <Select
                  value={group.selectedSearchId}
                  onChange={(v) => onUpdateGroup(gi, { selectedSearchId: v })}
                  options={group.searchResults.map((candidate) => ({
                    value: candidate.provider_id,
                    label: `${candidate.title}${candidate.year ? ` (${candidate.year})` : ""} - ${candidate.provider}`,
                  }))}
                />
                <button className="secondary-button" type="button" onClick={onApply}>
                  Apply
                </button>
              </>
            )}
            {group.searchStatus && <span className="muted">{group.searchStatus}</span>}
          </div>
          <p className="muted path-line">{ep?.source_path}</p>
        </div>
      )}
    </div>
  );
}

