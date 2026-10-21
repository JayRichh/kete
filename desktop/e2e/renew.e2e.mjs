// Orchestrator renewal from the window (K9): the session menu's Renew
// orchestrator opens the form; a note the daemon takes ends in the notice
// and a new orchestrator; one it refuses shows inline. The built window over
// a real daemon, a scripted fake agent as Claude (harness.mjs). `npm run e2e`.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { Desktop, Key } from './harness.mjs';

/** One window per test, closed (driver, daemon, temp root) however it ends. */
function flow(name, options, body) {
  test(name, { timeout: 180_000 }, async () => {
    const d = await Desktop.open(options);
    try {
      await body(d);
    } finally {
      await d.close();
    }
  });
}

const FORM = '[role="dialog"][aria-labelledby="renew-title"]';
const card = (id) => `[data-target="agent:${id}"]`;
const orchestrator = async (d) => (await d.agents()).find((a) => a.role === 'orchestrator' && !a.archived);

/** A repo in the window, an orchestrator started in its root checkout (so
 *  a note inside the repo is one the daemon allows), and the board up with
 *  its card. Resolves to the repo and the orchestrator. */
async function withOrchestrator(d) {
  const repo = d.makeRepo('renew-proj');
  await d.addRepo(repo);
  const project = (await d.invoke('get_state')).projects[0].id;
  await d.invoke('start_orchestrator', { spec: { kind: 'claude', project, replace: false } });
  const old = await d.waitFor('the orchestrator in the model', () => orchestrator(d));
  await d.click('[aria-label="Home view"] button', 'Board');
  await d.el(card(old.id));
  return { repo, old };
}

/** The session menu of the orchestrator's card, its Renew orchestrator row
 *  clicked, and the form up with the focus on the note field. */
async function openForm(d, old) {
  await d.contextClick(card(old.id));
  const rows = await d.menu(`session ${old.name}`);
  assert.ok(rows.some((r) => r.label.startsWith('Renew orchestrator')), 'the session menu has Renew orchestrator');
  await d.click('[role="menu"] [role^="menuitem"]', 'Renew orchestrator');
  await d.el(`${FORM} input[aria-label="Handover note"]`);
  await d.waitFor('the focus on the note field', () =>
    d.wd.exec(`return document.activeElement?.getAttribute('aria-label') === 'Handover note';`),
  );
}

const submit = (d) => d.click(`${FORM} button`, 'Renew');
const formGone = (d) => d.waitFor('the form closed', async () => !(await d.wd.exec(`return !!document.querySelector(arguments[0]);`, FORM)));

flow('Renew orchestrator opens the form; the daemon’s refusal and a missing note show inline; Escape closes', {}, async (d) => {
  const { old } = await withOrchestrator(d);
  await openForm(d, old);

  // Nothing typed: the form says so and sends nothing.
  await submit(d);
  await d.waitForText('Name the handover note first.');

  // A relative path is refused in the window, before the daemon.
  await d.type(`${FORM} input[aria-label="Handover note"]`, 'notes\\handover.md');
  await submit(d);
  await d.waitForText('The note path must be absolute');

  // A note outside the data dir’s orchestrator folder and every repo: the
  // daemon refuses, its words land in the form, and the form stays up.
  await d.press(Key.Escape);
  await formGone(d);
  await openForm(d, old);
  const outside = path.join(d.root, 'outside.md');
  fs.writeFileSync(outside, 'not an allowed place\n');
  await d.type(`${FORM} input[aria-label="Handover note"]`, outside);
  await submit(d);
  const refusal = await d.waitFor('the daemon’s refusal in the form', () =>
    d.wd.exec(`return document.querySelector(arguments[0] + ' [role="alert"]')?.innerText || null;`, FORM),
  );
  assert.ok(!/absolute|Name the handover/.test(refusal), `the daemon refused, not the window: ${refusal}`);
  assert.equal((await orchestrator(d)).id, old.id, 'the old orchestrator is still in charge');

  await d.press(Key.Escape);
  await formGone(d);
});

flow('Renew orchestrator with a note: the toast, the notice naming the note, and a new orchestrator', {}, async (d) => {
  const { repo, old } = await withOrchestrator(d);
  const note = path.join(repo, 'handover.md');
  fs.writeFileSync(note, '# Handover\n\nPick up the queue where I left it.\n');

  await openForm(d, old);
  await d.type(`${FORM} input[aria-label="Handover note"]`, note);
  // Renew now: the old session is idle, nothing to wait for.
  await d.click(`${FORM} label`, 'Renew now');
  await submit(d);
  await d.waitForText('Renewal scheduled');
  await formGone(d);

  // Done: the notice names the note and offers it.
  await d.waitForText('Orchestrator renewed from handover.md');
  await d.el('button', 'Open note');

  // The model: a different session holds the role, the old one is archived.
  const now = await d.waitFor('a new orchestrator', async () => {
    const o = await orchestrator(d);
    return o && o.id !== old.id ? o : null;
  });
  const gone = (await d.agents()).find((a) => a.id === old.id);
  assert.equal(gone?.archived, true, 'the old orchestrator is archived');
  await d.el(card(now.id));

  // Dismiss takes the notice away.
  await d.click('button[aria-label="Dismiss"]');
  await d.waitForGone('Orchestrator renewed from handover.md');
});
