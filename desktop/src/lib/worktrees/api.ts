// The Rust side's worktree, branch and pull request commands
// (src-tauri branches.rs, pr_poll.rs).

import { invoke } from '@tauri-apps/api/core';

/** What deleting a checkout would throw away. */
export interface CheckoutState {
  /** Tracked files changed, staged or not. */
  changed: number;
  untracked: number;
  /** Commits on HEAD no remote-tracking branch holds; null with no remote. */
  unpushed: number | null;
}

/** One branch the root checkout could switch to. */
export interface Branch {
  /** `feature-x`, or `origin/feature-x` for a remote branch. */
  name: string;
  remote: boolean;
  current: boolean;
  /** The other checkout holding this branch: git refuses it there. */
  checked_out_at: string | null;
  /** Unix seconds. */
  committed: number;
  subject: string;
}

/** A checkout as the worktree rows show it (src-tauri git.rs `WorktreeGit`). */
export interface WorktreeGit {
  changed: number;
  untracked: number;
  /** `origin/feature-x`; null for a branch with no upstream. */
  upstream: string | null;
  /** Against the upstream; null with none. */
  ahead: number | null;
  behind: number | null;
  /** HEAD's committer date, unix seconds; 0 before the first commit. */
  committed: number;
  /** Commits on the branch since its base branch's merge-base; 0 on the
   *  base branch or with none. */
  ahead_of_base: number;
  /** The checkout is on the base branch itself. */
  on_default_branch: boolean;
}

export interface Change {
  xy: string;
  path: string;
}

export type Carry =
  | { kind: 'ask' }
  | { kind: 'stash' }
  | { kind: 'bring' }
  | { kind: 'discard'; keys: string[] }
  | { kind: 'create' };

export type Outcome =
  | { kind: 'switched'; branch: string; note: string | null }
  | { kind: 'dirty'; files: Change[]; keys: string[] }
  | { kind: 'failed'; message: string }
  | { kind: 'stopped'; message: string };

export const worktreeApi = {
  /** Read fresh from config.json at delete time. */
  settings: () => invoke<{ delete_empty_worktree: boolean }>('worktree_settings'),
  checkoutState: (worktree: string) => invoke<CheckoutState>('checkout_state', { worktree }),
  /** Each named checkout's git state; one that can't be read is left out. */
  git: (worktrees: string[]) => invoke<Record<string, WorktreeGit>>('worktree_git', { worktrees }),
  /** A metadata-only fingerprint per checkout (src-tauri git.rs `git_stamp`). */
  stamps: (worktrees: string[]) => invoke<Record<string, string>>('git_stamps', { worktrees }),
  /** Open the checkout's folder in the file manager. */
  reveal: (worktree: string) => invoke<void>('worktree_reveal', { worktree }),
  /** Wake the PR poll now. */
  refreshPrs: () => invoke<void>('refresh_prs'),
  branchList: (worktree: string) => invoke<{ branches: Branch[]; changes: number | null }>('branch_list', { worktree }),
  /** `git fetch --all`; true when it worked. */
  branchFetch: (worktree: string) => invoke<boolean>('branch_fetch', { worktree }),
  branchSwitch: (worktree: string, target: { name: string; remote: boolean }, carry: Carry) =>
    invoke<Outcome>('branch_switch', { worktree, target, carry }),
};
