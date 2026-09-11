/**
 * The tabs both applications carry, defined once.
 *
 * Every one of these screens is rendered twice, so its path, its label and —
 * for the four that are `ImageTool` with different props — its wording belong
 * in one place. They were copied between the two `main.ts` files and agreed
 * only because nobody had edited one of them: a reworded blurb on the web
 * would have left the desktop describing the same tool differently, and
 * nothing would have failed.
 *
 * What is *not* here is each application's own screens. The web opens on a
 * dashboard and carries `Publish`, because the Google refresh token lives on
 * exactly one machine; the desktop opens on `Ingest`, because §2.3 puts the
 * card reader on the Mac. Those stay in their own routers, where the reason
 * for them is visible.
 *
 * No `vue-router` import: it is not a dependency of the shared package, and
 * these records are structurally what a router wants. Each application spreads
 * them into its own `routes` array and types them there.
 */
import ContactSheet from './views/ContactSheet.vue';
import Dates from './views/Dates.vue';
import Geotag from './views/Geotag.vue';
import ImageTool from './views/ImageTool.vue';
import RawToJpeg from './views/RawToJpeg.vue';
import Rename from './views/Rename.vue';
import Transform from './views/Transform.vue';

export const sharedToolRoutes = [
  { path: '/dates', component: Dates, meta: { label: 'Dates' } },
  { path: '/rename', component: Rename, meta: { label: 'Rename' } },
  { path: '/geotag', component: Geotag, meta: { label: 'Geotag' } },
  {
    path: '/split',
    component: ImageTool,
    meta: { label: 'Split' },
    props: {
      operation: 'split',
      title: 'Half-frame split',
      blurb:
        'Separate the two photographs in a half-frame scan. The lab border is removed, the divider located, and each half written as _A and _B.',
      applyLabel: 'Split scans',
    },
  },
  { path: '/contact-sheet', component: ContactSheet, meta: { label: 'Sheet' } },
  { path: '/transform', component: Transform, meta: { label: 'Transform' } },
  {
    path: '/border',
    component: ImageTool,
    meta: { label: 'Border' },
    props: {
      operation: 'border',
      title: 'Print border',
      blurb:
        'Place an image on a fixed white print canvas with rounded corners, sized for print and for platforms that crop unpredictably.',
      applyLabel: 'Add borders',
    },
  },
  {
    path: '/raw-to-jpeg',
    component: RawToJpeg,
    meta: { label: 'RAW' },
  },
  {
    path: '/tiff-to-jpeg',
    component: ImageTool,
    meta: { label: 'TIFF' },
    props: {
      operation: 'tiffToJpeg',
      title: 'TIFF to JPEG',
      blurb:
        'Convert scanner output to a distributable format. Multi-page TIFFs produce one numbered JPEG per page.',
      applyLabel: 'Convert',
    },
  },
];

/** The shared tabs as a navigation bar reads them: a path and a word. */
export const sharedToolLinks = sharedToolRoutes.map((route) => ({
  to: route.path,
  label: route.meta.label,
}));
