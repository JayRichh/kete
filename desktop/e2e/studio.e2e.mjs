// The studio board on the real window: a fixture repo with a studio
// manifest, a DEPARTMENTS-style table and an inbox of notes. The Studio
// page shows one column per department with its queue count, the What
// needs you strip names the note to Jay, and a note click opens that note
// in the file view. Read-only: the board writes nothing.
// `node --test --test-concurrency=1 e2e/studio.e2e.mjs`.

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

/** An inbox note: frontmatter from `fm`, then `body`. */
function note(fm, body) {
  const head = Object.entries(fm).map(([k, v]) => `${k}: ${v}`);
  return ['---', ...head, '---', body, ''].join('\n');
}

const MANIFEST = `version = 1
inbox = "notes/inbox"
departments = "docs/DEPARTMENTS.md"
`;

// A heading, a paragraph, the departments table, then a second table the
// board must ignore. Animation's parenthetical is dropped from its label.
const DEPARTMENTS = `# Departments

Every aspect of the game has one department.

| Department | Owns | Review rubric |
|---|---|---|
| Audio | music and sound | bands |
| Art and 3D | models and textures | review |
| Animation (sub-department of Art and 3D) | rigs and clips | review |

| Second | table |
|---|---|
| Not a department | ignored |
`;

const BODY = 'Shimmer pad under the title card.';

/** The Studio page, on the repo the rail names. */
async function toStudio(d) {
  // The rail learns studio repos once the projects change: click waits for it.
  await d.click('[data-rail="studio"]');
  await d.el('[data-studio-view]');
}

/** The board's column slugs on screen, in order. */
const columns = (d) =>
  d.wd.exec(`return [...document.querySelectorAll('[data-studio-board] [data-board-column]')].map((e) => e.getAttribute('data-board-column'));`);

/** The text of `css` inside the column `slug`, trimmed ('' when absent). */
const inColumn = (d, slug, css) =>
  d.wd.exec(
    `const [slug, css] = arguments;
     const col = document.querySelector('[data-board-column="' + slug + '"]');
     return (col?.querySelector(css)?.innerText ?? '').trim();`,
    slug,
    css,
  );

flow('the board shows a column per department with its queue count and opens a note in the file view', async (d) => {
  const repo = d.makeRepo('board-proj');
  put(repo, {
    '.kete/studio.toml': MANIFEST,
    'docs/DEPARTMENTS.md': DEPARTMENTS,
    'notes/inbox/audio-1.md': note(
      { id: 'audio-1', status: 'open', to: 'audio', about: 'Audio', priority: 'high', title: 'Mix the title theme', at: '2026-10-20T09:00:00Z' },
      BODY,
    ),
    'notes/inbox/audio-2.md': note(
      { id: 'audio-2', status: 'open', to: 'audio', about: 'Audio', priority: 'normal', title: 'Trim the footstep loops', at: '2026-10-19T09:00:00Z' },
      'Cut the tails.',
    ),
    'notes/inbox/art-1.md': note(
      { id: 'art-1', status: 'open', to: 'art-and-3d', about: 'Art and 3D', priority: 'normal', title: 'Retopo the lighthouse', at: '2026-10-18T09:00:00Z' },
      'Keep it under budget.',
    ),
    // Done: not a queue item.
    'notes/inbox/done-1.md': note(
      { id: 'done-1', status: 'done', to: 'audio', about: 'Audio', priority: 'normal', title: 'Old audio job', at: '2026-10-01T09:00:00Z' },
      'Finished.',
    ),
    // To Jay: the strip, not a department's queue.
    'notes/inbox/jay-1.md': note(
      { id: 'jay-1', status: 'open', to: 'jay', about: 'Audio', priority: 'normal', title: 'Pick the credits track', at: '2026-10-20T10:00:00Z' },
      'Two candidates.',
    ),
  });
  await d.addRepo(repo);
  await toStudio(d);
  await d.el('[data-studio-board]');

  // One column per department, in the table's order; no sessions, so no
  // Unassigned. The second table gives none.
  await d.waitFor('the three columns', async () => (await columns(d)).length === 3);
  assert.deepEqual(await columns(d), ['audio', 'art-and-3d', 'animation']);

  // Open notes to a department, a done note not counted.
  await d.waitFor('the audio queue count', async () => (await inColumn(d, 'audio', '[data-board-queue-count]')) === '2');
  assert.equal(await inColumn(d, 'art-and-3d', '[data-board-queue-count]'), '1');

  // The note to Jay is on the strip.
  const strip = await d.el('[data-board-needs-you]', 'Pick the credits track');
  assert.ok(strip, 'the strip names the note to Jay');

  // A click on the first audio note (high priority) raises the file view on it.
  await d.click('[data-board-note="audio-1"]');
  const tab = '[role="dialog"][aria-label="Opened files"] [role="tab"][aria-selected="true"]';
  await d.el(tab, 'audio-1.md');
  // The file's own text, read in the sheet (the board holds titles, never bodies).
  await d.waitFor('the note body in the file view', () =>
    d.wd.exec(
      `return (document.querySelector('[role="dialog"][aria-label="Opened files"]')?.innerText ?? '').replace(/\\u00A0/g, ' ').includes(arguments[0]);`,
      BODY,
    ),
  );
});

flow('a repo with studio mode on and no manifest shows the empty state naming the manifest', async (d) => {
  const repo = d.makeRepo('bare-proj');
  // Studio mode on for this machine only: an entry in config.local.json, no
  // `.kete/studio.toml`. StudioView lists a repo with a manifest or an entry.
  fs.writeFileSync(
    path.join(d.dataDir, 'config.local.json'),
    JSON.stringify({ studio: { [repo.replace(/\\/g, '/')]: { studio_inbox: '', autonomy: 'A1', queue_exclude: [] } } }),
  );
  await d.addRepo(repo);
  await toStudio(d);
  const empty = await d.el('[data-board-empty]');
  assert.ok(empty);
  await d.waitFor('the empty state naming the manifest', () =>
    d.wd.exec(`return (document.querySelector('[data-board-empty]')?.innerText ?? '').includes('.kete/studio.toml');`),
  );
  assert.equal(await d.wd.exec(`return document.querySelectorAll('[data-board-column]').length;`), 0);
});
