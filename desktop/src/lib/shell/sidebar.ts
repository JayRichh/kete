// The sidebar's width rules: the limits a drag keeps to, the default, and
// what a saved width reads back as. The same numbers as
// crates/nebula-config/src/desktop.rs (SIDEBAR_*), which clamps the saved
// value again.

export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 480;
/** The sidebar was 220 px; a little wider is 24 more. */
export const SIDEBAR_DEFAULT = 244;
/** The icon rail the sidebar folds to. */
export const RAIL_WIDTH = 44;
/** How long the pointer rests on the rail before the sidebar peeks. */
export const PEEK_DELAY_MS = 300;
/** The arrow keys' step on the resizer; Shift takes the big one. */
export const KEY_STEP = 16;
export const KEY_STEP_BIG = 48;

/** `width` held between the limits, whole pixels; a number that is not
 *  one reads as the default. */
export function clampSidebar(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_DEFAULT;
  return Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, width)));
}

/** The width a saved preference means: none (0, absent, junk) is the default. */
export function savedSidebar(saved: unknown): number {
  return typeof saved === 'number' && Number.isFinite(saved) && saved > 0 ? clampSidebar(saved) : SIDEBAR_DEFAULT;
}

/** The width a drag makes: the pointer's distance from the sidebar's left edge. */
export function dragWidth(clientX: number, leftEdge: number): number {
  return clampSidebar(clientX - leftEdge);
}

/** The width an arrow key on the resizer makes; Home and End go to the limits. */
export function keyWidth(key: string, width: number, shift = false): number | undefined {
  const step = shift ? KEY_STEP_BIG : KEY_STEP;
  if (key === 'ArrowLeft') return clampSidebar(width - step);
  if (key === 'ArrowRight') return clampSidebar(width + step);
  if (key === 'Home') return SIDEBAR_MIN;
  if (key === 'End') return SIDEBAR_MAX;
  return undefined;
}
