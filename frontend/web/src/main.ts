import { createApp } from 'vue';
import { createRouter, createWebHistory } from 'vue-router';
import '@ui/style.css';
import App from './App.vue';

import ContactSheet from '@ui/views/ContactSheet.vue';
import Dashboard from '@ui/views/Dashboard.vue';
import Dates from '@ui/views/Dates.vue';
import ImageTool from '@ui/views/ImageTool.vue';
import Rename from '@ui/views/Rename.vue';
import Transform from '@ui/views/Transform.vue';

// Web-only: the Google refresh token lives on the server (§2.3), so publishing
// has no meaning in a build that talks to `core` directly.
import Publish from './views/Publish.vue';

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: Dashboard },
    { path: '/publish', component: Publish },
    { path: '/dates', component: Dates },
    { path: '/rename', component: Rename },
    {
      path: '/split',
      component: ImageTool,
      props: {
        operation: 'split',
        title: 'Half-frame split',
        blurb:
          'Separate the two photographs in a half-frame scan. The lab border is removed, the divider located, and each half written as _A and _B.',
        applyLabel: 'Split scans',
      },
    },
    { path: '/contact-sheet', component: ContactSheet },
    { path: '/transform', component: Transform },
    {
      path: '/border',
      component: ImageTool,
      props: {
        operation: 'border',
        title: 'Print border',
        blurb:
          'Place an image on a fixed white print canvas with rounded corners, sized for print and for platforms that crop unpredictably.',
        applyLabel: 'Add borders',
      },
    },
    {
      path: '/tiff-to-jpeg',
      component: ImageTool,
      props: {
        operation: 'tiffToJpeg',
        title: 'TIFF to JPEG',
        blurb:
          'Convert scanner output to a distributable format. Multi-page TIFFs produce one numbered JPEG per page.',
        applyLabel: 'Convert',
      },
    },
  ],
});

createApp(App).use(router).mount('#app');
