import { createApp } from 'vue';
import { createRouter, createWebHistory } from 'vue-router';
import './style.css';
import App from './App.vue';

import ContactSheet from './views/ContactSheet.vue';
import Dashboard from './views/Dashboard.vue';
import Dates from './views/Dates.vue';
import ImageTool from './views/ImageTool.vue';
import Library from './views/Library.vue';
import Rename from './views/Rename.vue';
import Transform from './views/Transform.vue';

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: Dashboard },
    { path: '/library', component: Library },
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
