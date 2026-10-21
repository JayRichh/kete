import { describe, expect, it } from 'vitest';
import { VIEWS, createNav } from '../shell/history.svelte';
import { gauntletItem, parseGauntletItem, showStudioRail, studioOnly, type StudioRepo } from './types';

const repo = (over: Partial<StudioRepo>): StudioRepo => ({
  repo: 'p1',
  name: 'my-game',
  manifest: false,
  enabled: false,
  autonomy: null,
  ...over,
});

describe('studio views', () => {
  it('View includes studio and gauntlet', () => {
    expect(VIEWS).toContain('studio');
    expect(VIEWS).toContain('gauntlet');
  });

  it('the nav can stand on both', () => {
    const n = createNav();
    n.go({ view: 'studio', repo: 'p1' });
    expect(n.current.view).toBe('studio');
    n.go({ view: 'gauntlet', repo: 'p1', item: gauntletItem('art', 'v-3') });
    expect(n.current.item).toBe('art/v-3');
  });

  it('a gauntlet item is <kind>/<id>', () => {
    expect(parseGauntletItem('art/v-3')).toEqual({ kind: 'art', id: 'v-3' });
    expect(parseGauntletItem('art/a/b')).toEqual({ kind: 'art', id: 'a/b' });
    expect(parseGauntletItem('art')).toBeNull();
    expect(parseGauntletItem('/x')).toBeNull();
    expect(parseGauntletItem('x/')).toBeNull();
    expect(parseGauntletItem(undefined)).toBeNull();
  });
});

describe('studio repos', () => {
  it('keeps manifest-or-enabled rows only', () => {
    const rows = [
      repo({ repo: 'a' }),
      repo({ repo: 'b', manifest: true }),
      repo({ repo: 'c', enabled: true, autonomy: 'A1' }),
      repo({ repo: 'd', manifest: true, enabled: true }),
    ];
    expect(studioOnly(rows).map((r) => r.repo)).toEqual(['b', 'c', 'd']);
    expect(studioOnly([])).toEqual([]);
  });

  it('the rail entries are hidden with no studio repos', () => {
    expect(showStudioRail([])).toBe(false);
    expect(showStudioRail(studioOnly([repo({})]))).toBe(false);
    expect(showStudioRail(studioOnly([repo({ manifest: true })]))).toBe(true);
  });
});
