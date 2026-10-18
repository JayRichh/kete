import { describe, expect, it } from 'vitest';
import { moveReport } from './move';

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
