import { describe, expect, it } from 'vitest';
import { moveQuestion, moveReport, oneAtATime } from './move';

describe('moveReport', () => {
  it('says nothing for a clean checkout or one it could not read', () => {
    expect(moveReport({ changed: 0, untracked: 0, unpushed: 0 }, 'main')).toBeNull();
    expect(moveReport({ changed: 0, untracked: 0, unpushed: null }, 'main')).toBeNull();
    expect(moveReport(null, 'main')).toBeNull();
  });

  it('counts untracked files as changed and names the branch', () => {
    expect(moveReport({ changed: 2, untracked: 1, unpushed: 0 }, 'feature-x')).toBe('3 changed files stay in feature-x.');
    expect(moveReport({ changed: 1, untracked: 0, unpushed: null }, 'feature-x')).toBe('1 changed file stays in feature-x.');
  });

  it('joins files and commits', () => {
    expect(moveReport({ changed: 3, untracked: 0, unpushed: 2 }, 'feature-x')).toBe(
      '3 changed files and 2 unpushed commits stay in feature-x.',
    );
    expect(moveReport({ changed: 0, untracked: 0, unpushed: 1 }, 'b')).toBe('1 unpushed commit stays in b.');
  });
});

describe('moveQuestion', () => {
  const dirty = { state: { changed: 1, untracked: 0, unpushed: 0 }, timed_out: false };
  const clean = { state: { changed: 0, untracked: 0, unpushed: null }, timed_out: false };

  it('asks with the report for a dirty checkout', () => {
    expect(moveQuestion(dirty, 'feat', 'tidy', 'main')).toBe('1 changed file stays in feat. Move "tidy" to main anyway?');
  });

  it('asks anyway when the read ran out of time, and says so', () => {
    expect(moveQuestion({ state: null, timed_out: true }, 'feat', 'tidy', 'main')).toBe(
      'Could not read what stays behind in feat (git took too long). Move "tidy" to main anyway?',
    );
  });

  it('moves a clean or unreadable checkout at once unless the caller asks its own question', () => {
    expect(moveQuestion(clean, 'feat', 'tidy', 'main')).toBeNull();
    expect(moveQuestion({ state: null, timed_out: false }, 'feat', 'tidy', 'main')).toBeNull();
    expect(moveQuestion(null, 'feat', 'tidy', 'main')).toBeNull();
    expect(moveQuestion(clean, 'feat', 'tidy', 'main', 'Move it?')).toBe('Move it?');
  });
});

describe('oneAtATime', () => {
  it('starts no second run for a key while one is out, and frees the key after', async () => {
    const guard = oneAtATime<string>();
    let release!: (v: string) => void;
    let runs = 0;
    const first = guard('a1', () => {
      runs += 1;
      return new Promise<string>((r) => (release = r));
    });
    expect(await guard('a1', async () => (runs += 1, 'again'))).toBeNull();
    expect(await guard('a2', async () => 'other')).toBe('other');
    release('done');
    expect(await first).toBe('done');
    expect(runs).toBe(1);
    expect(await guard('a1', async () => 'later')).toBe('later');
  });

  it('frees the key when the run fails', async () => {
    const guard = oneAtATime<string>();
    await expect(guard('a1', async () => Promise.reject(new Error('no')))).rejects.toThrow('no');
    expect(await guard('a1', async () => 'ok')).toBe('ok');
  });
});
