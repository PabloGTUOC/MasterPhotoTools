/**
 * Main entry point for the Develop harness.
 */
import { createApp, h, ref } from 'vue';
import Develop from '../../src/views/Develop.vue';
import '@ui/style.css';

const isMounted = ref(true);

declare global {
  interface Window {
    __unmountDevelop: () => void;
    __remountDevelop: () => void;
  }
}

if (typeof window !== 'undefined') {
  window.__unmountDevelop = () => {
    isMounted.value = false;
  };
  window.__remountDevelop = () => {
    isMounted.value = true;
  };
}

const app = createApp({
  render() {
    return isMounted.value ? h(Develop) : h('div', { id: 'unmounted-marker' }, 'Unmounted');
  },
});

app.mount('#app');
