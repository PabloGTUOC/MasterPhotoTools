import { createApp } from 'vue';
import { createRouter, createWebHashHistory } from 'vue-router';
import '@ui/style.css';
import App from '../../src/App.vue';

import Develop from '../../src/views/Develop.vue';
import Edit from '../../src/views/Edit.vue';
import BulkLut from '../../src/views/BulkLut.vue';

// Hash history: the app is served from a file URL inside Tauri, where path
// routing has no server to fall back on.
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: Develop },
    { path: '/edit', component: Edit },
    { path: '/batch', component: BulkLut },
    { path: '/bulk-lut', redirect: '/batch' },
  ],
});

createApp(App).use(router).mount('#app');
