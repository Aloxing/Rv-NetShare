export type ThemeMode = 'light' | 'dark' | 'system';

const THEME_KEY = 'lan:theme';
const darkMedia = window.matchMedia('(prefers-color-scheme: dark)');

export function loadThemeMode(): ThemeMode {
  const raw = localStorage.getItem(THEME_KEY);
  return raw === 'light' || raw === 'dark' || raw === 'system' ? raw : 'system';
}

export function applyThemeMode(mode: ThemeMode) {
  const root = document.documentElement;
  root.classList.remove('theme-light', 'theme-dark');
  if (mode === 'light') {
    root.classList.add('theme-light');
  } else if (mode === 'dark') {
    root.classList.add('theme-dark');
  } else if (darkMedia.matches) {
    root.classList.add('theme-dark');
  } else {
    root.classList.add('theme-light');
  }
}

export function initTheme() {
  applyThemeMode(loadThemeMode());
  darkMedia.addEventListener('change', () => {
    if (loadThemeMode() === 'system') applyThemeMode('system');
  });
}

export function setThemeMode(mode: ThemeMode): ThemeMode {
  localStorage.setItem(THEME_KEY, mode);
  applyThemeMode(mode);
  return mode;
}
