/**
 * Main entry point for the Edit view benchmark and acceptance harness.
 */
import { createApp, h, ref } from 'vue';
import Edit from '../../src/views/Edit.vue';
import '@ui/style.css';

const showEdit = ref(true);
(window as any).__showEdit = showEdit;

const app = createApp({
  render() {
    return showEdit.value ? h(Edit) : h('div', { id: 'other-tab' }, 'Other tab');
  },
});

app.mount('#app');
