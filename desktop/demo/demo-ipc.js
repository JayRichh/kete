// Demo bridge for screenshots: stands in for the Tauri backend so the
// window renders in a plain browser with labelled synthetic data. Nothing
// here talks to a daemon, git or Azure DevOps. Loaded only by
// demo/index.html (`npx vite`, then open /demo/?view=tasks&theme=dark&start=1).
// Not part of the app: vite builds only the root index.html, and
// tsconfig and the encoding check cover src/ only.
//
// Query: view=tasks|branches, theme=light|dark, start=1 (open the Start
// dialog), offline=1 (tasks come back as a saved list), sel=<branch>,
// diff=1 (run Diff vs main on the selected branch), view=settings with
// tab=<id> (desktop, general, sessions, appearance, agents, project,
// experimental, hotkeys, presets, profiles); view=menu (a row's context
// menu), view=palette (q=<query>), view=help, view=quick (the quick prompt
// expanded); view=orchestrator (the orchestrator view), view=start-orchestrator
// (its start form), view=review (a ReviewOpened notice opened into the review
// sheet; tab=terminal|diff|history|pr), view=overview-cue (sessions the
// orchestrator started, in their repos), view=accounts (the rail's accounts,
// a needs-login row's command open), view=context (a session open: context
// bars in the tree and the session header); the board: view=board,
// board-review (a card's review, its diff against the base with the
// commits strip), board-ship (the PR form; ship_error=1 sends it into the
// sign-in error), board-merge (the merge confirm), board-verbs (the verbs
// menu), board-palette, board-scope. The settings rows are the real
// schema, exported
// by `cargo test -- --ignored write_demo_settings` in src-tauri.

const q = new URLSearchParams(location.search);
const now = Date.now();
const DAY = 86_400_000;
const iso = (d) => new Date(d).toISOString().slice(0, 10) + 'T00:00:00Z';
const today = Date.parse(new Date(now).toISOString().slice(0, 10) + 'T00:00:00Z');

// ---- daemon model: one ADO repo (synthetic), one plain repo ----
const model = {
  connected: true,
  error: null,
  daemon_pid: 4242,
  protocol_version: 53,
  runtime_dir: 'demo',
  projects: [
    { id: 'p1', name: 'ledger-api', repo_path: 'C:\\demo\\ledger-api', sort_order: 0 },
    { id: 'p2', name: 'storefront-web', repo_path: 'C:\\demo\\storefront-web', sort_order: 1 },
  ],
  worktrees: [
    { id: 'w1', project_id: 'p1', path: 'C:\\demo\\ledger-api', branch: 'main', is_main: true, sort_order: 0 },
    { id: 'w2', project_id: 'p1', path: 'C:\\demo\\ledger-api-worktrees\\gst-rounding', branch: 'jari/gst-rounding', is_main: false, sort_order: 1 },
    { id: 'w3', project_id: 'p2', path: 'C:\\demo\\storefront-web', branch: 'main', is_main: true, sort_order: 0 },
  ],
  agents: [
    {
      id: 'a1', worktree_id: 'w2', name: 'gst-rounding', status: 'running', archived: false, unseen: false,
      status_changed_at: now - 120_000, kind: 'claude', custom_harness: null, model: null, profile: 'work',
      alive: true, recent_prompts: [{ text: 'Round GST per invoice', submitted_at: now - 600_000 }],
      role: 'worker', sort_order: 0, parent: null,
    },
  ],
  terminals: [],
  ports: [],
  prs: {},
  subagents: [],
};

// Wave 2 shots (menu, palette, help, quick) want a fuller overview: a
// session per state, a nested one, a terminal and a PR.
const ORCH_VIEWS = ['orchestrator', 'start-orchestrator', 'review', 'overview-cue'];
const ORCH = ORCH_VIEWS.includes(q.get('view'));
// The board shots (view=board, board-review, board-ship, board-scope):
// every lane, a PR in Ship, commits past the base, the ship sheet, a scope.
const BOARD = ['board', 'board-review', 'board-ship', 'board-scope', 'board-merge', 'board-verbs', 'board-palette'].includes(q.get('view'));
const W2 = ['menu', 'palette', 'help', 'quick', 'selftest', 'accounts', 'context', 'worktrees', 'wt-overview'].includes(q.get('view')) || ORCH || BOARD;
if (W2) {
  const a = (id, worktree_id, name, status, extra = {}) => ({
    id, worktree_id, name, status, archived: false, unseen: false, status_changed_at: now - 60_000,
    kind: 'claude', custom_harness: null, model: null, effort: null, profile: null, alive: true,
    recent_prompts: [], role: 'worker', sort_order: 0, parent: null, ...extra,
  });
  model.worktrees.push({ id: 'w4', project_id: 'p2', path: 'C:\\demo\\storefront-web-worktrees\\cart-badge', branch: 'jari/cart-badge', is_main: false, sort_order: 1 });
  model.agents.push(
    a('a2', 'w1', 'refund-backoff', 'needs_feedback', { status_changed_at: now - 30_000, recent_prompts: [{ text: 'Back off refund webhook retries', submitted_at: now - 900_000 }] }),
    a('a3', 'w4', 'cart-badge', 'finished', { unseen: true, kind: 'codex', status_changed_at: now - 400_000, recent_prompts: [{ text: 'Show the item count on the cart icon', submitted_at: now - 1_200_000 }] }),
    a('a4', 'w3', 'checkout-a11y', 'finished', { status_changed_at: now - 3_600_000, profile: 'personal' }),
    a('a5', 'w2', 'ledger-recon-test', 'running', { parent: 'a1', status_changed_at: now - 90_000 }),
  );
  model.terminals.push({ id: 't1', worktree_id: 'w3', name: 'shell', sort_order: 0, alive: true, run_command: null });
  model.prs = { w2: { number: 418, url: 'https://dev.azure.com/demo-org/Platform/_git/ledger-api/pullrequest/418', title: 'Round GST per invoice', draft: true } };
  // The worktree shots: an empty checkout (Show all worktrees), a merged one.
  if (q.get('view') === 'worktrees' || q.get('view') === 'wt-overview') {
    model.worktrees.push(
      { id: 'w5', project_id: 'p2', path: 'C:\\demo\\storefront-web-worktrees\\old-promo', branch: 'jari/old-promo', is_main: false, sort_order: 2 },
      { id: 'w6', project_id: 'p2', path: 'C:\\demo\\storefront-web-worktrees\\font-swap', branch: 'jari/font-swap', is_main: false, sort_order: 3 },
    );
    model.prs.w5 = { number: 77, url: 'https://github.com/demo/storefront-web/pull/77', title: 'Retire the spring promo', draft: false, state: 'merged' };
    model.ports.push({ worktree: 'w3', block: null, live: [5173] });
    model.terminals.push({ id: 't2', worktree_id: 'w3', name: 'run', sort_order: 1, alive: true, run_command: 'npm run dev' });
  }
}

if (BOARD) {
  model.agents.push({
    id: 'a6', worktree_id: 'w2', name: 'gst-docs', status: 'finished', archived: false, unseen: false, status_changed_at: now - 1_900_000,
    kind: 'claude', custom_harness: null, model: null, effort: null, profile: 'work', alive: true,
    recent_prompts: [{ text: 'Document the GST rounding rule', submitted_at: now - 2_000_000 }], role: 'worker', sort_order: 0, parent: null,
  });
  model.prs.w2 = { ...model.prs.w2, draft: false, state: 'open' };
}
// Lanes, cards and the scope as the Tauri side works them out (a
// stand-in for nebula-core's rule, enough for the shots).
const LANE_OF = { a1: 'running', a5: 'running', a2: 'needs', a3: 'unseen', a6: 'pr', a4: 'done', o1: 'done' };
const R = (reason) => ({ act: 'refuse', reason });
function dropsFor(l) {
  const d = {
    running: R('Only the agent puts a card in Running, by starting a turn: send it a follow-up.'),
    needs: R('Only the agent puts a card in Needs you, by asking.'),
    unseen: R('Review comes from a turn finishing or from new changes; a card cannot be sent back to it.'),
    pr: R('Nothing to ship from Done: its worktree has no changes, or its pull request already landed.'),
    done: R('Running is the agent\u2019s: the card moves on when the turn ends.'),
    archived: { act: l === 'done' ? 'archive_and_offer_worktree_delete' : 'archive' },
  };
  d[l] = { act: 'reorder' };
  if (l === 'unseen') Object.assign(d, { pr: { act: 'open_pr' }, done: R('Its worktree has changes no pull request carries: commit and push them, ship them (drop on Ship) or archive the card.') });
  if (l === 'pr') d.done = { act: 'merge_pr' };
  return d;
}
function layOut() {
  const sc = model.scope ?? {};
  const proj = (a) => model.worktrees.find((w) => w.id === a.worktree_id)?.project_id;
  const admits = (a) => (!sc.repo || proj(a) === sc.repo) && (!sc.worktree || a.worktree_id === sc.worktree) && (!sc.session || a.id === sc.session) && (!sc.account || (a.profile ?? '@' + a.kind) === sc.account);
  model.cards = {};
  model.lanes = { running: [], needs: [], unseen: [], pr: [], done: [] };
  model.hidden = { running: 0, needs: 0, unseen: 0, pr: 0, done: 0 };
  for (const a of model.agents) {
    const lane = LANE_OF[a.id] ?? 'done';
    const in_scope = admits(a);
    model.cards[a.id] = { lane, tier: 3, in_scope, drops: dropsFor(lane) };
    if (a.archived || a.role === 'orchestrator') continue;
    if (in_scope) model.lanes[lane].push(a.id);
    else model.hidden[lane] += 1;
  }
}
model.scope = q.get('view') === 'board-scope' ? { repo: 'p1' } : {};
layOut();

// The orchestrator shots: a protocol-54 daemon, the orchestrator in its own
// workspace, and sessions it started in both repos (one of them nested).
if (ORCH) {
  model.protocol_version = 54;
  model.projects.unshift({ id: 'po', name: 'orchestrator', repo_path: 'C:\\demo\\kete\\orchestrator', sort_order: -1 });
  model.worktrees.unshift({ id: 'wo', project_id: 'po', path: 'C:\\demo\\kete\\orchestrator', branch: 'main', is_main: true, sort_order: 0 });
  if (q.get('view') !== 'start-orchestrator') {
    model.agents.unshift({
      id: 'o1', worktree_id: 'wo', name: 'orchestrator', status: 'finished', archived: false, unseen: false,
      status_changed_at: now - 45_000, kind: 'claude', custom_harness: null, model: 'opus', effort: 'high', profile: 'work',
      alive: true, recent_prompts: [{ text: 'Get GST rounding and the cart badge to review today', submitted_at: now - 1_800_000 }],
      role: 'orchestrator', sort_order: -1, parent: null,
    });
    for (const a of model.agents) if (a.id === 'a1' || a.id === 'a3') a.parent = 'o1';
  }
}
const ORCH_SCREEN = [
  '\x1b[1m> Get GST rounding and the cart badge to review today\x1b[0m',
  '',
  '\x1b[2m$ kete ado --project ledger-api tasks\x1b[0m',
  '  4211  Task  Round GST per invoice, not per line        Active',
  '  4212  Task  Ledger reconciliation test for GST totals  New',
  '  4231  Task  Back off refund webhook retries            Active',
  '',
  '\x1b[2m$ kete ado --project ledger-api start-task 4211 --agent claude\x1b[0m',
  '  worktree jari/gst-rounding ready, agent gst-rounding started',
  '\x1b[2m$ kete orchestrator spawn --project storefront-web --worktree jari/cart-badge --kind codex "Show the item count on the cart icon"\x1b[0m',
  '  started cart-badge',
  '',
  'Both are under way. \x1b[1mgst-rounding\x1b[0m spun up \x1b[1mledger-recon-test\x1b[0m for the',
  'reconciliation test; \x1b[1mcart-badge\x1b[0m finished its first pass and is unread.',
  '',
  '\x1b[2m$ kete orchestrator review cart-badge --tab diff\x1b[0m',
  '  opened a review of cart-badge for you',
  '',
  'cart-badge is ready for you to look at. refund-backoff (not mine) is',
  'waiting on a permission prompt; want me to read it?',
  '',
  '\x1b[1m>\x1b[0m ',
].join('\r\n');
const CHILD_SCREEN = [
  '\x1b[1m> Show the item count on the cart icon\x1b[0m',
  '',
  '  Edited src/components/CartIcon.tsx (+18 -3)',
  '  Edited src/components/CartIcon.test.tsx (+42)',
  '  Ran npm test -- CartIcon   \x1b[32m6 passed\x1b[0m',
  '',
  'The badge shows the count, caps at 99+, and is announced to screen readers.',
  '',
  '\x1b[1m>\x1b[0m ',
].join('\r\n');
const b64 = (t) => btoa(unescape(encodeURIComponent(t)));
const callbacks = {};
const listeners = {};
// ---- Accounts and context (synthetic): the CLI's own Claude login with
// its 5h and weekly use (weekly in the ember band), a disabled profile, a
// profile that needs a login, a Copilot account's monthly allowances. ----
const H1 = 3600;
const nowS = Math.floor(now / 1000);
const win = (kind, percent, resets, label = null, readAgo = 60) => ({ kind, label, percent, resets_at: resets ? nowS + resets : null, read_at: nowS - readAgo });
const accounts = {
  '@claude': { account: '@claude', harness: 'claude', windows: [win('session', 34, 3 * H1 + 12 * 60), win('weekly', 82, 2 * 86400 + 4 * H1)], source: 'statusline', at: now - 40_000, plan: null },
  'codex-team': { account: 'codex-team', harness: 'codex', windows: [win('session', 97, 40 * 60), win('weekly', 61, 5 * 86400)], source: 'rollout', at: now - 600_000, plan: 'plus' },
  'codex-spare': { account: 'codex-spare', harness: 'codex', windows: [win('session', 22, null, null, 4 * H1), win('weekly', 48, 3 * 86400, null, 4 * H1)], source: 'rollout', at: now - 4 * H1 * 1000, plan: 'plus' },
  'gh-work': { account: 'gh-work', harness: 'copilot', windows: [win('monthly', 41, 17 * 86400, 'premium'), win('monthly', 6, 17 * 86400, 'chat')], source: 'copilot_api', at: now - 1_800_000, plan: 'business' },
};
const accountRow = (key, name, harness, o = {}) => ({
  key, name, harness, profile: true, default: false, disabled: false, useDefault: false, login: 'logged_in', loginHint: null,
  usage: accounts[key] ?? null, ...o,
});
const accountRows = [
  accountRow('@claude', 'claude', 'claude', { profile: false, useDefault: true }),
  accountRow('codex-team', 'codex-team', 'codex', { default: true }),
  accountRow('codex-spare', 'codex-spare', 'codex', { disabled: true }),
  accountRow('gh-work', 'gh-work', 'copilot', { default: true }),
  accountRow('gh-personal', 'gh-personal', 'copilot', { login: 'needs_login', loginHint: 'kete profile login gh-personal' }),
];
model.protocol_version = 55;
model.accounts = accounts;
model.contexts = {
  a1: { percent: 96, tokens: 192_000, window: 200_000, compactions: 2, last_compact_at: now - 25 * 60_000, source: 'statusline', at: now - 30_000 },
  ...(W2 ? {
    a2: { percent: 41, tokens: 82_000, window: 200_000, compactions: 0, last_compact_at: null, source: 'statusline', at: now - 60_000 },
    a3: { percent: 12, tokens: 57_000, window: 475_000, compactions: 0, last_compact_at: null, source: 'rollout', at: now - 400_000 },
    a5: { percent: 83, tokens: 166_000, window: 200_000, compactions: 1, last_compact_at: now - 3_600_000, source: 'transcript', at: now - 90_000 },
  } : {}),
};
// Why a session's last turn stopped short: a running one waiting out a
// usage limit (resumes 3:45pm), and one that needs a login.
model.stops = {
  a1: { reason: 'rate_limit', waiting: true, resumes: '3:45pm', resets_at: nowS + 3 * H1 + 12 * 60, detail: null, at: now - 90_000 },
  ...(W2 ? {
    a2: { reason: 'authentication_failed', waiting: false, resumes: null, resets_at: null, detail: 'OAuth token has expired', at: now - 30_000 },
  } : {}),
};

// ---- Tasks (synthetic sprint) ----
const sprints = [
  { id: 's41', name: 'Sprint 41', path: 'Platform\\Sprint 41', start: iso(today - 31 * DAY), finish: iso(today - 17 * DAY), timeframe: 'past' },
  { id: 's42', name: 'Sprint 42', path: 'Platform\\Sprint 42', start: iso(today - 16 * DAY), finish: iso(today - 3 * DAY), timeframe: 'past' },
  { id: 's43', name: 'Sprint 43', path: 'Platform\\Sprint 43', start: iso(today - 2 * DAY), finish: iso(today + 11 * DAY), timeframe: 'current' },
  { id: 's44', name: 'Sprint 44', path: 'Platform\\Sprint 44', start: iso(today + 12 * DAY), finish: iso(today + 25 * DAY), timeframe: 'future' },
];
const ME = 'Jamie Rivers (demo)';
const item = (id, kind, title, state, cat, col, rem, parent, who = ME, desc = '', acc = '') => ({
  id, kind, title, state, stateCategory: cat, assignedTo: who, remaining: rem, parent, boardColumn: col,
  descriptionMd: desc, acceptanceMd: acc, url: `https://dev.azure.com/demo-org/Platform/_workitems/edit/${id}`,
});
const story = item(4210, 'User Story', 'GST rounding on invoices', 'Active', 'InProgress', 'Doing', null, null, ME,
  'Invoices round GST on every line, so a 12-line invoice can drift a few cents from the ledger.',
  '- Totals match the ledger to the cent\n- Rounding happens once per invoice\n- Existing invoices are not re-rounded');
const bug = item(4230, 'Bug', 'Refund webhook retries forever when the gateway is down', 'Active', 'InProgress', 'Doing', null, null, 'Gale Ashby (demo)');
const groups = [
  {
    parent: story,
    items: [
      item(4211, 'Task', 'Round GST per invoice, not per line', 'Active', 'InProgress', 'Doing', 3, 4210, ME,
        'Move rounding out of InvoiceLine.Total and into Invoice.GstTotal. Keep the per-line figure for display only.',
        story.acceptanceMd),
      item(4212, 'Task', 'Ledger reconciliation test for GST totals', 'New', 'Proposed', 'To Do', 2, 4210, ME, '', story.acceptanceMd),
      item(4213, 'Task', 'Show the rounding note on the invoice PDF', 'New', 'Proposed', 'To Do', 1.5, 4210, ME),
      item(4209, 'Task', 'Spike: how the ledger rounds today', 'Closed', 'Completed', 'Done', 0, 4210, ME),
    ],
  },
  {
    parent: bug,
    items: [
      item(4231, 'Task', 'Back off refund webhook retries exponentially', 'Active', 'InProgress', 'Doing', 4, 4230, ME,
        'Retries run every 5 s with no ceiling. Use exponential back-off capped at 10 min, then park the event.'),
      item(4232, 'Task', 'Alert when a refund event is parked', 'Resolved', 'Resolved', 'Review', 0.5, 4230, ME),
    ],
  },
  { parent: null, items: [item(4250, 'Task', 'Rotate the sandbox payment keys', 'New', 'Proposed', 'To Do', 1, null, ME)] },
];
const taskList = (scope) => {
  if (scope && typeof scope === 'object') {
    const it = sprints.find((s) => s.id === scope.iteration) ?? null;
    return { iteration: it, stale: false, next: null, groups: it?.id === 's42' ? groups : [], cachedAt: null };
  }
  return {
    iteration: sprints[1],
    stale: scope !== 'myOpen',
    next: sprints[2],
    groups,
    cachedAt: q.get('offline') ? now - 14 * 60_000 : null,
  };
};

// ---- Branches (synthetic) ----
const H = 3600;
const br = (name, o) => ({
  name, person: name.includes('/') ? name.slice(0, name.indexOf('/')) : 'other', local: true, remote: true,
  ahead: 0, behind: 0, gone: false, last_commit_age: 5 * H, last_author: 'Jamie Rivers (demo)',
  task: null, pr: null, worktree: null, session: null, ...o,
});
const tree = {
  me: 'jari',
  people: [
    ['jari', [
      br('jari/gst-rounding', { ahead: 2, behind: 1, last_commit_age: 25 * 60, task: 4211, pr: { id: 812, status: 'active', is_draft: true }, worktree: 'w2', session: 'a1' }),
      br('jari/refund-webhook-backoff', { remote: false, ahead: 3, last_commit_age: 3 * H, task: 4231 }),
      br('jari/ledger-export-csv', { last_commit_age: 4 * 86400, task: 4180, pr: { id: 790, status: 'active', is_draft: false } }),
      br('jari/old-pdf-fonts', { remote: false, gone: true, last_commit_age: 26 * 86400 }),
    ]],
    ['gaas', [
      br('gaas/flaky-login-test', { last_commit_age: 7 * H, last_author: 'Gale Ashby (demo)', pr: { id: 808, status: 'active', is_draft: false } }),
      br('gaas/gst-docs', { local: false, last_commit_age: 2 * 86400, last_author: 'Gale Ashby (demo)' }),
    ]],
    ['mipa', [
      br('mipa/refund-alerts', { local: false, last_commit_age: 30 * H, last_author: 'Mika Patel (demo)', task: 4232 }),
      br('mipa/ci-node-24', { local: false, last_commit_age: 9 * 86400, last_author: 'Mika Patel (demo)' }),
      br('mipa/spike-ledger-rounding', { local: false, last_commit_age: 15 * 86400, last_author: 'Mika Patel (demo)' }),
    ]],
    ['other', [br('main', { last_commit_age: 2 * H, last_author: 'Gale Ashby (demo)' })]],
  ],
};
const range = {
  base: '3f9c2a1d0b7e4c5a8d6f1e2b3c4d5e6f7a8b9c0d',
  tip: 'a17e5b9c2d3f4e5a6b7c8d9e0f1a2b3c4d5e6f70',
  offline: false,
  files: [
    { path: 'src/Ledger.Api/Invoices/Invoice.cs', from: null, status: 'M', added: 38, removed: 12 },
    { path: 'src/Ledger.Api/Invoices/InvoiceLine.cs', from: null, status: 'M', added: 4, removed: 19 },
    { path: 'src/Ledger.Api/Tax/GstRounding.cs', from: null, status: 'A', added: 61, removed: 0 },
    { path: 'tests/Ledger.Api.Tests/Tax/GstRoundingTests.cs', from: null, status: 'A', added: 112, removed: 0 },
  ],
};

const prompt = `# Task 4211: Round GST per invoice, not per line

https://dev.azure.com/demo-org/Platform/_workitems/edit/4211

Parent: User Story 4210: GST rounding on invoices

## Description
Move rounding out of InvoiceLine.Total and into Invoice.GstTotal. Keep the per-line figure for display only.

## Acceptance criteria
- Totals match the ledger to the cent
- Rounding happens once per invoice
- Existing invoices are not re-rounded
`;

const MID = String.fromCharCode(0xb7);
const presets = [
  { name: 'reviewer', harness: 'claude', model: 'opus', effort: 'high', prefix: 'Review this branch like a strict senior engineer. Point at lines.', postfix: '', skipTask: false, spec: ['claude', 'opus', 'high'].join(' ' + MID + ' '), usable: true },
  { name: 'commit-and-push', harness: 'claude', model: null, effort: null, prefix: '', postfix: 'Commit with a plain message and push the branch.', skipTask: true, spec: 'claude', usable: true },
  { name: 'codex-scout', harness: 'codex', model: 'gpt-5.5', effort: null, prefix: 'Read the code first; do not edit.', postfix: 'End with a short list of findings.', skipTask: false, spec: ['codex', 'gpt-5.5'].join(' ' + MID + ' '), usable: true },
];
const profiles = [
  { name: 'work', harness: 'claude', default: true, useDefault: false, envKeys: [], home: 'C:\\Users\\demo\\AppData\\Local\\kete\\data\\profiles\\work', problem: null, login: 'kete profile login work', usage: '41% of weekly' },
  { name: 'personal', harness: 'claude', default: false, useDefault: true, envKeys: [], home: null, problem: null, login: null, usage: null },
  { name: 'codex-team', harness: 'codex', default: true, useDefault: false, envKeys: ['HTTPS_PROXY'], home: 'C:\\Users\\demo\\AppData\\Local\\kete\\data\\profiles\\codex-team', problem: null, login: 'kete profile login codex-team', usage: null },
];

let settingsSchema = null;
const loadSchema = async () => (settingsSchema ??= await (await fetch('/demo/settings-schema.json')).json());
const sameTarget = (a, b) => JSON.stringify(a) === JSON.stringify(b);

const handlers = {
  get_state: () => model,
  board_scope: ({ scope }) => {
    model.scope = scope;
    layOut();
    callbacks[listeners['kete://state']]?.({ event: 'kete://state', id: 2, payload: { ...model } });
    return null;
  },
  ship_status: ({ worktree }) =>
    worktree === 'w2'
      ? { forge: 'ado', remote: 'origin', branch: 'jari/gst-rounding', base: 'main', unpushed: 0, pr: { forge: 'ado', number: 418, url: model.prs.w2.url, title: 'Round GST per invoice', state: 'open', draft: false, head: 'jari/gst-rounding', base: 'main', headSha: null }, error: null, suggestion: null }
      : { forge: 'github', remote: 'origin', branch: 'jari/cart-badge', base: 'main', unpushed: 2, pr: null, error: { kind: 'signIn', forge: 'github', message: 'GitHub sign-in needed: run `gh auth login`, then try again.', url: null }, suggestion: { title: 'Show the item count on the cart icon', body: '## Summary\n- Cart badge with the item count, capped at 99+\n' } },
  ship_create_pr: () => Promise.reject({ kind: 'signIn', forge: 'github', message: 'GitHub sign-in needed: run `gh auth login`, then try again.', url: null }),
  ship_merge: () => ({ pr: { number: 418 }, state: 'merged', branchDeleted: true, note: null }),
  refresh_prs: () => null,
  'plugin:event|listen': ({ event, handler }) => ((listeners[event] = handler), handler),
  attach: ({ session, onEvent }) => {
    const text = session.Agent === 'o1' ? ORCH_SCREEN : CHILD_SCREEN;
    setTimeout(() => callbacks[onEvent.id]?.({ index: 0, message: { kind: 'scrollback', data: b64(text) } }), 60);
    return null;
  },
  detach: () => null,
  resize: () => null,
  input: () => null,
  mark_seen: () => null,
  follow_up: () => null,
  read_session: () => '## user\nShow the item count on the cart icon\n\n## assistant\nEdited CartIcon.tsx and its test. The badge caps at 99+ and has an aria-label.\n',
  start_orchestrator: () => 'o1',
  set_orchestrator: () => null,
  clear_orchestrator: () => null,
  diff_files: () => [
    { path: 'src/components/CartIcon.tsx', from: null, status: 'M', added: 18, removed: 3 },
    { path: 'src/components/CartIcon.test.tsx', from: null, status: 'A', added: 42, removed: 0 },
  ],
  diff_pair: ({ file }) =>
    file.status === 'A'
      ? { original: '', modified: "import { render } from '@testing-library/react';\n\ntest('caps at 99+', () => {\n  // ...\n});\n", note: null }
      : { original: 'export function CartIcon() {\n  return <Icon name="cart" />;\n}\n', modified: 'export function CartIcon({ count }: { count: number }) {\n  const label = count > 99 ? "99+" : String(count);\n  return (\n    <span aria-label={`Cart, ${count} items`}>\n      <Icon name="cart" />\n      {count > 0 && <b>{label}</b>}\n    </span>\n  );\n}\n', note: null },
  // The review diff: against the base by default, with the commits since it.
  diff_review: ({ against }) => ({
    against: against ?? 'base',
    base: { sha: '3f9c2a1d0b7e4c5a8d6f1e2b3c4d5e6f7a8b9c0d', short: '3f9c2a1', branch: 'main' },
    commits: [
      { sha: 'c3', short: 'a17e5b9', subject: 'Announce the count to screen readers' },
      { sha: 'c2', short: '6d0c4e2', subject: 'Cap the cart badge at 99+ and test the edges' },
      { sha: 'c1', short: '9b81f07', subject: 'Show the item count on the cart icon' },
    ],
    files: handlers.diff_files(),
  }),
  diff_blob: ({ side, path }) => {
    const f = handlers.diff_files().find((x) => x.path === path);
    const pair = handlers.diff_pair({ file: f ?? { status: 'M' } });
    const text = side.kind === 'worktree' ? pair.modified : pair.original;
    return text ? { state: 'text', value: text } : { state: 'missing' };
  },
  diff_hunks: () => [],
  'plugin:event|unlisten': () => null,
  'plugin:window|set_theme': () => null,
  launchables: () => ({
    harnesses: [
      { id: 'claude', label: 'Claude Code', choices: { models: ['default', 'opus', 'sonnet', 'haiku'], model: 'opus', efforts: Object.fromEntries(['default', 'opus', 'sonnet', 'haiku'].map((m) => [m, ['default', 'low', 'medium', 'high', 'max']])), effort: 'high' } },
      { id: 'codex', label: 'Codex', choices: { models: ['default', 'gpt-5.5', 'gpt-5.5-mini'], model: 'default', efforts: Object.fromEntries(['default', 'gpt-5.5', 'gpt-5.5-mini'].map((m) => [m, ['default', 'low', 'medium', 'high']])), effort: 'default' } },
    ],
    default_harness: 'claude',
    new_worktree: false,
    follow: true,
    ask_before_archive: true,
    profiles: [{ name: 'work', harness: 'claude', default: true }, { name: 'personal', harness: 'claude', default: false }],
  }),
  startup: () => ({ view: null, session: null, theme: q.get('theme') ?? 'light', diff_mode: null }),
  ado_context: ({ worktree }) =>
    worktree === 'w1'
      ? { host: 'dev.azure.com', org: 'demo-org', project: 'Platform', repo: 'ledger-api', orgUrl: 'https://dev.azure.com/demo-org', repoKey: 'demo-org/Platform/ledger-api', ui: {} }
      : null,
  ado_save_ui: () => null,
  ado_teams: () => ['Payments Team', 'Platform Team'],
  ado_iterations: () => sprints,
  ado_tasks: ({ scope }) => taskList(scope),
  ado_set_team: () => null,
  ado_task_prompt: () => prompt,
  ado_start_task: () => ({ worktreeId: 'w9', agentId: null, problems: [] }),
  ado_branches: () => tree,
  ado_fetch: () => null,
  ado_checkout_worktree: () => 'w9',
  ado_delete_local: () => null,
  diff_range: () => range,
  open_url: () => null,
  worktree_git: ({ worktrees }) =>
    Object.fromEntries(
      worktrees.map((id, i) => [
        id,
        { changed: [0, 3, 0, 1, 0, 0][i % 6], untracked: i % 3 === 1 ? 1 : 0, upstream: id === 'w6' ? null : 'origin/x', ahead: id === 'w6' ? null : i % 3, behind: i === 0 ? 2 : 0, committed: Math.round((now - i * 7_200_000) / 1000), ahead_of_base: id === 'w4' ? 3 : id === 'w2' ? 5 : 0, on_default_branch: id === 'w1' || id === 'w3' },
      ]),
    ),
  worktree_reveal: () => null,
  desktop_set_collapsed: () => null,
  checkout_state: ({ worktree }) => ({ changed: 0, untracked: 0, unpushed: worktree === 'w6' ? 0 : 2 }),
  worktree_settings: () => ({ delete_empty_worktree: false }),
  settings_schema: () => loadSchema(),
  settings_set: async ({ target, value }) => {
    const s = await loadSchema();
    for (const t of s.tabs) for (const g of t.groups) for (const r of g.rows) if (sameTarget(r.target, target)) r.value = value;
    return null;
  },
  settings_reset: () => null,
  desktop_prefs: () => ({ theme: 'system', diff_mode: 'side', home: BOARD ? 'board' : 'overview' }),
  config_problem: () => null,
  // The critters stay off in the demo (the screenshots are of the app).
  critters_prefs: () => ({ on: false, size: 'medium', often: 'normal', animations: true, debug: false }),
  critters_log: () => ({ version: 1, species: {}, lamps: 0 }),
  critters_log_update: () => ({ version: 1, species: {}, lamps: 0 }),
  run_commands: () => ({ w1: 'dotnet watch run', w2: 'dotnet watch run' }),
  start_run: () => 't9',
  stop_run: () => null,
  presets_list: () => presets,
  preset_save: ({ original, preset }) => {
    const i = original ? presets.findIndex((p) => p.name === original) : -1;
    const row = { ...preset, spec: [preset.harness, preset.model, preset.effort].filter((x) => x && x !== 'default').join(' / '), usable: true };
    if (i >= 0) presets[i] = row;
    else presets.push(row);
    return presets;
  },
  preset_delete: ({ name }) => (presets.splice(presets.findIndex((p) => p.name === name), 1), presets),
  profiles_list: () => profiles,
  profile_harnesses: () => ['claude', 'codex', 'copilot'],
  profile_add: () => ({ rows: profiles, login: 'kete profile login work', home: null }),
  profile_update: () => ({ rows: profiles, login: null, home: null }),
  profile_remove: () => ({ rows: profiles, login: null, home: null }),
  profile_set_default: () => profiles,
  suggest_branch: () => 'amber-otter-glides',
  quick_launch: () => ({ worktree: 'w1', agent: 'a1', terminal: null, picked: 'claude: its own login (no profile)' }),
  accounts_list: () => accountRows,
  profile_set_enabled: ({ name, enabled }) => {
    const r = accountRows.find((x) => x.name === name);
    if (r) r.disabled = !enabled;
    return null;
  },
  profile_set_reserved: ({ name, reserved }) => {
    const r = accountRows.find((x) => x.name === name);
    if (r) r.reserved = reserved;
    return null;
  },
};

let cb = 0;
window.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
  transformCallback: (fn) => {
    const id = ++cb;
    callbacks[id] = fn;
    return id;
  },
  unregisterCallback: () => {},
  convertFileSrc: (p) => p,
  invoke: async (cmd, args) => {
    const h = handlers[cmd];
    if (!h) throw new Error(`demo: no handler for ${cmd}`);
    await new Promise((r) => setTimeout(r, 30));
    return h(args ?? {});
  },
};

// ---- drive the window to the requested place ----
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(find, tries = 100) {
  for (let i = 0; i < tries; i++) {
    const el = find();
    if (el) return el;
    await wait(50);
  }
  throw new Error('demo: element never appeared');
}
const byText = (sel, text) => () => [...document.querySelectorAll(sel)].find((e) => e.textContent.trim() === text);

window.addEventListener('load', async () => {
  const view = q.get('view');
  if (!view) return;
  if (view === 'settings') {
    (await until(byText('aside button', 'Settings'))).click();
    const tab = q.get('tab');
    if (tab) (await until(() => document.getElementById('settings-tab-' + tab))).click();
    return;
  }
  const key = (k, m = {}) => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, ...m }));
  const ready = () => until(() => document.querySelector('[data-target="agent:a1"]'));
  if (view === 'menu') {
    const row = await ready();
    const r = row.getBoundingClientRect();
    row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: r.left + 300, clientY: r.top + r.height / 2 }));
    await wait(150);
    for (let i = 0; i < 2; i++) document.querySelector('[role="menu"]')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    return;
  }
  if (view === 'palette') {
    await ready();
    key('k', { ctrlKey: true });
    const input = await until(() => document.querySelector('[aria-label="Command palette"] input'));
    if (q.get('q')) {
      input.value = q.get('q');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    }
    return;
  }
  if (view === 'accounts') {
    await ready();
    (await until(() => document.querySelector('[aria-label="Accounts"] [aria-expanded]'))).click();
    return;
  }
  if (view === 'context') {
    (await ready()).click();
    return;
  }
  if (view === 'help') {
    await ready();
    key('?', { shiftKey: true });
    return;
  }
  // view=selftest: each layer takes the keys when it opens and Esc on
  // whatever has the focus closes it. The verdict goes in the title, for
  // a headless --dump-dom to read.
  if (view === 'selftest') {
    await ready();
    const esc = () => (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    const out = [];
    const check = async (name, open, sel) => {
      open();
      await wait(150);
      const opened = !!document.querySelector(sel);
      esc();
      await wait(150);
      out.push(name + ':' + (opened && !document.querySelector(sel) ? 'ok' : opened ? 'stuck' : 'no-open'));
    };
    await check('help', () => key('?', { shiftKey: true }), '[aria-labelledby="help-title"]');
    await check('palette', () => key('k', { ctrlKey: true }), '[aria-label="Command palette"]');
    await check('menu', () => {
      const row = document.querySelector('[data-target="agent:a1"]');
      row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 400, clientY: 120 }));
    }, '[role="menu"]');
    await check('rowkey-rename', () => {
      document.querySelector('[data-target="agent:a1"]').focus();
      document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'r', bubbles: true }));
    }, '[role="dialog"][aria-labelledby="dialog-title"]');
    // The quick prompt's options row opens over the column, not in it.
    const col = document.querySelector('main > div.relative');
    const before = col.getBoundingClientRect().height;
    document.querySelector('textarea[aria-label="Prompt"]').focus();
    await wait(150);
    const opened = !!document.querySelector('[aria-label="Launch options"]');
    out.push('quick-height:' + (opened && col.getBoundingClientRect().height === before ? 'ok' : opened ? 'shrank' : 'no-open'));
    document.title = 'SELFTEST ' + out.join(' ');
    return;
  }
  if (view === 'quick') {
    await ready();
    const box = await until(() => document.querySelector('textarea[aria-label="Prompt"]'));
    box.focus();
    box.value = 'Add a reconciliation test for GST totals across 12-line invoices.\nKeep the fixtures small.';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    return;
  }
  if (view === 'orchestrator' || view === 'review') {
    (await until(byText('aside nav button span', 'Orchestrator'))).closest('button').click();
    if (view === 'review') {
      await wait(400);
      // The daemon's ReviewOpened, as kete orchestrator review sends it.
      const id = listeners['kete://review'];
      callbacks[id]?.({ event: 'kete://review', id: 1, payload: { opener: 'o1', sessions: [{ Agent: 'a3' }], tabs: [] } });
      if (q.get('notice')) return;
      (await until(byText('button', 'Open'))).click();
      const tab = q.get('tab');
      if (tab) (await until(byText('[role="tab"]', tab === 'pr' ? 'PR' : tab[0].toUpperCase() + tab.slice(1)))).click();
    }
    return;
  }
  if (view === 'start-orchestrator') {
    (await until(byText('aside nav button span', 'Start orchestrator'))).closest('button').click();
    return;
  }
  if (view === 'overview-cue' || view === 'wt-overview') return;
  if (BOARD) {
    // A review bar button by its word (its key cap follows the word).
    const reviewBtn = (word) => () => [...document.querySelectorAll('[data-review-bar] button')].find((b) => b.textContent.trim().startsWith(word));
    if (view === 'board-review') (await until(() => document.querySelector('[data-target="agent:a3"] button'))).click();
    if (view === 'board-ship') {
      (await until(() => document.querySelector('[data-target="agent:a3"] button'))).click();
      (await until(reviewBtn('Open PR'))).click();
      // ship_error=1: create it, and show the sign-in error under the form.
      if (q.get('ship_error')) {
        await wait(400);
        (await until(() => [...document.querySelectorAll('[role="dialog"] button, form button')].find((b) => /^(Create|Open) (draft )?(pull request|PR)/i.test(b.textContent.trim()))))?.click();
      }
    }
    if (view === 'board-merge') {
      (await until(() => document.querySelector('[data-target="agent:a6"] button'))).click();
      (await until(reviewBtn('Merge'))).click();
    }
    if (view === 'board-verbs') {
      // The card's own button takes the focus (its wrapper cannot), and
      // `?` goes to it, so the menu is the card's.
      const card = await until(() => document.querySelector('[data-target="agent:a3"] button'));
      card.focus();
      await wait(100);
      card.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true }));
    }
    if (view === 'board-palette') {
      await until(() => document.querySelector('[data-target="agent:a3"]'));
      key('k', { ctrlKey: true });
    }
    return;
  }
  if (view === 'worktrees') {
    (await until(() => document.querySelector('[aria-label="storefront-web views"] button'))).click();
    return;
  }
  const label = view === 'tasks' ? 'Tasks' : 'Branches';
  (await until(byText('aside nav button', label))).click();
  if (view === 'tasks') {
    (await until(() => document.querySelector('[data-id="4211"]'))).click();
    if (q.get('start')) (await until(byText('button', 'Start task'))).click();
  } else {
    const sel = q.get('sel');
    if (sel) {
      (await until(() => document.querySelector(`[data-branch="${sel}"] button`))).click();
      if (q.get('diff')) (await until(byText('button', 'Diff vs main'))).click();
    }
  }
});
