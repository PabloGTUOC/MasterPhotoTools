/**
 * Main entry point for the Bulk LUT harness.
 */
import { createApp, h } from 'vue';
import BulkLut from '../../src/views/BulkLut.vue';
import '@ui/style.css';

const app = createApp({
  render() {
    return h(BulkLut);
  },
});

app.mount('#app');
