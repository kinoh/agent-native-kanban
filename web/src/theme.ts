export type Theme = "light" | "dark";

export const themeStorageKey = "agent-native-kanban.theme";

function systemTheme(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = theme;
}

export function currentTheme(): Theme {
  const theme = document.documentElement.dataset.theme;
  return theme === "light" || theme === "dark" ? theme : systemTheme();
}

export function chooseTheme(theme: Theme) {
  applyTheme(theme);
  try {
    window.localStorage.setItem(themeStorageKey, theme);
  } catch (error) {
    console.warn("Could not store theme preference", error);
  }
}
