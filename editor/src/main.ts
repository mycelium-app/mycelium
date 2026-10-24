import { mount } from 'svelte';
import App from './App.svelte';
import { themeFromQuery } from './lib/theme';
import './tokens.css';
import './editor.css';

const systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
const { theme, variables } = themeFromQuery(window.location.search, systemDark);
const root = document.documentElement;

root.dataset.theme = theme;
for (const [name, value] of Object.entries(variables)) {
  root.style.setProperty(name, value);
}

mount(App, { target: document.getElementById('app')! });
