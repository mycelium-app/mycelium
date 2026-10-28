import { mount } from 'svelte';
import Root from './Root.svelte';
import { themeFromQuery } from './lib/theme';
import './tokens.css';
import './editor.css';
import './kinds.css';

const systemDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
const { theme, variables } = themeFromQuery(window.location.search, systemDark);
const root = document.documentElement;

root.dataset.theme = theme;
for (const [name, value] of Object.entries(variables)) {
  root.style.setProperty(name, value);
}

mount(Root, { target: document.getElementById('app')! });
