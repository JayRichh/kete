// The gauntlet view on the real window: a fixture repo with a studio
// manifest, two verdict kinds (one reconciled pair) and an evidence folder.
// Read-only: Open in Studio and Show in Explorer are never clicked, since
// they would open a real browser or Explorer. `node --test --test-concurrency=1`.

import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Desktop } from './harness.mjs';

const hoursAgo = (h) => new Date(Date.now() - h * 3_600_000).toISOString();

/** Write `files` (repo-relative path -> text or bytes) under `repo`. */
function put(repo, files) {
  for (const [rel, body] of Object.entries(files)) {
    const file = path.join(repo, rel);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, body);
  }
}

const MANIFEST = `version = 1
signoffs = "reviews/signoffs"
evidence = "reviews/evidence"
studio_url = "http://127.0.0.1:5173"

[links]
art = "/art?id={id}"

[kinds]
art = "art"
`;

describe('the gauntlet view', { timeout: 240_000 }, () => {
  /** @type {Desktop} */
  let d;

  /** Whether `css` is in the page. */
  const has = (css) => d.wd.exec('return !!document.querySelector(arguments[0]);', css);

  /** The row ids on screen, in order. */
  const rows = () =>
    d.wd.exec(
      `return [...document.querySelectorAll('[data-gauntlet-view] [data-gauntlet-row]')].map((e) => e.getAttribute('data-gauntlet-row'));`,
    );

  /** Until the rows on screen are exactly `want`, in that order. */
  async function rowsAre(want) {
    await d.waitFor(`rows ${JSON.stringify(want)}`, async () => JSON.stringify(await rows()) === JSON.stringify(want));
  }

  /** Until `css` is gone. */
  const gone = (css) => d.waitFor(`${css} gone`, async () => !(await has(css)));

  before(
    async () => {
      d = await Desktop.open();
      const repo = d.makeRepo('gauntlet-proj');
      const s = 'reviews/signoffs';
      put(repo, {
        '.kete/studio.toml': MANIFEST,
        [`${s}/art/C02.json`]: JSON.stringify({
          id: 'C02',
          verdict: 'split',
          by: 'second-reviewer',
          at: hoursAgo(2),
          evidence: ['EV1'],
          tags: ['banding', 'sky'],
          note: 'Banding in the sky.\nThe second reviewer disagrees.',
        }),
        [`${s}/art/C02-reconciled.json`]: JSON.stringify({ verdict: 'approve' }),
        [`${s}/art/C05.json`]: JSON.stringify({
          verdict: 'approve',
          at: hoursAgo(72).slice(0, 10),
        }),
        // The real Studio app's sound shape: no `by` or `at`.
        [`${s}/sound/tui.json`]: JSON.stringify({
          id: 'tui',
          verdict: 'rejected',
          reviewer: 'Jay',
          date: '2026-10-19',
          note: 'The TUI call is too long.',
        }),
        'reviews/evidence/EV1/shot.png': Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        'reviews/evidence/EV1/notes.txt': 'seen on the dusk sky\n',
      });
      await d.addRepo(repo);
      // The window learns studio repos once the projects change: the rail
      // entry may lag the repo, so click waits for it.
      await d.click('[data-rail="gauntlet"]');
      await d.el('[data-gauntlet-view]');
    },
    { timeout: 120_000 },
  );

  after(async () => {
    await d?.close();
  });

  it('shows a tab for each kind, art first', async () => {
    await d.el('[data-gauntlet-kind="art"]');
    await d.el('[data-gauntlet-kind="sound"]');
    await d.el('[data-gauntlet-kind="art"][aria-selected="true"]');
  });

  it('lists a reconciled pair as one row, newest first', async () => {
    await rowsAre(['art/C02', 'art/C05']);
    assert.equal(await has('[data-gauntlet-row="art/C02-reconciled"]'), false);
  });

  it('narrows by age and brings the rows back', async () => {
    await d.click('[data-gauntlet-age="24h"]');
    await rowsAre(['art/C02']);
    await d.click('[data-gauntlet-age="all"]');
    await rowsAre(['art/C02', 'art/C05']);
  });

  it('narrows by verdict word', async () => {
    await d.click('[data-gauntlet-word="approve"]');
    await rowsAre(['art/C05']);
    await d.click('[data-gauntlet-word=""]');
    await rowsAre(['art/C02', 'art/C05']);
  });

  it('opens a verdict with its evidence and the Studio link', async () => {
    await d.click('[data-gauntlet-row="art/C02"]');
    await d.el('[data-gauntlet-detail]');
    await d.el('[data-gauntlet-evidence="reviews/evidence/EV1/notes.txt"]');
    await d.el('[data-gauntlet-evidence="reviews/evidence/EV1/shot.png"]');
    await d.el('[data-gauntlet-studio]');
  });

  it('shows no Studio link for a kind without a route', async () => {
    await d.click('[data-gauntlet-kind="sound"]');
    await d.el('[data-gauntlet-kind="sound"][aria-selected="true"]');
    await rowsAre(['sound/tui']);
    await d.click('[data-gauntlet-row="sound/tui"]');
    // The detail is the sound verdict's before the button is judged.
    await d.waitFor('the sound verdict in the detail', () =>
      d.wd.exec(`return (document.querySelector('[data-gauntlet-detail]')?.innerText ?? '').includes('The TUI call is too long.');`),
    );
    await gone('[data-gauntlet-studio]');
  });
});
