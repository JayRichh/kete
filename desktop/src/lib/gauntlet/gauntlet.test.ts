import { describe, expect, it } from 'vitest';
import {
  atLabel,
  filterVerdicts,
  firstLine,
  menuRowIds,
  openEvidenceWhy,
  toneClass,
  toneGlyph,
  toneLabel,
  verdictTime,
  verdictWords,
  wordLabel,
  NO_CHECKOUT,
  type EvidenceEntry,
  type Tone,
  type VerdictRow,
} from './gauntlet';

const v = (over: Partial<VerdictRow> = {}): VerdictRow => ({
  id: 'a',
  kind: 'qa',
  verdict: 'pass',
  by: null,
  at: null,
  note: null,
  tags: [],
  evidence: [],
  reconciled: null,
  path: '/r/a.md',
  modified: null,
  tone: 'plain',
  rel: 'a.md',
  reconciled_rel: null,
  ...over,
});

const HOUR = 3600e3;
const NOW = new Date(2026, 9, 22, 12, 0, 0).getTime();

describe('tones', () => {
  const tones: Tone[] = ['done', 'failed', 'needs_you', 'plain'];
  it('gives each tone its own glyph and label, so colour is never alone', () => {
    expect(new Set(tones.map(toneGlyph)).size).toBe(4);
    expect(new Set(tones.map(toneLabel)).size).toBe(4);
    expect(tones.map(toneGlyph)).toEqual(['\u2713', '\u2715', '\u25cf', '\u00b7']);
  });
  it('uses ember for needs_you and for nothing else', () => {
    expect(toneClass('needs_you')).toMatch(/ember/);
    for (const t of ['done', 'failed', 'plain'] as const) expect(toneClass(t)).not.toMatch(/ember/);
    expect(toneClass('done')).toBe('text-done');
    expect(toneClass('failed')).toBe('text-fail');
    expect(toneClass('plain')).toBe('text-muted');
  });
});

describe('verdictTime', () => {
  it('reads a bare date as local midnight', () => {
    expect(verdictTime(v({ at: '2026-10-19' }))).toBe(new Date(2026, 9, 19).getTime());
  });
  it('reads RFC3339', () => {
    expect(verdictTime(v({ at: '2026-10-19T03:04:05Z' }))).toBe(Date.UTC(2026, 9, 19, 3, 4, 5));
  });
  it('falls back to modified seconds when at is missing, blank or unreadable', () => {
    expect(verdictTime(v({ at: null, modified: 1_700_000_000 }))).toBe(1_700_000_000_000);
    expect(verdictTime(v({ at: '  ', modified: 5 }))).toBe(5000);
    expect(verdictTime(v({ at: 'last week', modified: 5 }))).toBe(5000);
  });
  it('is null with neither', () => {
    expect(verdictTime(v())).toBeNull();
    expect(verdictTime(v({ at: 'soon' }))).toBeNull();
  });
});

describe('filterVerdicts', () => {
  const rows = [
    v({ id: 'new', verdict: 'Pass', at: new Date(NOW - 2 * HOUR).toISOString() }),
    v({ id: 'week', verdict: 'fail', at: new Date(NOW - 3 * 24 * HOUR).toISOString() }),
    v({ id: 'old', verdict: 'pass', modified: (NOW - 30 * 24 * HOUR) / 1000 }),
    v({ id: 'undated', verdict: '' }),
  ];
  const ids = (r: VerdictRow[]) => r.map((x) => x.id);
  it('keeps every row for all words and all ages, the undated too', () => {
    expect(ids(filterVerdicts(rows, { word: null, age: 'all' }, NOW))).toEqual(['new', 'week', 'old', 'undated']);
  });
  it('matches the word case-insensitively', () => {
    expect(ids(filterVerdicts(rows, { word: 'PASS', age: 'all' }, NOW))).toEqual(['new', 'old']);
    expect(ids(filterVerdicts(rows, { word: 'Fail', age: 'all' }, NOW))).toEqual(['week']);
  });
  it('matches the blank word only against blank', () => {
    expect(ids(filterVerdicts(rows, { word: '', age: 'all' }, NOW))).toEqual(['undated']);
  });
  it('24h keeps only the last day, and drops rows with no time', () => {
    expect(ids(filterVerdicts(rows, { word: null, age: '24h' }, NOW))).toEqual(['new']);
  });
  it('7d keeps the last week, and drops rows with no time', () => {
    expect(ids(filterVerdicts(rows, { word: null, age: '7d' }, NOW))).toEqual(['new', 'week']);
  });
  it('combines the word and the age', () => {
    expect(ids(filterVerdicts(rows, { word: 'pass', age: '7d' }, NOW))).toEqual(['new']);
    expect(filterVerdicts(rows, { word: 'fail', age: '24h' }, NOW)).toEqual([]);
  });
  it('uses the modified time for a row with no at', () => {
    const r = [v({ id: 'm', modified: (NOW - HOUR) / 1000 })];
    expect(ids(filterVerdicts(r, { word: null, age: '24h' }, NOW))).toEqual(['m']);
  });
});

describe('words and notes', () => {
  it('lists the distinct words sorted, folding case', () => {
    const rows = [v({ verdict: 'pass' }), v({ verdict: 'Fail' }), v({ verdict: 'PASS' }), v({ verdict: '' })];
    expect(verdictWords(rows)).toEqual(['', 'Fail', 'pass']);
    expect(verdictWords([])).toEqual([]);
  });
  it('says a blank word as (none)', () => {
    expect(wordLabel('')).toBe('(none)');
    expect(wordLabel('pass')).toBe('pass');
  });
  it('takes the first non-empty line of a note', () => {
    expect(firstLine('  one  \ntwo')).toBe('one');
    expect(firstLine('\n\r\n  later\nmore')).toBe('later');
    expect(firstLine(null)).toBe('');
    expect(firstLine('')).toBe('');
  });
  it('shortens a stamp to the minute and leaves a date alone', () => {
    expect(atLabel(v({ at: '2026-10-19' }))).toBe('2026-10-19');
    expect(atLabel(v({ at: '2026-10-19T03:04:05Z' }))).toBe('2026-10-19 03:04');
    expect(atLabel(v())).toBe('');
  });
});

describe('the row menu', () => {
  const ev = ['e1'];
  it('has all four rows with Studio and evidence', () => {
    expect(menuRowIds(v({ evidence: ev }), true)).toEqual([
      'local:gauntlet.evidence',
      'local:gauntlet.studio',
      'local:gauntlet.copy_id',
      'local:gauntlet.copy_path',
    ]);
  });
  it('leaves Studio out for a kind without it', () => {
    expect(menuRowIds(v({ evidence: ev }), false)).toEqual(['local:gauntlet.evidence', 'local:gauntlet.copy_id', 'local:gauntlet.copy_path']);
  });
  it('leaves Show evidence out for a verdict with none', () => {
    expect(menuRowIds(v(), true)).toEqual(['local:gauntlet.studio', 'local:gauntlet.copy_id', 'local:gauntlet.copy_path']);
  });
  it('is the two copies alone with neither', () => {
    expect(menuRowIds(v(), false)).toEqual(['local:gauntlet.copy_id', 'local:gauntlet.copy_path']);
  });
});

describe('Open in Kete on evidence', () => {
  const file = (bytes: number): EvidenceEntry => ({ name: 'a.png', path: 'e/a.png', is_dir: false, bytes });
  it('opens a small file', () => expect(openEvidenceWhy(file(10), 'w1', 100, 'big')).toBeNull());
  it('greys a big file with the reason', () => expect(openEvidenceWhy(file(101), 'w1', 100, 'big')).toBe('big'));
  it('greys with no checkout', () => expect(openEvidenceWhy(file(10), null, 100, 'big')).toBe(NO_CHECKOUT));
  it('greys a folder', () => expect(openEvidenceWhy({ ...file(0), is_dir: true }, 'w1', 100, 'big')).toMatch(/folder/));
});
