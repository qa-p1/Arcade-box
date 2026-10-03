import { mount } from 'svelte';
import PinViewer from './lib/PinViewer.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('Pinned image root element is missing');

mount(PinViewer, { target });
