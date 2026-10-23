import { mount } from 'svelte';
import App from './App.svelte';
import './tokens.css';
import './editor.css';

mount(App, { target: document.getElementById('app')! });
