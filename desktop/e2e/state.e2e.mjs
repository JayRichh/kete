// The state map on the real window (K10): a fixture repo with a studio
// manifest and a three-node `.kete/state-map.json` (a department owning a
// system owning a decision). The Studio page shows the map under the board:
// the department and its system as roots open, the decision once its system
// opens, and the selected node's line beside the tree. Read-only. Studio
// mode is off on this machine, so the daemon never rebuilds the file.
// `node --test --test-concurrency=1 e2e/state.e2e.mjs`.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Desktop } from './harness.mjs';

/** One window per test, closed (driver, daemon, temp root) however it ends. */
function flow(name, body) {
  test(name, { timeout: 180_000 }, async () => {
    const d = await Desktop.open();
    try {
      await body(d);
    } finally {
      await d.close();
    }
  });
}

/** Write `files` (repo-relative path -> text) under `repo`. */
function put(repo, files) {
  for (const [rel, body] of Object.entries(files)) {
    const file = path.join(repo, rel);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, body);
  }
}

const node = (id, kind, label, more = {}) => ({ id, kind, label, status: '', owner: null, changed_at: null, questions: 0, links: [], ...more });

const MAP = {
  repo: 'C:/Code/game',
  built_at: Date.now(),
  nodes: [
    node('dept:audio', 'department', 'Audio'),
    node('SYS-1', 'system', 'Music', { status: 'building' }),
    node('D-7', 'decision', 'Loudness', { status: 'open', questions: 2 }),
  ],
  edges: [
    { from: 'dept:audio', to: 'SYS-1', kind: 'owns' },
    { from: 'SYS-1', to: 'D-7', kind: 'owns' },
  ],
  note: null,
};

/** The tree's node ids on screen, in order. */
const rows = (d) =>
  d.wd.exec(`return [...document.querySelectorAll('[data-state-map] [data-state-node]')].map((e) => e.getAttribute('data-state-node'));`);

flow('a fake repo with three nodes renders in the state map tree', async (d) => {
  const repo = d.makeRepo('state-proj');
  put(repo, {
    '.kete/studio.toml': 'version = 1\n',
    '.kete/state-map.json': JSON.stringify(MAP),
  });
  await d.addRepo(repo);
  await d.click('[data-rail="studio"]');
  await d.el('[data-studio-view]');
  await d.el('[data-state-map]');

  // Roots start open, everything below closed: the decision waits.
  await d.waitFor('the department and its system', async () => (await rows(d)).length === 2);
  assert.deepEqual(await rows(d), ['dept:audio', 'SYS-1']);

  // Opening the system shows the third node under it.
  await d.click('[data-state-node="SYS-1"] button[aria-label="Open Music"]');
  await d.waitFor('the decision under its system', async () => (await rows(d)).length === 3);
  assert.deepEqual(await rows(d), ['dept:audio', 'SYS-1', 'D-7']);
  assert.equal(await d.wd.exec(`return document.querySelector('[data-state-node="D-7"]').getAttribute('aria-level');`), '3');

  // The selected node (the one just opened) is described beside the tree.
  await d.waitFor('the selected node line', () =>
    d.wd.exec(`return (document.querySelector('[data-state-detail]')?.innerText ?? '').includes('system SYS-1: Music (building)');`),
  );
});
