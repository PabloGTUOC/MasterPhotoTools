/**
 * The tabs both applications carry, defined once — **in workflow order**.
 *
 * Every one of these screens is rendered twice, so its path, its label and —
 * for the four that are `ImageTool` with different props — its wording belong
 * in one place. They were copied between the two `main.ts` files and agreed
 * only because nobody had edited one of them: a reworded blurb on the web
 * would have left the desktop describing the same tool differently, and
 * nothing would have failed.
 *
 * The order is the order the work is actually done in: dates before names
 * because the renamer builds a name out of the date, both before the timeline
 * and geotagging because a position is matched on time, the timeline before
 * geotagging because a fix has to be in the library before a frame can be
 * matched to it, conversion before the tools that
 * only read JPEG, and splitting before bordering because a border drawn on a
 * half-frame scan would be cut in half by the split. It is not alphabetical
 * and not the order these were built in.
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
import Timeline from './views/Timeline.vue';
import Transform from './views/Transform.vue';

export const sharedToolRoutes = [
  { path: '/dates', component: Dates, meta: { label: 'Dates' } },
  { path: '/rename', component: Rename, meta: { label: 'Rename' } },
  // Timeline before Geotag: a position has to be in the library before anything
  // can be matched to it, and the tab that fills a blank day is the one you
  // reach for when Geotag has nothing to offer a roll of film.
  { path: '/timeline', component: Timeline, meta: { label: 'Timeline' } },
  { path: '/geotag', component: Geotag, meta: { label: 'Geotag' } },
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
  {
    path: '/raw-to-jpeg',
    component: RawToJpeg,
    meta: { label: 'RAW' },
  },
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
  // Neither of these is a step in the chain above — a contact sheet is made
  // *from* a set of photographs rather than applied to each, and Transform is
  // the general-purpose escape hatch for a one-off rotate or resize. They sit
  // after the chain and before publishing so the numbered run reads straight
  // through.
  { path: '/contact-sheet', component: ContactSheet, meta: { label: 'Sheet' } },
  { path: '/transform', component: Transform, meta: { label: 'Transform' } },
];

/**
 * Ingest is step 1 and belongs to the desktop, so the shared block starts at 2.
 *
 * The number is the step in the workflow, not the position in a menu, which is
 * why it is computed here and not in either navigation bar: Geotag is 05 on the
 * Mac and 05 on a phone, even though the two bars begin with different screens.
 */
export const FIRST_SHARED_STEP = 2;

/** Publishing is the last step, and only the web application offers it. */
export const PUBLISH_STEP = FIRST_SHARED_STEP + sharedToolRoutes.length;

/** `2` → `'02'`. Two digits so the labels stay in one column. */
export function stepLabel(step: number): string {
  return String(step).padStart(2, '0');
}

/** The shared tabs as a navigation bar reads them: a path, a word, a step. */
export const sharedToolLinks = sharedToolRoutes.map((route, index) => ({
  to: route.path,
  label: route.meta.label,
  step: stepLabel(FIRST_SHARED_STEP + index),
}));
