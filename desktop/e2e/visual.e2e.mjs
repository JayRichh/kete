// The 0.60 surfaces at the widths the window supports (640, 900 and 1280
// CSS px) in the light and the dark theme, on the real window: a studio
// repo with departments, an inbox, verdicts, a state map, role cards and an
// orchestrator; then a plain repo, where none of it may show. Each surface
// is checked in the page (nothing wider than the window, no em dash, every
// visible control with a name) and photographed into
// `.superpowers/sdd/2026-10-21-release-060/visual/` (or `KETE_VISUAL_DIR`)
// for a person to look at. Opt-in: `node --test --test-concurrency=1
// e2e/visual.e2e.mjs`.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Desktop, Key, git } from './harness.mjs';
import { E2E } from './paths.mjs';

const OUT = process.env.KETE_VISUAL_DIR ?? path.resolve(E2E, '..', '..', '.superpowers', 'sdd', '2026-10-21-release-060', 'visual');
const WIDTHS = [640, 900, 1280];
const HEIGHT = 820;
const PROMPT = 'form[aria-label="Start a session"] textarea[aria-label="Prompt"]';
const FORM = '[role="dialog"][aria-labelledby="renew-title"]';

/** Write `files` (repo-relative path -> text or bytes) under `repo`. */
function put(repo, files) {
  for (const [rel, body] of Object.entries(files)) {
    const file = path.join(repo, rel);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, body);
  }
}

const note = (fm, body) => ['---', ...Object.entries(fm).map(([k, v]) => `${k}: ${v}`), '---', body, ''].join('\n');
const node = (id, kind, label, more = {}) => ({ id, kind, label, status: '', owner: null, changed_at: null, questions: 0, links: [], ...more });

const STUDIO = {
  '.kete/studio.toml': `version = 1
inbox = "notes/inbox"
departments = "docs/DEPARTMENTS.md"
signoffs = "reviews/signoffs"
evidence = "reviews/evidence"
studio_url = "http://127.0.0.1:5173"

[links]
art = "/art?id={id}"

[kinds]
art = "art-and-3d"
sound = "audio"
`,
  '.kete/roles.toml': `version = 1

[roles.runner]
label = "Runner"
model = "haiku"
department = "audio"
tier = 3
permission = "allowlist"
prompt = "Run the audio queue."
`,
  'docs/DEPARTMENTS.md': `| Department | Owns |
|---|---|
| Audio | music and sound |
| Art and 3D | models and textures |
| Animation (sub-department of Art and 3D) | rigs and clips |
`,
  'notes/inbox/audio-1.md': note({ id: 'audio-1', status: 'open', to: 'audio', about: 'Audio', priority: 'high', title: 'Mix the title theme', at: '2026-10-20T09:00:00Z' }, 'Shimmer pad.'),
  'notes/inbox/art-1.md': note({ id: 'art-1', status: 'open', to: 'art-and-3d', about: 'Art and 3D', priority: 'normal', title: 'Retopo the lighthouse', at: '2026-10-18T09:00:00Z' }, 'Under budget.'),
  'notes/inbox/jay-1.md': note({ id: 'jay-1', status: 'open', to: 'jay', about: 'Audio', priority: 'normal', title: 'Pick the credits track', at: '2026-10-20T10:00:00Z' }, 'Two candidates.'),
  'reviews/signoffs/art/C02.json': JSON.stringify({ id: 'C02', verdict: 'split', by: 'second-reviewer', at: '2026-10-19', evidence: ['EV1'], tags: ['banding', 'sky'], note: 'Banding in the sky.' }),
  'reviews/signoffs/art/C05.json': JSON.stringify({ verdict: 'approve', at: '2026-10-18' }),
  'reviews/signoffs/sound/tui.json': JSON.stringify({ id: 'tui', verdict: 'rejected', reviewer: 'Jay', date: '2026-10-19', note: 'The TUI call is too long.' }),
  'reviews/evidence/EV1/notes.txt': 'seen on the dusk sky\n',
  '.kete/state-map.json': JSON.stringify({
    repo: 'C:/Code/game',
    built_at: Date.now(),
    nodes: [node('dept:audio', 'department', 'Audio'), node('SYS-1', 'system', 'Music', { status: 'building' }), node('D-7', 'decision', 'Loudness', { status: 'open', questions: 2 })],
    edges: [
      { from: 'dept:audio', to: 'SYS-1', kind: 'owns' },
      { from: 'SYS-1', to: 'D-7', kind: 'owns' },
    ],
    note: null,
  }),
};

/** Two profiles, one reserved: the Accounts view's Reserve for me. */
const PROFILES = [
  { name: 'work', harness: 'claude', reserved: true },
  { name: 'home', harness: 'claude' },
];

/** The window's inner width set to `w` CSS px: the outer rect corrected by
 *  what the frame and scaling take. Resolves to the width it reached. */
async function resize(d, w) {
  let outer = w;
  for (let i = 0; i < 4; i++) {
    await d.wd.call('POST', '/window/rect', { width: Math.round(outer), height: HEIGHT });
    await d.settle();
    const inner = await d.wd.exec('return window.innerWidth;');
    if (inner === w) return inner;
    outer += w - inner;
  }
  return d.wd.exec('return window.innerWidth;');
}

/** What the page says is wrong with `css` as drawn: wider than the window,
 *  an em dash in its words, a visible control with no name. */
function problems(d, css) {
  return d.wd.exec(
    `const el = document.querySelector(arguments[0]);
     if (!el) return ['no ' + arguments[0]];
     const out = [];
     const doc = document.documentElement;
     if (doc.scrollWidth > innerWidth + 1) {
       // Name what sticks out: the outermost element past the edge that no
       // scrolling or clipping box holds in.
       // (An absolute box escapes a scroller that is not positioned.)
       const held = (e) => { if (['absolute', 'fixed'].includes(getComputedStyle(e).position)) return false; for (let p = e.parentElement; p; p = p.parentElement) { const o = getComputedStyle(p).overflowX; if (o !== 'visible') return p !== doc && p !== document.body; } return false; };
       const past = [...document.body.querySelectorAll('*')].filter((e) => e.getBoundingClientRect().right > innerWidth + 1 && !held(e));
       const top = past.filter((e) => !past.includes(e.parentElement)).slice(0, 2).map((e) => e.tagName.toLowerCase() + '.' + String(e.className).slice(0, 80) + ' right ' + Math.round(e.getBoundingClientRect().right));
       out.push('the page scrolls sideways: ' + doc.scrollWidth + ' > ' + innerWidth + ' (' + top.join('; ') + ')');
     }
     if (el.getBoundingClientRect().right > innerWidth + 1) out.push('runs past the window: ' + Math.round(el.getBoundingClientRect().right));
     // In words; a lone dash standing for no value in a cell is not copy.
     if (/[^\\s] ?\\u2014 ?[^\\s]/.test(el.innerText || '')) out.push('an em dash');
     for (const c of el.querySelectorAll('button, select, input, [role="radio"], [role="switch"], [role="tab"]')) {
       const r = c.getBoundingClientRect();
       if (!r.width || !r.height || getComputedStyle(c).visibility === 'hidden') continue;
       const name = c.getAttribute('aria-label') || c.title || (c.innerText || '').trim() || c.getAttribute('placeholder') || (c.labels && c.labels[0] && c.labels[0].innerText);
       if (!name) out.push('a ' + c.tagName.toLowerCase() + ' with no name: ' + c.outerHTML.slice(0, 120));
     }
     return out;`,
    css,
  );
}

/** Checks `css` and photographs the window as `<theme>/<width>-<name>.png`. */
async function shoot(d, theme, width, name, css, seen) {
  await d.wd.exec('return document.fonts.ready.then(() => true);');
  await d.settle();
  const bad = await problems(d, css);
  await d.screenshot(path.join(OUT, theme, `${width}-${name}.png`));
  seen.push({ theme, width, name, bad });
}

/** The Accounts view, through the palette: the rail's entry changes shape
 *  with the width. */
async function toAccounts(d) {
  await d.press('k', Key.Control);
  await d.type('[aria-label="Command palette"] input', 'Accounts');
  await d.press(Key.Enter);
  await d.el('section[aria-label="Accounts"]');
}

async function studioRepo(d) {
  const repo = d.makeRepo('game');
  put(repo, STUDIO);
  git(repo, 'add', '.');
  git(repo, 'commit', '-q', '-m', 'studio');
  git(repo, 'push', '-q', 'origin', 'main');
  await d.addRepo(repo);
  const project = (await d.invoke('get_state')).projects[0].id;
  await d.invoke('start_orchestrator', { spec: { kind: 'claude', project } });
  const orch = await d.waitFor('the orchestrator', async () => (await d.agents()).find((a) => a.role === 'orchestrator' && !a.archived));
  return { repo, orch };
}

for (const theme of ['light', 'dark']) {
  test(`the 0.60 surfaces at 640, 900 and 1280 px, ${theme}`, { timeout: 600_000 }, async () => {
    const d = await Desktop.open({ config: { desktop: { theme } } });
    const seen = [];
    try {
      assert.equal(await d.wd.exec('return document.documentElement.dataset.theme;'), theme);
      const { repo, orch } = await studioRepo(d);
      // A Runner session from the quick prompt, for the role chips.
      const role = 'form[aria-label="Start a session"] select[aria-label="Role"]';
      // The option row shows while the prompt has the focus.
      await d.click(PROMPT);
      await d.waitFor('the runner role offered', () => d.wd.exec(`return !!document.querySelector(arguments[0] + ' option[value="runner"]');`, role));
      await d.wd.exec(`const s = document.querySelector(arguments[0]); s.value = 'runner'; s.dispatchEvent(new Event('change', { bubbles: true }));`, role);
      const runner = await d.launch('mix the bells');
      for (const w of WIDTHS) {
        const width = await resize(d, w);
        assert.equal(width, w, `the window reaches ${w} px`);

        await d.click('[data-rail="studio"]');
        await d.el('[data-studio-board]');
        await d.el('[data-state-map]');
        await shoot(d, theme, w, 'studio', '[data-studio-view]', seen);
        await d.wd.exec(`document.querySelector('[data-state-map]').scrollIntoView();`);
        await shoot(d, theme, w, 'studio-state-map', '[data-studio-view]', seen);

        await d.click('[data-rail="gauntlet"]');
        await d.el('[data-gauntlet-row]');
        await shoot(d, theme, w, 'gauntlet', '[data-gauntlet-view]', seen);
        await d.click('[data-gauntlet-row="art/C02"]');
        await d.el('[data-gauntlet-evidence]');
        await shoot(d, theme, w, 'gauntlet-detail', '[data-gauntlet-view]', seen);

        await d.click('[data-target="orchestrator:rail"]');
        await d.el('[data-testid=triage-strip]');
        await shoot(d, theme, w, 'orchestrator-triage', '[data-testid=triage-strip]', seen);

        await d.click('[aria-label="Home view"] button', 'Board');
        await d.el(`[data-target="agent:${runner}"]`);
        await shoot(d, theme, w, 'board-role-chips', 'main', seen);

        // The orchestrator's menu is its header's, in the orchestrator view.
        await d.click('[data-target="orchestrator:rail"]');
        await d.contextClick(`[data-target="agent:${orch.id}"]`);
        await d.click('[role="menu"] [role^="menuitem"]', 'Renew orchestrator');
        await d.el(FORM);
        await shoot(d, theme, w, 'renew-form', FORM, seen);
        await d.press(Key.Escape);
        await d.click('[aria-label="Home view"] button', 'Board');

        await d.click(PROMPT);
        await d.el('form[aria-label="Start a session"] select[aria-label="Role"]');
        await shoot(d, theme, w, 'quick-prompt-role', 'form[aria-label="Start a session"]', seen);
        // The option row: nothing wraps past its height, no select squeezed shut.
        const row = await d.wd.exec(`const r = document.querySelector('[aria-label="Launch options"]');
          const out = [];
          for (const c of r.children) if (c.getBoundingClientRect().width && c.scrollHeight > r.clientHeight + 1) out.push('wraps: ' + c.innerText.replace(/\s+/g, ' ').slice(0, 40));
          for (const s of r.querySelectorAll('select')) if (s.getBoundingClientRect().width < 64) out.push('squeezed: ' + s.getAttribute('aria-label') + ' ' + Math.round(s.getBoundingClientRect().width));
          return out;`);
        seen.at(-1).bad.push(...row);

        await d.press('k', Key.Control);
        await d.type('[aria-label="Command palette"] input', 'studio');
        await d.waitForText('Studio board');
        await shoot(d, theme, w, 'palette-studio', '[aria-label="Command palette"]', seen);
        await d.press(Key.Escape);
      }

      // The renew notice, narrowest: a real renewal from a note in the repo.
      await resize(d, 640);
      const handover = path.join(repo, 'handover.md');
      fs.writeFileSync(handover, '# Handover\n\nPick up the queue.\n');
      await d.click('[data-target="orchestrator:rail"]');
      await d.contextClick(`[data-target="agent:${orch.id}"]`);
      await d.click('[role="menu"] [role^="menuitem"]', 'Renew orchestrator');
      await d.type(`${FORM} input[aria-label="Handover note"]`, handover);
      await d.click(`${FORM} label`, 'Renew now');
      await d.click(`${FORM} button`, 'Renew');
      await d.waitForText('Orchestrator renewed from handover.md');
      await shoot(d, theme, 640, 'renew-notice', '.fixed[role="status"]', seen);
    } finally {
      fs.mkdirSync(path.join(OUT, theme), { recursive: true });
      fs.writeFileSync(path.join(OUT, theme, 'checks.json'), JSON.stringify(seen, null, 2));
      await d.close();
    }
    const bad = seen.filter((s) => s.bad.length);
    assert.deepEqual(bad, [], 'every surface whole at every width');
  });
}

// Profiles change how a launch runs, so the reserved account has its own
// window with no sessions in it.
for (const theme of ['light', 'dark']) {
  test(`Reserve for me in Accounts at 640, 900 and 1280 px, ${theme}`, { timeout: 300_000 }, async () => {
    const d = await Desktop.open({ config: { desktop: { theme }, profiles: PROFILES } });
    const seen = [];
    try {
      for (const w of WIDTHS) {
        assert.equal(await resize(d, w), w);
        await toAccounts(d);
        await d.el('[role="switch"]', 'Reserve for me');
        await shoot(d, theme, w, 'accounts-reserve', 'section[aria-label="Accounts"]', seen);
      }
    } finally {
      fs.mkdirSync(path.join(OUT, theme), { recursive: true });
      fs.writeFileSync(path.join(OUT, theme, 'checks-accounts.json'), JSON.stringify(seen, null, 2));
      await d.close();
    }
    assert.deepEqual(seen.filter((s) => s.bad.length), [], 'Accounts whole at every width');
  });
}

test('with no studio repo: no Studio or Gauntlet on the rail, and the 0.59 views as they were', { timeout: 300_000 }, async () => {
  const d = await Desktop.open({ config: { desktop: { theme: 'dark' } } });
  const seen = [];
  try {
    await d.addRepo(d.makeRepo('plain'));
    for (const w of WIDTHS) {
      assert.equal(await resize(d, w), w);
      assert.equal(await d.wd.exec(`return !!document.querySelector('[data-rail="studio"], [data-rail="gauntlet"]');`), false, 'no studio rail entries');
      await d.el('form[aria-label="Start a session"]');
      assert.equal(await d.wd.exec(`return !!document.querySelector('form[aria-label="Start a session"] select[aria-label="Role"]');`), false, 'no role picker without roles');
      await shoot(d, 'plain', w, 'home', 'main', seen);
      await d.click('[data-target="orchestrator:rail"]');
      await d.waitForText('Start orchestrator');
      await shoot(d, 'plain', w, 'orchestrator-none', 'main', seen);
      await toAccounts(d);
      await shoot(d, 'plain', w, 'accounts', 'section[aria-label="Accounts"]', seen);
      await d.click('[aria-label="Home view"] button', 'Board');
    }
  } finally {
    fs.mkdirSync(path.join(OUT, 'plain'), { recursive: true });
    fs.writeFileSync(path.join(OUT, 'plain', 'checks.json'), JSON.stringify(seen, null, 2));
    await d.close();
  }
  assert.deepEqual(seen.filter((s) => s.bad.length), [], 'the 0.59 views whole at every width');
});
