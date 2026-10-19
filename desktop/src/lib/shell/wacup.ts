// The WACUP panel's pure side: what the backend's poll answers (wacup.rs),
// the clock line, and the poll's rhythm.

/** The shape `wacup_state` answers. */
export interface WacupState {
  supported: boolean;
  running: boolean;
  /** `WACUP`, `Winamp`, or the program's file name. */
  player: string;
  playback: 'playing' | 'paused' | 'stopped';
  title: string;
  position_ms: number;
  /** 0 when the player does not know the length (a stream). */
  length_ms: number;
  can_launch: boolean;
}

export type WacupControl = 'previous' | 'play_pause' | 'stop' | 'next';

/** The poll, while the panel is visible. */
export const POLL_MS = 1000;

/** `3:05`, or `1:02:03` from an hour; a negative or missing time is `0:00`. */
export function clock(ms: number): string {
  const total = Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 1000) : 0;
  const s = total % 60;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  const two = (n: number) => String(n).padStart(2, '0');
  return h ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`;
}

/** `1:23 / 3:35`; just the elapsed time when the length is unknown. */
export function clockLine(positionMs: number, lengthMs: number): string {
  return lengthMs > 0 ? `${clock(positionMs)} / ${clock(lengthMs)}` : clock(positionMs);
}

/** How far a title wider than its box slides to show its end, and how long
 *  that takes at a reading pace (about 30 px a second, at least 4 s). */
export function marquee(textWidth: number, boxWidth: number): { shift: number; seconds: number } | null {
  const shift = Math.ceil(textWidth - boxWidth);
  if (!(shift > 0)) return null;
  return { shift, seconds: Math.max(4, Math.round(shift / 30) + 2) };
}

/** The line under the transport when there is no track. */
export function idleLine(s: WacupState): string {
  if (!s.running) return 'WACUP not running';
  return s.playback === 'stopped' ? `${s.player} is stopped` : '';
}
