import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PEEK_DELAY_MS, RAIL_WIDTH, SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN, clampSidebar, dragWidth, keyWidth, savedSidebar } from './sidebar';

const set = vi.fn(() => Promise.resolve());
vi.mock('../api', () => ({ api: { desktopSetSidebar: (...a: unknown[]) => (set as (...a: unknown[]) => unknown)(...a) } }));
vi.mock('../store.svelte', () => ({ toast: vi.fn() }));

describe('the sidebar width', () => {
  it('is 24 px wider than the 220 px it was', () => {
    expect(SIDEBAR_DEFAULT).toBe(220 + 24);
    expect(RAIL_WIDTH).toBe(44);
    expect(PEEK_DELAY_MS).toBe(300);
  });

  it('stays between 200 and 480 px', () => {
    expect(clampSidebar(10)).toBe(SIDEBAR_MIN);
    expect(clampSidebar(5000)).toBe(SIDEBAR_MAX);
    expect(clampSidebar(300.6)).toBe(301);
    expect(clampSidebar(Number.NaN)).toBe(SIDEBAR_DEFAULT);
  });

  it('reads a saved width back clamped, and none as the default', () => {
    expect(savedSidebar(undefined)).toBe(SIDEBAR_DEFAULT);
    expect(savedSidebar(0)).toBe(SIDEBAR_DEFAULT);
    expect(savedSidebar('300')).toBe(SIDEBAR_DEFAULT);
    expect(savedSidebar(320)).toBe(320);
    expect(savedSidebar(9000)).toBe(SIDEBAR_MAX);
    expect(savedSidebar(50)).toBe(SIDEBAR_MIN);
  });

  it('follows the pointer from the sidebar edge, held to the limits', () => {
    expect(dragWidth(300, 0)).toBe(300);
    expect(dragWidth(100, 0)).toBe(SIDEBAR_MIN);
    expect(dragWidth(900, 0)).toBe(SIDEBAR_MAX);
  });

  it('steps with the arrows and jumps with Home and End', () => {
    expect(keyWidth('ArrowRight', 244)).toBe(260);
    expect(keyWidth('ArrowLeft', 244, true)).toBe(SIDEBAR_MIN);
    expect(keyWidth('ArrowLeft', 204)).toBe(SIDEBAR_MIN);
    expect(keyWidth('Home', 300)).toBe(SIDEBAR_MIN);
    expect(keyWidth('End', 300)).toBe(SIDEBAR_MAX);
    expect(keyWidth('a', 300)).toBeUndefined();
  });
});

describe('the sidebar state', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    set.mockClear();
  });

  it('loads the saved width and fold', async () => {
    const { sidebar, loadSidebar } = await import('./rail-state.svelte');
    loadSidebar({ sidebar_width: 333, sidebar_collapsed: true });
    expect([sidebar.width, sidebar.collapsed]).toEqual([333, true]);
    loadSidebar(null);
    expect([sidebar.width, sidebar.collapsed]).toEqual([SIDEBAR_DEFAULT, false]);
  });

  it('saves once after a run of changes, and the default as 0', async () => {
    const { sidebar, loadSidebar, setSidebarWidth, toggleSidebar } = await import('./rail-state.svelte');
    loadSidebar(null);
    setSidebarWidth(260);
    setSidebarWidth(300);
    setSidebarWidth(9999);
    vi.advanceTimersByTime(500);
    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenLastCalledWith(SIDEBAR_MAX, false);
    toggleSidebar();
    setSidebarWidth(SIDEBAR_DEFAULT);
    vi.advanceTimersByTime(500);
    expect(set).toHaveBeenLastCalledWith(0, true);
    expect(sidebar.collapsed).toBe(true);
  });

  it('a fold ends a peek', async () => {
    const { sidebar, setSidebarCollapsed } = await import('./rail-state.svelte');
    sidebar.peeking = true;
    setSidebarCollapsed(false);
    expect(sidebar.peeking).toBe(false);
  });
});
