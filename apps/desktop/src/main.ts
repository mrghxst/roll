import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import './lib/styles/tokens.css';
import './lib/styles/base.css';
import { mount } from 'svelte';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('Missing #app element');

export default mount(App, { target });
