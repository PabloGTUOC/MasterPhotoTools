import { createApp } from 'vue';
import { createRouter, createWebHashHistory } from 'vue-router';
import '@ui/style.css';
import App from './App.vue';

import { sharedToolRoutes } from '@ui/routes';

// Desktop-only: §2.3 puts the card reader on the Mac, so the review screen has
// no meaning in a build that cannot see a card.
import Ingest from './views/Ingest.vue';

// Hash history: the app is served from a file URL inside Tauri, where path
// routing has no server to fall back on.
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    // Ingest is the landing screen: §2.3 puts the card reader on this machine,
    // and reading a card is what the desktop application is for.
    { path: '/', component: Ingest },
    // The tabs both applications carry, defined once in `@ui/routes`.
    ...sharedToolRoutes,
  ],
});

createApp(App).use(router).mount('#app');
