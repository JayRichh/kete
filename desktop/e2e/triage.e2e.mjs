// The triage strip, on the real binaries: the built window over a real
// daemon, the scripted fake agent as the orchestrator, a temp repo whose
// committed manifest names its inbox. `npm run e2e` (Windows).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Desktop, Key, git } from './harness.mjs';

const utcDate = () => new Date().toISOString().slice(0, 10);

test('triage strip files a note under a fake orchestrator', { timeout: 180_000 }, async () => {
  const d = await Desktop.open();
  try {
    const repo = d.makeRepo('triage-proj');
    // The manifest names the inbox; committed, so the repo is clean.
    fs.mkdirSync(path.join(repo, '.kete'), { recursive: true });
    fs.writeFileSync(path.join(repo, '.kete', 'studio.toml'), 'version = 1\ninbox = "notes/inbox"\n');
    git(repo, 'add', '.');
    git(repo, 'commit', '-q', '-m', 'studio manifest');
    git(repo, 'push', '-q', 'origin', 'main');
    await d.addRepo(repo);

    // The fake agent as the repo's orchestrator (the harness has no helper
    // for it): the window's own command, then the rail's Orchestrator.
    const project = (await d.invoke('get_state')).projects[0].id;
    const orch = await d.invoke('start_orchestrator', { spec: { kind: 'claude', project } });
    await d.waitFor('the orchestrator in the model', async () => (await d.agents()).some((a) => a.id === orch));
    await d.click('[data-target="orchestrator:rail"]');
    await d.el('[data-testid=triage-strip]');

    // Typed and sent through WebDriver, never the OS keyboard. The date is
    // taken either side of the send in case UTC midnight falls between.
    const before = utcDate();
    await d.type('[data-testid=triage-input]', 'storm sky bands at dusk');
    await d.press(Key.Enter);

    // The fake orchestrator never answers a second turn, so the route line
    // is not waited for: the note on disk is the whole assertion.
    const inbox = path.join(repo, 'notes', 'inbox');
    const slug = 'storm-sky-bands-at-dusk';
    const file = await d.waitFor('the note in the inbox', () => {
      const days = new Set([before, utcDate()]);
      return [...days].map((day) => path.join(inbox, `${day}-${slug}.md`)).find((f) => fs.existsSync(f));
    });
    const text = fs.readFileSync(file, 'utf8');
    assert.match(text, /^status: new$/m);
    assert.match(text, /^to: triage$/m);
  } finally {
    await d.close();
  }
});
