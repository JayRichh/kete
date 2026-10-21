// Workshop's rules held over the views 0.60 added, read as written: square
// corners and no shadows, no em dash in their words, and ember only for
// what needs you, so an error never wears it.

import { describe, expect, it } from 'vitest';

// From disk, as look-contrast.test.ts reads theme.css (the type check has
// no Node types, so the module is named by a variable).
const fsModule = 'node:fs';
const { readFileSync } = (await import(/* @vite-ignore */ fsModule)) as { readFileSync: (path: URL, encoding: 'utf8') => string };

const FILES = [
  'studio/StudioView.svelte',
  'studio/board/Board.svelte',
  'studio/board/Dot.svelte',
  'studio/queue/AutonomyControl.svelte',
  'studio/queue/QueueHeader.svelte',
  'gauntlet/GauntletView.svelte',
  'triage/TriageStrip.svelte',
  'roles/RoleChip.svelte',
  'roles/RolePicker.svelte',
  'orchestrator/RenewForm.svelte',
  'orchestrator/RenewNotice.svelte',
  'ledger/SpendLine.svelte',
  'state/StateMap.svelte',
];

const read = (file: string) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8');

describe('the 0.60 views keep Workshop', () => {
  for (const file of FILES) {
    it(file, () => {
      const text = read(file);
      expect(text, 'an em dash').not.toContain('—');
      expect(text.match(/\b(shadow|rounded)(-[a-z0-9]+)?\b/g)?.filter((c) => c !== 'rounded-none') ?? [], 'shadows or round corners').toEqual([]);
      expect(text.match(/error[^}\n]*\?\s*'[^']*ember/g) ?? [], 'an error in ember').toEqual([]);
    });
  }

  it('the queue header shows a failed read in the fail tone', () => {
    expect(read('studio/queue/QueueHeader.svelte')).toContain("q.error ? 'text-fail'");
  });
});
