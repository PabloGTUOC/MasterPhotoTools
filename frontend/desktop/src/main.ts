import { createApp } from 'vue';
import { createRouter, createWebHashHistory } from 'vue-router';
import '@ui/style.css';
import App from './App.vue';

import ContactSheet from '@ui/views/ContactSheet.vue';
import Dates from '@ui/views/Dates.vue';
import ImageTool from '@ui/views/ImageTool.vue';
import Library from '@ui/views/Library.vue';
import Rename from '@ui/views/Rename.vue';
import Transform from '@ui/views/Transform.vue';

// Desktop-only: §2.3 puts the card reader on the Mac, so the review screen has
// no meaning in a build that cannot see a card.
import Ingest from './views/Ingest.vue';

// Hash history: the app is served from a file URL inside Tauri, where path
// routing has no server to fall back on.
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', component: Library },
    { path: '/ingest', component: Ingest },
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
