import { createApp } from 'vue';
import { createRouter, createWebHistory } from 'vue-router';
import '@ui/style.css';
import App from './App.vue';

import Dashboard from '@ui/views/Dashboard.vue';
import { sharedToolRoutes } from '@ui/routes';

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: Dashboard },
    // The tabs both applications carry, defined once in `@ui/routes`.
    ...sharedToolRoutes,
  ],
});

createApp(App).use(router).mount('#app');
