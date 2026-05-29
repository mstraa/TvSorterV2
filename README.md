# TvSorter V2

TvSorter is a LAN-only web app for curating mounted TV/anime/film files into clean
output libraries by hardlinking or copying selected media. V2 is a full rewrite of
the original Python/FastAPI app with a **Rust backend** and a **TypeScript/React frontend**.

- **Backend:** Rust (axum + tokio), SQLite via rusqlite, reqwest for metadata providers.
- **Frontend:** React + TypeScript built with Vite, embedded into the Rust binary.
- **Deployment:** privileged Proxmox LXC, built from source (Rust + Node) and run under systemd.

## Features

- Browse one or more configured input roots from the web UI.
- Select individual files or whole folders; folders are expanded recursively and only supported video files are imported.
- Sort TV, Anime, and Film into separate output roots.
- Parse titles, years, season/episode numbers, episode titles, and quality tags from filenames.
- **ffprobe resolution fallback** fills the quality when no quality tag is present in the filename.
- Look up TV metadata with TVMaze, anime metadata with Jikan, and film metadata with IMDb-style public suggestions plus Wikidata fallback.
- Cache provider responses in SQLite and de-duplicate metadata lookups during batch matching.
- Manually correct every match field before import (title, year, season, episode, episode title, quality, provider, provider ID).
- Search metadata again from the match queue and apply a selected result to one row or every row in the current batch.
- **Provider episode dropdown:** load a show's episode list and pick the exact episode to fill season/number/title.
- Multi-season anime matching falls back to matching by episode number when the season does not line up with the provider entry.
- Preview destination paths before importing.
- Import by hardlink, copy, or test/preview-only mode.
- Handle destination conflicts with skip, replace, keep-both indexing, or fail.
- Run imports as background jobs with current item, total progress, current-file copy progress, and cancellation.
- Remove partial destination files when a copy is cancelled.
- Limit copy speed from Settings; the default is 15 Mo/s, and `0` disables the limit.
- Keep source files untouched.
- Persist settings, input roots, import history, library state, provider cache, and manual source status overrides in SQLite.
- Show latest source status in Browse, filter Browse by status, and manually mark selected files/folders as auto, no status, imported, failed, skipped, preview, or conflict.
- Show Library and History pages, including an output-folder rescan that discovers existing media and marks missing files.
- Pick input and output folders from a server-side folder browser in Settings.
- Show read/write permission checks for configured roots.
- Toggle light/dark theme in the browser.
- Expose `GET /health` for service health checks.
- Provide Proxmox LXC creation, update, and media-mount access helper scripts.

## Import Workflow

1. Open Settings and configure input roots plus TV, Anime, and Film output roots.
2. Use Browse to choose an input root, navigate folders, select files or folders, choose the media type, and click **Match Selected**.
3. Review the Match Queue, adjust metadata if needed, optionally search providers again or load the episode list, and preview destination paths.
4. Choose hardlink, copy, or test and select a conflict policy.
5. Start the import. The progress dialog shows batch and copy progress and can cancel a running copy.
6. Review Import Results, then use Library and History to inspect persisted output state.

## Naming

TV and Anime:

```text
Show Name (Year)/Season XX/Show Name (Year) - SXXEYY - Episode Name - Quality.ext
```

Film:

```text
Film Name (Year) - Quality.ext
```

Supported video extensions are `.avi`, `.m2ts`, `.m4v`, `.mkv`, `.mov`, `.mp4`, `.mpeg`, `.mpg`, `.ts`, `.webm`, and `.wmv`.

## Development

Requires a recent stable Rust toolchain and Node.js 18+.

Run the backend (serves the JSON API on port 8080):

```sh
TVSORTER_DATA_DIR=.local-data cargo run
```

In a second terminal, run the Vite dev server (proxies `/api` and `/health` to the backend):

```sh
cd frontend
npm install
npm run dev
```

Open the Vite URL (default `http://127.0.0.1:5173`), configure input/output folders, then browse and import.

### Production build

The frontend bundle is embedded into the Rust binary, so build the frontend first:

```sh
cd frontend && npm ci && npm run build && cd ..
cargo build --release
./target/release/tvsorter
```

## Verification

```sh
cargo test            # backend unit tests
cd frontend && npm run build   # type-check + bundle the frontend
```

## Configuration

Environment variables:

- `TVSORTER_DATA_DIR`: directory for SQLite data, default `~/.local/share/tvsorter`
- `TVSORTER_DATABASE`: explicit SQLite database path
- `TVSORTER_HOST`: service host, default `0.0.0.0`
- `TVSORTER_PORT`: service port, default `8080`

Runtime settings saved through the UI:

- Input roots, one path per line.
- TV output root.
- Anime output root.
- Film output root.
- Copy speed limit in Mo/s.

See [docs/PRD.md](docs/PRD.md) and [DEV.md](DEV.md) before development work.

## Proxmox LXC

Run this from the Proxmox VE host to create a privileged Debian LXC and build TvSorter from source.
Because the container compiles Rust, the defaults are larger than the Python version
(4 cores / 4096 MiB / 14 GiB disk).

```sh
bash -c "$(curl -fsSL https://raw.githubusercontent.com/mstraa/TvSorterV2/main/scripts/create-proxmox-lxc.sh)" -- \
  --ctid 120 \
  --mount /tank/downloads:/mnt/downloads \
  --mount /tank/media/TV:/mnt/media/TV \
  --mount /tank/media/Anime:/mnt/media/Anime \
  --mount /tank/media/Films:/mnt/media/Films
```

> Replace the repo URL with your own remote, or pass `--repo https://github.com/you/TvSorterV2.git`.
> The default `github.com/mstraa/TvSorterV2` is a placeholder.

The script prompts for root disk and template storage when run interactively. Use `--help` to see static IP, SSH key, storage, and sizing options.

The LXC console is configured to autologin as root, matching common Proxmox helper-script containers.

Inside the LXC, update TvSorter to the latest GitHub `main` (this re-runs the frontend and Rust build) with:

```sh
update
```

If output mounts are shared with another LXC, keep the existing media permissions and match TvSorter to the mount identity instead:

```sh
stat -c '%u:%g %A %n' /mnt/data/Movies
tvsorter-access --path /mnt/data/Movies --mode group
```

Use `--mode owner` instead if the mount is writable only by its owner.
