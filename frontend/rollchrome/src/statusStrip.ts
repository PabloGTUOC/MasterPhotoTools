import type { InjectionKey } from 'vue';

export interface StatusStripState {
  hasDetails: boolean;
  detailsOpen: boolean;
  /**
   * How far, in px, the details drawer stops short of the main column's right edge. A screen with a
   * settings column whose action footer must stay usable sets it, so the drawer opens beside the
   * actions rather than under them. Reset to 0 on every route change.
   */
  detailsInsetRight: number;
}

export const STATUS_STRIP: InjectionKey<StatusStripState> = Symbol('STATUS_STRIP');
