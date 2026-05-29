const STORAGE_KEY = "tvsorter-theme";

export type Theme = "light" | "dark";

export function initTheme(): Theme {
  const saved = localStorage.getItem(STORAGE_KEY) as Theme | null;
  // The terminal aesthetic is dark-first; default to dark unless the user
  // explicitly chose light before.
  const theme = saved ?? "dark";
  applyTheme(theme);
  return theme;
}

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem(STORAGE_KEY, theme);
}

export function toggleTheme(current: Theme): Theme {
  const next: Theme = current === "dark" ? "light" : "dark";
  applyTheme(next);
  return next;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 o";
  }
  const units = ["o", "Ko", "Mo", "Go", "To"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** exponent;
  const precision = exponent <= 2 ? 0 : 2;
  let formatted = value.toFixed(precision);
  if (formatted.includes(".")) {
    formatted = formatted.replace(/0+$/, "").replace(/\.$/, "");
  }
  return `${formatted.replace(".", ",")} ${units[exponent]}`;
}
