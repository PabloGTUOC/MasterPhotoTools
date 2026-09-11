import { createApp } from 'vue';
import { createRouter, createWebHistory } from 'vue-router';
import '@ui/style.css';
import App from './App.vue';

import Dashboard from '@ui/views/Dashboard.vue';
import { sharedToolRoutes } from '@ui/routes';

// Web-only: the Google refresh token lives on the server (§2.3), so publishing
// has no meaning in a build that talks to `core` directly.
import Publish from './views/Publish.vue';

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: Dashboard },
    // The tabs both applications carry, defined once in `@ui/routes`.
    ...sharedToolRoutes,
    // Last, as it is the last step of the workflow.
    { path: '/publish', component: Publish },
  ],
});

createApp(App).use(router).mount('#app');
