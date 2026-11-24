import type { InjectionKey } from 'vue';

export interface StatusStripState {
  hasDetails: boolean;
  detailsOpen: boolean;
}

export const STATUS_STRIP: InjectionKey<StatusStripState> = Symbol('STATUS_STRIP');
