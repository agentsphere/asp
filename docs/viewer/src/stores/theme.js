import { writable } from 'svelte/store';

const stored = typeof localStorage !== 'undefined' ? localStorage.getItem('theme') : null;
const prefersDark = typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches;

export const theme = writable(stored || (prefersDark ? 'dark' : 'dark'));

theme.subscribe(value => {
  if (typeof document !== 'undefined') {
    document.documentElement.classList.toggle('light', value === 'light');
  }
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('theme', value);
  }
});

export function toggleTheme() {
  theme.update(t => t === 'dark' ? 'light' : 'dark');
}
