import { useState } from "react";
import { NavLink, Navigate, Route, Routes } from "react-router-dom";
import { ProgressProvider } from "./components/Progress";
import BrowsePage from "./pages/Browse";
import MatchPage from "./pages/Match";
import ResultsPage from "./pages/Results";
import LibraryPage from "./pages/Library";
import HistoryPage from "./pages/History";
import SettingsPage from "./pages/Settings";
import { toggleTheme, type Theme } from "./theme";

const NAV = [
  { to: "/browse", label: "Browse", hint: "input" },
  { to: "/library", label: "Library", hint: "output" },
  { to: "/history", label: "History", hint: "log" },
  { to: "/settings", label: "Settings", hint: "config" },
];

export default function App() {
  const [theme, setTheme] = useState<Theme>(
    (document.documentElement.dataset.theme as Theme) || "dark",
  );

  return (
    <ProgressProvider>
      <div className="terminal-shell">
        <aside className="sidebar">
          <div className="brand">
            <span className="prompt-mark">▸</span>
            <div>
              <h1>TvSorter</h1>
              <p className="eyebrow">media sorter</p>
            </div>
          </div>

          <nav className="filter-list">
            {NAV.map((item) => (
              <NavLink key={item.to} to={item.to} className="nav-link">
                <span>{item.label}</span>
                <strong>{item.hint}</strong>
              </NavLink>
            ))}
          </nav>

          <div className="sidebar-bottom">
            <div className="connection live">
              <span className="connection-dot" />
              LAN · hardlink / copy
            </div>
            <button
              className="secondary-button theme-toggle"
              type="button"
              onClick={() => setTheme(toggleTheme(theme))}
            >
              theme: {theme}
            </button>
          </div>
        </aside>

        <main className="workspace">
          <Routes>
            <Route path="/" element={<Navigate to="/browse" replace />} />
            <Route path="/browse" element={<BrowsePage />} />
            <Route path="/match" element={<MatchPage />} />
            <Route path="/results/:jobId" element={<ResultsPage />} />
            <Route path="/library" element={<LibraryPage />} />
            <Route path="/history" element={<HistoryPage />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="*" element={<Navigate to="/browse" replace />} />
          </Routes>
        </main>
      </div>
    </ProgressProvider>
  );
}
