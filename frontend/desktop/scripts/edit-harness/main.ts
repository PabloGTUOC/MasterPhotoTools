/**
 * Main entry point for the Edit view benchmark and acceptance harness.
 */
import { createApp, h } from 'vue';
import Edit from '../../src/views/Edit.vue';
import '@ui/style.css';

const app = createApp({
  render() {
    return h(Edit);
  },
});

app.mount('#app');
