//! SQLite persistence. Write volume is trivial (entity CRUD + status
//! changes), so a mutex-guarded connection is sufficient — no ORM, no
//! connection pool.

use crate::session_title::TitleState;
use anyhow::{Context, Result};
use nebula_core::clock::now_ms;
use nebula_core::review_base::RecordedBase;
use nebula_core::session_snapshot::SnapshotPoint;
use nebula_core::studio::wire::{LedgerRow, SpendRow};
use nebula_core::{
    Agent, AgentId, AgentKind, AgentStatus, Link, LinkId, PrSeen, Project, ProjectId, PromptEntry,
    ReorderScope, TerminalId, TerminalTab, Worktree, WorktreeId, RECENT_PROMPTS_KEPT,
};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    "
    CREATE TABLE projects (
      id          TEXT PRIMARY KEY,
      name        TEXT NOT NULL,
      repo_path   TEXT NOT NULL UNIQUE,
      sort_order  INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL
    );
    CREATE TABLE worktrees (
      id          TEXT PRIMARY KEY,
      project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
      path        TEXT NOT NULL,
      branch      TEXT NOT NULL,
      is_main     INTEGER NOT NULL DEFAULT 0,
      sort_order  INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL,
      UNIQUE (project_id, path)
    );
    CREATE TABLE agents (
      id                TEXT PRIMARY KEY,
      worktree_id       TEXT NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
      name              TEXT NOT NULL,
      status            TEXT NOT NULL DEFAULT 'fresh',
      archived          INTEGER NOT NULL DEFAULT 0,
      claude_session_id TEXT,
      sort_order        INTEGER NOT NULL DEFAULT 0,
      created_at        INTEGER NOT NULL,
      status_changed_at INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE terminals (
      id          TEXT PRIMARY KEY,
      worktree_id TEXT NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
      name        TEXT NOT NULL DEFAULT 'shell',
      sort_order  INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL
    );
    CREATE TABLE ui_state (
      id    INTEGER PRIMARY KEY CHECK (id = 1),
      json  TEXT NOT NULL
    );
    ",
    // 2: project group dividers
    "
    ALTER TABLE projects ADD COLUMN divider_after INTEGER NOT NULL DEFAULT 0;
    ",
    // 3: divider labels
    "
    ALTER TABLE projects ADD COLUMN divider_label TEXT;
    ",
    // 4: agent kind (claude | codex); claude_session_id doubles as the
    // resume id for whichever kind the agent runs.
    "
    ALTER TABLE agents ADD COLUMN kind TEXT NOT NULL DEFAULT 'claude';
    ",
    // 5: pinned agents — the PIN feature was removed on 2026-08-28; the
    //    column stays (unread) rather than costing a table rebuild
    "
    ALTER TABLE agents ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
    ",
    // 6: pinned worktrees (same story as 5)
    "
    ALTER TABLE worktrees ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
    ",
    // 7: the leading divider — drawn above the whole list, owned by the
    // first project
    "
    ALTER TABLE projects ADD COLUMN divider_before INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE projects ADD COLUMN divider_before_label TEXT;
    ",
    // 8: per-worktree todo notes
    "
    CREATE TABLE todos (
      id          TEXT PRIMARY KEY,
      worktree_id TEXT NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
      text        TEXT NOT NULL,
      done        INTEGER NOT NULL DEFAULT 0,
      sort_order  INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL
    );
    ",
    // 9: per-agent model/effort launch options (NULL = CLI default)
    "
    ALTER TABLE agents ADD COLUMN model TEXT;
    ALTER TABLE agents ADD COLUMN effort TEXT;
    ",
    // 10: todos gain a project scope — exactly one of project_id /
    // worktree_id is set. Table rebuild: SQLite can't relax the old
    // NOT NULL worktree_id in place. Existing rows stay worktree-owned.
    "
    CREATE TABLE todos_new (
      id          TEXT PRIMARY KEY,
      project_id  TEXT REFERENCES projects(id) ON DELETE CASCADE,
      worktree_id TEXT REFERENCES worktrees(id) ON DELETE CASCADE,
      text        TEXT NOT NULL,
      done        INTEGER NOT NULL DEFAULT 0,
      sort_order  INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL,
      CHECK ((project_id IS NULL) <> (worktree_id IS NULL))
    );
    INSERT INTO todos_new (id, worktree_id, text, done, sort_order, created_at)
      SELECT id, worktree_id, text, done, sort_order, created_at FROM todos;
    DROP TABLE todos;
    ALTER TABLE todos_new RENAME TO todos;
    ",
    // 11: when the agent was archived (orders the ARCHIVED group
    // newest-first; 0 for rows archived before this migration)
    "
    ALTER TABLE agents ADD COLUMN archived_at INTEGER NOT NULL DEFAULT 0;
    ",
    // 12: sessions created with the generated default name await one
    // agent-driven auto-title (`nebula rename` from inside the CLI);
    // cleared by the first rename, user- or agent-made. Daemon-internal —
    // never leaves the store, so pre-existing rows defaulting to 0 simply
    // keep their names.
    "
    ALTER TABLE agents ADD COLUMN auto_title_pending INTEGER NOT NULL DEFAULT 0;
    ",
    // 13: workspaces — named project groups, exactly one open (`active`) at
    // a time. Every install gets the built-in 'default' workspace and all
    // pre-existing projects move into it. The new projects column stays
    // nullable (SQLite forbids a non-NULL default on an added REFERENCES
    // column); reads COALESCE to 'default'.
    "
    CREATE TABLE workspaces (
      id          TEXT PRIMARY KEY,
      name        TEXT NOT NULL UNIQUE,
      active      INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL
    );
    INSERT INTO workspaces (id, name, active, created_at) VALUES ('default', 'default', 1, 0);
    ALTER TABLE projects ADD COLUMN workspace_id TEXT REFERENCES workspaces(id);
    UPDATE projects SET workspace_id = 'default';
    ",
    // 14: workspaces are free-form groupings, so the same repo may be added
    // to any number of them — uniqueness moves from a global repo_path
    // constraint to (workspace, repo_path). Table rebuild: SQLite can't
    // drop the inline UNIQUE. Runs with foreign keys off (see migrate())
    // so the DROP doesn't cascade into worktrees/agents/terminals/todos.
    "
    CREATE TABLE projects_new (
      id          TEXT PRIMARY KEY,
      name        TEXT NOT NULL,
      repo_path   TEXT NOT NULL,
      sort_order  INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL,
      divider_after INTEGER NOT NULL DEFAULT 0,
      divider_label TEXT,
      divider_before INTEGER NOT NULL DEFAULT 0,
      divider_before_label TEXT,
      workspace_id TEXT REFERENCES workspaces(id)
    );
    INSERT INTO projects_new (id, name, repo_path, sort_order, created_at, divider_after, divider_label, divider_before, divider_before_label, workspace_id)
      SELECT id, name, repo_path, sort_order, created_at, divider_after, divider_label, divider_before, divider_before_label, workspace_id FROM projects;
    DROP TABLE projects;
    ALTER TABLE projects_new RENAME TO projects;
    CREATE UNIQUE INDEX projects_workspace_repo ON projects (COALESCE(workspace_id, 'default'), repo_path);
    ",
    // 15: todos are now "notes" everywhere — rename the table to match.
    "
    ALTER TABLE todos RENAME TO notes;
    ",
    // 16: per-worktree links — pull requests, tickets, docs. Worktree-only
    // (unlike notes): a link describes the branch's work, and a project's
    // links would be the same for every checkout.
    "
    CREATE TABLE links (
      id          TEXT PRIMARY KEY,
      worktree_id TEXT NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
      url         TEXT NOT NULL,
      sort_order  INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL
    );
    ",
    // 17: how far the user has read into a pull request's conversation.
    // Keyed by URL rather than worktree — the PR is the thing that grows
    // comments, it outlives the checkout, and the same one can be pinned to
    // more than one of them.
    "
    CREATE TABLE pr_seen (
      url      TEXT PRIMARY KEY,
      marker   TEXT NOT NULL,
      seen_at  INTEGER NOT NULL
    );
    ",
    // 18: project group dividers are gone (migrations 2, 3 and 7 added
    // them; 14 carried them through the table rebuild). Plain columns with
    // no index or constraint, so DROP COLUMN is enough.
    "
    ALTER TABLE projects DROP COLUMN divider_after;
    ALTER TABLE projects DROP COLUMN divider_label;
    ALTER TABLE projects DROP COLUMN divider_before;
    ALTER TABLE projects DROP COLUMN divider_before_label;
    ",
    // 19: a turn finished while nobody was looking (see `Agent::unseen`).
    // Set by `set_agent_status` on a live → finished flip, cleared by
    // `mark_agent_seen`, by leaving `finished`, and by archiving.
    "
    ALTER TABLE agents ADD COLUMN unseen INTEGER NOT NULL DEFAULT 0;
    ",
    // 20: the Claude Cloud session a row launched (`Agent::cloud_session_id`),
    // read off the `claude --cloud` spawn's output. Drives the attach /
    // teleport restart path; NULL for every local row.
    "
    ALTER TABLE agents ADD COLUMN cloud_session_id TEXT;
    ",
    // 21: notes are gone (migration 8 created them as `todos`, 10 gave them
    // a project scope, 15 renamed the table). Nothing else references the
    // table, so a plain DROP retires the feature and its rows.
    "
    DROP TABLE IF EXISTS notes;
    ",
    // 22: the PR URL that scopes a Claude AGENT created from an OPEN PRS
    // row. Nullable and request-driven: every existing AGENT remains an
    // ordinary session, while a PR-created one can rebuild its appended
    // system prompt after a daemon restart or RESUME.
    "
    ALTER TABLE agents ADD COLUMN pr_url TEXT;
    ",
    // 23: the title Claude Code itself holds for the row's session — what
    // `/rename` set inside the CLI, or what nebula last pushed into it
    // through the UserPromptSubmit hook reply (CLAUDE TITLE SYNC, see
    // `session_title.rs`). Compared with `name` to keep the two tied
    // without either side undoing the other's newer choice; NULL until
    // the first sync, so every existing row simply starts unsynced.
    "
    ALTER TABLE agents ADD COLUMN claude_title TEXT;
    ",
    // 24: RECENT PROMPTS — the newest few prompts typed into the session,
    // as a JSON array of `PromptEntry` (oldest first), for the SESSIONS
    // PANEL's history lines. A bounded list on the row rather than a
    // table: it is read with every row and pruned on every write.
    // NULL (the empty history) for every row that predates the capture.
    "
    ALTER TABLE agents ADD COLUMN recent_prompts TEXT;
    ",
    // 25: the GitHub issue an ISSUE SESSION was launched for (the ISSUES
    // MODAL's prompt and preset launches). Nullable and request-driven
    // like `pr_url`: every existing AGENT remains an ordinary session, and
    // an issue-created one rebuilds its issue context on every spawn.
    "
    ALTER TABLE agents ADD COLUMN issue_url TEXT;
    ",
    // 26: the command a RUN TERMINAL runs (`r` on a worktree starts
    // `.nebula.json`'s `run`). Nullable: every existing terminal stays a
    // plain shell tab.
    "
    ALTER TABLE terminals ADD COLUMN run_command TEXT;
    ",
    // 27: the custom harness registry id for `AgentKind::Custom` rows.
    // Nullable: every built-in harness reads its kind column alone.
    "
    ALTER TABLE agents ADD COLUMN custom_harness TEXT;
    ",
    // 28: workspaces are gone (13 created them, 14 let one repo sit in
    // several) — every project is in one list again. A repo that sat in
    // two workspaces was two rows: the older row survives and takes the
    // other's checkouts, and a checkout both rows knew (their root, at the
    // least) keeps the survivor's row, with the duplicate's sessions,
    // terminals and links moved onto it before the duplicate goes. The
    // projects table is rebuilt without its workspace column and with
    // repo_path UNIQUE again, as it was before 14, and the order is
    // renumbered workspace by workspace — oldest workspace first — so the
    // one list reads the way the groups did. Runs with foreign keys off
    // (see migrate()) so the DROP doesn't cascade into the children.
    "
    CREATE TEMP TABLE project_merge AS
      SELECT p.id AS dup,
        (SELECT s.id FROM projects s WHERE s.repo_path = p.repo_path
          ORDER BY s.created_at, s.id LIMIT 1) AS keep
      FROM projects p;
    DELETE FROM project_merge WHERE dup = keep;
    CREATE TEMP TABLE worktree_home AS
      SELECT w.id AS id, w.path AS path, w.created_at AS created_at,
        COALESCE(m.keep, w.project_id) AS project,
        (m.dup IS NOT NULL) AS moved
      FROM worktrees w LEFT JOIN project_merge m ON m.dup = w.project_id;
    CREATE TEMP TABLE worktree_merge AS
      SELECT h.id AS dup,
        (SELECT c.id FROM worktree_home c WHERE c.project = h.project AND c.path = h.path
          ORDER BY c.moved, c.created_at, c.id LIMIT 1) AS keep
      FROM worktree_home h WHERE h.moved;
    DELETE FROM worktree_merge WHERE dup = keep;
    UPDATE agents SET worktree_id = (SELECT keep FROM worktree_merge WHERE dup = agents.worktree_id)
      WHERE worktree_id IN (SELECT dup FROM worktree_merge);
    UPDATE terminals SET worktree_id = (SELECT keep FROM worktree_merge WHERE dup = terminals.worktree_id)
      WHERE worktree_id IN (SELECT dup FROM worktree_merge);
    UPDATE links SET worktree_id = (SELECT keep FROM worktree_merge WHERE dup = links.worktree_id)
      WHERE worktree_id IN (SELECT dup FROM worktree_merge);
    DELETE FROM worktrees WHERE id IN (SELECT dup FROM worktree_merge);
    UPDATE worktrees SET is_main = 0
      WHERE is_main = 1 AND project_id IN (SELECT dup FROM project_merge);
    UPDATE worktrees SET project_id = (SELECT keep FROM project_merge WHERE dup = worktrees.project_id)
      WHERE project_id IN (SELECT dup FROM project_merge);
    CREATE TABLE projects_new (
      id          TEXT PRIMARY KEY,
      name        TEXT NOT NULL,
      repo_path   TEXT NOT NULL UNIQUE,
      sort_order  INTEGER NOT NULL DEFAULT 0,
      created_at  INTEGER NOT NULL
    );
    INSERT INTO projects_new (id, name, repo_path, sort_order, created_at)
      SELECT p.id, p.name, p.repo_path,
        ROW_NUMBER() OVER (ORDER BY COALESCE(w.created_at, 0), p.sort_order, p.created_at, p.id) - 1,
        p.created_at
      FROM projects p LEFT JOIN workspaces w ON w.id = COALESCE(p.workspace_id, 'default')
      WHERE p.id NOT IN (SELECT dup FROM project_merge);
    DROP TABLE projects;
    ALTER TABLE projects_new RENAME TO projects;
    DROP TABLE workspaces;
    DROP TABLE temp.project_merge;
    DROP TABLE temp.worktree_home;
    DROP TABLE temp.worktree_merge;
    ",
    // 29: privileged orchestrator role. Existing agents are ordinary workers.
    "
    ALTER TABLE agents ADD COLUMN role TEXT NOT NULL DEFAULT 'worker';
    ",
    // 30: the PROFILE a session runs under, by name. Nullable: every
    // existing session keeps the daemon's own environment.
    "
    ALTER TABLE agents ADD COLUMN profile TEXT;
    ",
    // 31: each worktree's PORT BLOCK slot (`nebula_core::ports`). No
    // foreign key: the delete hook still reads a removed worktree's block,
    // and the row goes after it; a slot whose worktree is gone any other
    // way is freed the next time one is handed out.
    "
    CREATE TABLE worktree_ports (
      worktree_id TEXT PRIMARY KEY,
      slot        INTEGER NOT NULL UNIQUE
    );
    ",
    // 32: the session a `kete spawn` child was started from. No foreign
    // key: a child outlives its parent row, and reads as a root then.
    "
    ALTER TABLE agents ADD COLUMN parent TEXT;
    ",
    // 33: at most one ORCHESTRATOR per machine. Rows minted before the
    // daemon enforced it keep the newest live one; the rest go back to
    // workers, and a partial unique index makes a second impossible from
    // any path.
    "
    UPDATE agents SET role = 'worker'
      WHERE role = 'orchestrator' AND id NOT IN (
        SELECT id FROM agents WHERE role = 'orchestrator'
        ORDER BY archived ASC, created_at DESC, id DESC LIMIT 1
      );
    CREATE UNIQUE INDEX agents_one_orchestrator ON agents(role)
      WHERE role = 'orchestrator';
    ",
    // 34: the base a worktree was cut from, recorded at the cut
    // (`nebula_core::review_base::RecordedBase`), and each session's
    // checkout snapshots (`nebula_core::session_snapshot`): a NULL tree
    // with its `_at` set is one that could not be taken. Worktrees and
    // sessions from before have no row, and nothing is backfilled.
    "
    CREATE TABLE worktree_bases (
      worktree_id TEXT PRIMARY KEY REFERENCES worktrees(id) ON DELETE CASCADE,
      refname     TEXT NOT NULL,
      sha         TEXT NOT NULL
    );
    CREATE TABLE session_snapshots (
      agent_id    TEXT PRIMARY KEY REFERENCES agents(id) ON DELETE CASCADE,
      worktree_id TEXT NOT NULL,
      start_head  TEXT,
      start_tree  TEXT,
      start_at    INTEGER NOT NULL,
      last_head   TEXT,
      last_tree   TEXT,
      last_at     INTEGER
    );
    ",
    // 35: per-turn snapshots (`nebula_core::session_snapshot`, module
    // doc): each turn's start and end trees, the session's folded base and
    // tip, and this data dir's daemon id, which names the namespace its
    // snapshot refs live under. A 0.51 session's start..last becomes its
    // first base and tip, so what it did before the upgrade still shows.
    "
    ALTER TABLE session_snapshots ADD COLUMN base_tree TEXT;
    ALTER TABLE session_snapshots ADD COLUMN tip_tree TEXT;
    UPDATE session_snapshots SET base_tree = start_tree, tip_tree = last_tree
      WHERE start_tree IS NOT NULL AND last_tree IS NOT NULL;
    CREATE TABLE session_turns (
      agent_id   TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
      n          INTEGER NOT NULL,
      start_tree TEXT,
      end_tree   TEXT,
      start_at   INTEGER NOT NULL,
      end_at     INTEGER,
      PRIMARY KEY (agent_id, n)
    );
    CREATE TABLE daemon_identity (
      id  INTEGER PRIMARY KEY CHECK (id = 1),
      daemon_id TEXT NOT NULL
    );
    INSERT INTO daemon_identity (id, daemon_id) VALUES (1, lower(hex(randomblob(8))));
    ",
    // 36: the ROLE CARD a session launched with (`.kete/roles.toml`, by
    // id), so a resume or restart re-applies the same card. NULL for every
    // existing row and every session started without one.
    "ALTER TABLE agents ADD COLUMN role_card TEXT;",
    // 37: the USAGE LEDGER: one row per session per local day, summed at
    // each turn end. No foreign key: the ledger outlives a deleted
    // session, and `project`, `branch` and `session` (names, as they
    // were) are copied in so an old row still says what it was.
    "
    CREATE TABLE session_spend (
      agent_id           TEXT NOT NULL,
      day                TEXT NOT NULL,
      project            TEXT,
      branch             TEXT,
      session            TEXT,
      model              TEXT,
      profile            TEXT,
      role_card          TEXT,
      input_tokens       INTEGER NOT NULL DEFAULT 0,
      output_tokens      INTEGER NOT NULL DEFAULT 0,
      cache_read_tokens  INTEGER NOT NULL DEFAULT 0,
      cache_write_tokens INTEGER NOT NULL DEFAULT 0,
      cost_usd           REAL NOT NULL DEFAULT 0,
      tool_uses          INTEGER NOT NULL DEFAULT 0,
      turns              INTEGER NOT NULL DEFAULT 0,
      first_at           INTEGER NOT NULL,
      last_at            INTEGER NOT NULL,
      PRIMARY KEY (agent_id, day)
    );
    ",
];

/// One turn's spend for one session (the ledger lane builds it at a turn
/// end), added to the session's row for `day` by [`Store::add_spend`].
/// The descriptive fields are what the session was at that turn: a later
/// turn's non-empty value replaces an earlier one.
#[derive(Debug, Clone, PartialEq)]
pub struct SpendDelta {
    pub agent: AgentId,
    /// Local date, `YYYY-MM-DD`.
    pub day: String,
    /// The project's name.
    pub project: Option<String>,
    pub branch: Option<String>,
    /// The session's name.
    pub session: Option<String>,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub role_card: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: f64,
    pub tool_uses: u32,
    pub turns: u32,
    /// Epoch ms.
    pub at: i64,
}

pub struct Store {
    conn: Mutex<Connection>,
}

/// A session's snapshots as the store keeps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshots {
    pub worktree: WorktreeId,
    pub start: SnapshotPoint,
    pub last: Option<SnapshotPoint>,
    /// The folded base and tip (`session_snapshot` module doc); None until
    /// a turn with both trees has ended.
    pub base: Option<String>,
    pub tip: Option<String>,
}

/// One of a session's turns: its start, and its end once it has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTurn {
    pub n: u32,
    pub start_tree: Option<String>,
    pub end_tree: Option<String>,
    pub start_at: i64,
    pub end_at: Option<i64>,
}

/// A moment the user was at work in a session (`Store::activity_since`),
/// with what names the session's account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityMoment {
    pub profile: Option<String>,
    pub kind: AgentKind,
    pub custom_harness: Option<String>,
    /// Epoch ms.
    pub at: i64,
}

impl ActivityMoment {
    /// The ACCOUNT it counts against (`usage::account_key`).
    pub fn account(&self) -> String {
        let harness = match (&self.kind, self.custom_harness.as_deref()) {
            (AgentKind::Custom, Some(id)) => id.to_string(),
            (kind, _) => kind.as_str().to_string(),
        };
        nebula_core::usage::account_key(self.profile.as_deref(), &harness)
    }
}

pub type TreeRows = (Vec<Project>, Vec<Worktree>, Vec<Agent>, Vec<TerminalTab>);

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        // Rebuild-style migrations DROP a parent table (14 rebuilds
        // projects); with enforcement on, the DROP's implicit delete would
        // cascade into every child table. Standard SQLite rebuild procedure:
        // foreign keys off for the migration window, back on after. (On a
        // migration error the connection is abandoned with Store::open's
        // failure, so the early return never leaks a live FK-off handle.)
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        for (i, migration) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(&format!(
                "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                i + 1
            ))
            .with_context(|| format!("migration {}", i + 1))?;
        }
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    /// `DELETE FROM <table> WHERE id = ?1` — every entity delete is exactly
    /// this one statement, the schema's cascades taking the children with
    /// the row.
    fn delete_by_id(&self, table: &'static str, id: &str) -> Result<()> {
        self.conn
            .lock()
            .execute(&format!("DELETE FROM {table} WHERE id = ?1"), params![id])?;
        Ok(())
    }

    // ---- projects ----

    pub fn insert_project(&self, p: &Project) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO projects (id, name, repo_path, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![p.id.as_str(), p.name, p.repo_path.to_string_lossy(), p.sort_order, now_ms()],
        )?;
        Ok(())
    }

    /// Sort slot for a newly added project: after everything else.
    pub fn next_project_sort_order(&self) -> Result<i64> {
        Ok(self.conn.lock().query_row(
            "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM projects",
            [],
            |r| r.get(0),
        )?)
    }

    pub fn rename_project(&self, id: &ProjectId, name: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE projects SET name = ?2 WHERE id = ?1",
            params![id.as_str(), name],
        )?;
        Ok(())
    }

    /// Re-point a project at `repo_path`, retitled `name`, with each
    /// `(worktree, path)` pair's row moved to its new path — one transaction,
    /// so a failure leaves the project wholly where it was.
    pub fn set_project_path(
        &self,
        id: &ProjectId,
        repo_path: &Path,
        name: &str,
        worktrees: &[(WorktreeId, PathBuf)],
    ) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE projects SET repo_path = ?2, name = ?3 WHERE id = ?1",
            params![id.as_str(), repo_path.to_string_lossy(), name],
        )?;
        for (wt, path) in worktrees {
            tx.execute(
                "UPDATE worktrees SET path = ?2 WHERE id = ?1",
                params![wt.as_str(), path.to_string_lossy()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_project(&self, id: &ProjectId) -> Result<()> {
        self.delete_by_id("projects", id.as_str())
    }

    /// The project row registered for the repo at `path`, if any — one
    /// repo is one project (`repo_path` is UNIQUE).
    pub fn project_by_path(&self, path: &Path) -> Result<Option<ProjectId>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id FROM projects WHERE repo_path = ?1")?;
        let mut rows = stmt.query(params![path.to_string_lossy()])?;
        Ok(rows
            .next()?
            .map(|r| r.get::<_, String>(0))
            .transpose()?
            .map(ProjectId))
    }

    // ---- worktrees ----

    pub fn insert_worktree(&self, w: &Worktree) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO worktrees (id, project_id, path, branch, is_main, sort_order, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                w.id.as_str(),
                w.project_id.as_str(),
                w.path.to_string_lossy(),
                w.branch,
                w.is_main as i64,
                w.sort_order,
                now_ms()
            ],
        )?;
        Ok(())
    }

    /// Insert a new worktree row, returning it as stored: in the slot after
    /// its project's last one when the user has ordered them, else at 0
    /// ([`NEXT_SLOT`]).
    pub fn append_worktree(&self, mut w: Worktree) -> Result<Worktree> {
        w.sort_order = self.conn.lock().query_row(
            &format!("SELECT {NEXT_SLOT} FROM worktrees WHERE project_id = ?1"),
            params![w.project_id.as_str()],
            |r| r.get(0),
        )?;
        self.insert_worktree(&w)?;
        Ok(w)
    }

    pub fn delete_worktree(&self, id: &WorktreeId) -> Result<()> {
        self.delete_by_id("worktrees", id.as_str())
    }

    pub fn update_worktree_branch(&self, id: &WorktreeId, branch: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE worktrees SET branch = ?2 WHERE id = ?1",
            params![id.as_str(), branch],
        )?;
        Ok(())
    }

    /// Root-ness is derived from git's own checkout list on every reconcile
    /// rather than frozen at insert time, so it needs to be writable.
    pub fn set_worktree_main(&self, id: &WorktreeId, is_main: bool) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE worktrees SET is_main = ?2 WHERE id = ?1",
            params![id.as_str(), is_main as i64],
        )?;
        Ok(())
    }

    // ---- agents ----

    /// Sort slot for a new session in `worktree` ([`NEXT_SLOT`]).
    pub fn next_agent_sort_order(&self, worktree: &WorktreeId) -> Result<i64> {
        Ok(self.conn.lock().query_row(
            &format!("SELECT {NEXT_SLOT} FROM agents WHERE worktree_id = ?1"),
            params![worktree.as_str()],
            |r| r.get(0),
        )?)
    }

    pub fn insert_agent(&self, a: &Agent) -> Result<()> {
        self.insert_agent_with_launch_context(a, false, None, None)
    }

    /// `auto_title` marks the row as awaiting one agent-driven title
    /// (`nebula rename` from inside the CLI). The flag is store-internal:
    /// clients never see it, they only observe the eventual rename.
    pub fn insert_agent_with_auto_title(&self, a: &Agent, auto_title: bool) -> Result<()> {
        self.insert_agent_with_launch_context(a, auto_title, None, None)
    }

    /// Persist an AGENT plus the launch context that must be rebuilt on
    /// every process spawn. `pr_url` is intentionally not part of the
    /// shared Agent entity: it constrains the CLI's launch, not row
    /// display. `issue_url` is launch context too, and also rides the
    /// entity (`Agent::issue_url`) so the TUI's `⇧I` can open the issue.
    pub fn insert_agent_with_launch_context(
        &self,
        a: &Agent,
        auto_title: bool,
        pr_url: Option<&str>,
        issue_url: Option<&str>,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO agents (id, worktree_id, name, status, archived, archived_at, kind, claude_session_id, sort_order, created_at, status_changed_at, model, effort, auto_title_pending, unseen, cloud_session_id, pr_url, issue_url, custom_harness, role, profile, parent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
            params![
                a.id.as_str(),
                a.worktree_id.as_str(),
                a.name,
                a.status.as_str(),
                a.archived as i64,
                a.archived_at,
                a.kind.as_str(),
                a.session_id,
                a.sort_order,
                now_ms(),
                a.status_changed_at,
                a.model,
                a.effort,
                auto_title as i64,
                a.unseen as i64,
                a.cloud_session_id,
                pr_url,
                issue_url,
                a.custom_harness,
                a.role.as_str(),
                a.profile,
                a.parent.as_ref().map(AgentId::as_str),
            ],
        )?;
        Ok(())
    }

    /// PR launch context for an AGENT, or None for an ordinary/pre-existing
    /// row. A missing row also returns None; the spawn path has already
    /// resolved the Agent itself before asking for this adjunct.
    pub fn agent_pr_url(&self, id: &AgentId) -> Result<Option<String>> {
        self.agent_text_column(id, "pr_url")
    }

    /// Issue launch context for an AGENT (an ISSUE SESSION), or None.
    pub fn agent_issue_url(&self, id: &AgentId) -> Result<Option<String>> {
        self.agent_text_column(id, "issue_url")
    }

    fn agent_text_column(&self, id: &AgentId, column: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!("SELECT {column} FROM agents WHERE id = ?1"))?;
        let mut rows = stmt.query(params![id.as_str()])?;
        match rows.next()? {
            Some(row) => Ok(row.get(0)?),
            None => Ok(None),
        }
    }

    /// Pin `id` as the one ORCHESTRATOR — or, with None, nobody — in one
    /// transaction: every other row holding the role (archived ones too, so
    /// an unarchive can never bring back a second) is demoted first.
    /// Returns every row whose role changed, for the caller to broadcast.
    pub fn set_orchestrator(&self, id: Option<&AgentId>) -> Result<Vec<AgentId>> {
        let keep = id.map(|i| i.as_str());
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        if let Some(id) = id {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM agents WHERE id = ?1)",
                params![id.as_str()],
                |r| r.get(0),
            )?;
            if !exists {
                anyhow::bail!("agent not found");
            }
        }
        let mut changed = Vec::new();
        {
            let mut stmt =
                tx.prepare("SELECT id FROM agents WHERE role = 'orchestrator' AND id IS NOT ?1")?;
            let rows = stmt.query_map(params![keep], |r| r.get::<_, String>(0))?;
            for row in rows {
                changed.push(AgentId(row?));
            }
        }
        tx.execute(
            "UPDATE agents SET role = 'worker' WHERE role = 'orchestrator' AND id IS NOT ?1",
            params![keep],
        )?;
        if let Some(id) = id {
            let promoted = tx.execute(
                "UPDATE agents SET role = 'orchestrator' WHERE id = ?1 AND role != 'orchestrator'",
                params![id.as_str()],
            )?;
            if promoted == 1 {
                changed.push(id.clone());
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Record (or with None, clear) the ROLE CARD `id` launched with.
    pub fn set_agent_role_card(&self, id: &AgentId, card: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE agents SET role_card = ?2 WHERE id = ?1",
            params![id.as_str(), card],
        )?;
        Ok(())
    }

    /// Every session row holding a role card, archived ones too, in
    /// creation order.
    pub fn agent_role_cards(&self) -> Result<Vec<(AgentId, String)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, role_card FROM agents WHERE role_card IS NOT NULL ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([], |r| Ok((AgentId(r.get(0)?), r.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Add one turn's spend to `d.agent`'s row for `d.day`, creating it
    /// on the day's first turn, and return the row as it now stands.
    pub fn add_spend(&self, d: &SpendDelta) -> Result<SpendRow> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO session_spend (agent_id, day, project, branch, session, model, profile,
                role_card, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                cost_usd, tool_uses, turns, first_at, last_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)
             ON CONFLICT (agent_id, day) DO UPDATE SET
                project = COALESCE(excluded.project, project),
                branch = COALESCE(excluded.branch, branch),
                session = COALESCE(excluded.session, session),
                model = COALESCE(excluded.model, model),
                profile = COALESCE(excluded.profile, profile),
                role_card = COALESCE(excluded.role_card, role_card),
                input_tokens = input_tokens + excluded.input_tokens,
                output_tokens = output_tokens + excluded.output_tokens,
                cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
                cache_write_tokens = cache_write_tokens + excluded.cache_write_tokens,
                cost_usd = cost_usd + excluded.cost_usd,
                tool_uses = tool_uses + excluded.tool_uses,
                turns = turns + excluded.turns,
                last_at = MAX(last_at, excluded.last_at)",
            params![
                d.agent.as_str(),
                d.day,
                d.project,
                d.branch,
                d.session,
                d.model,
                d.profile,
                d.role_card,
                d.input_tokens as i64,
                d.output_tokens as i64,
                d.cache_read_tokens as i64,
                d.cache_write_tokens as i64,
                d.cost_usd,
                d.tool_uses as i64,
                d.turns as i64,
                d.at,
            ],
        )?;
        Ok(conn.query_row(
            &format!(
                "SELECT {SPEND_COLUMNS} FROM session_spend s WHERE agent_id = ?1 AND day = ?2"
            ),
            params![d.agent.as_str(), d.day],
            row_to_spend,
        )?)
    }

    /// The ledger: every session's row per day from `since` to `until`
    /// (local dates, `YYYY-MM-DD`, inclusive; None leaves that end open),
    /// for one project or all, by day, project and session. A session still
    /// in the tree says what it is now (name, status, pull request); a
    /// deleted one says what its rows recorded.
    pub fn spend_rows(
        &self,
        since: Option<&str>,
        until: Option<&str>,
        project: Option<&ProjectId>,
    ) -> Result<Vec<LedgerRow>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SPEND_COLUMNS},
                    COALESCE(a.name, s.session, s.agent_id),
                    COALESCE(p.name, s.project, ''),
                    COALESCE(s.branch, w.branch),
                    COALESCE(s.role_card, a.role_card),
                    COALESCE(s.profile, a.profile),
                    COALESCE(s.model, a.model),
                    a.status,
                    a.pr_url
             FROM session_spend s
             LEFT JOIN agents a ON a.id = s.agent_id
             LEFT JOIN worktrees w ON w.id = a.worktree_id
             LEFT JOIN projects p ON p.id = w.project_id
             WHERE (?1 IS NULL OR s.day >= ?1)
               AND (?2 IS NULL OR s.day <= ?2)
               AND (?3 IS NULL OR p.id = ?3
                    OR (p.id IS NULL AND s.project = (SELECT name FROM projects WHERE id = ?3)))
             ORDER BY s.day, COALESCE(p.name, s.project, ''), COALESCE(a.name, s.session, s.agent_id)"
        ))?;
        let rows = stmt.query_map(params![since, until, project.map(ProjectId::as_str)], |r| {
            Ok(LedgerRow {
                spend: row_to_spend(r)?,
                session: r.get(9)?,
                project: r.get(10)?,
                branch: r.get(11)?,
                role_card: r.get(12)?,
                profile: r.get(13)?,
                model: r.get(14)?,
                status: r
                    .get::<_, Option<String>>(15)?
                    .and_then(|s| AgentStatus::parse(&s))
                    .unwrap_or(AgentStatus::Terminated),
                pr_url: r.get(16)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The row holding the ORCHESTRATOR role, archived or not.
    pub fn orchestrator(&self) -> Result<Option<Agent>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE role = 'orchestrator'"
        ))?;
        let mut rows = stmt.query([])?;
        Ok(rows.next()?.map(row_to_agent).transpose()?)
    }

    /// User rename: always applies, and retires any pending auto-title so a
    /// late agent attempt can't clobber the user's choice.
    pub fn rename_agent(&self, id: &AgentId, name: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE agents SET name = ?2, auto_title_pending = 0 WHERE id = ?1",
            params![id.as_str(), name],
        )?;
        Ok(())
    }

    /// Agent rename: applies only while the auto-title is still pending
    /// (single atomic conditional update — concurrent attempts can't both
    /// win). Returns whether the rename was applied.
    pub fn rename_agent_if_auto_pending(&self, id: &AgentId, name: &str) -> Result<bool> {
        let changed = self.conn.lock().execute(
            "UPDATE agents SET name = ?2, auto_title_pending = 0 WHERE id = ?1 AND auto_title_pending = 1",
            params![id.as_str(), name],
        )?;
        Ok(changed == 1)
    }

    /// Whether the session still awaits its agent-driven auto-title (drives
    /// the hook server's decision to inject the titling instruction).
    pub fn agent_auto_title_pending(&self, id: &AgentId) -> Result<bool> {
        let pending: Option<i64> = self
            .conn
            .lock()
            .query_row(
                "SELECT auto_title_pending FROM agents WHERE id = ?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        Ok(pending == Some(1))
    }

    /// The title last seen from (or pushed into) Claude for this session;
    /// `None` until the CLAUDE TITLE SYNC has run once.
    /// Append one prompt to the row's RECENT PROMPTS, keeping only the
    /// newest [`RECENT_PROMPTS_KEPT`]. Returns whether a row was there to
    /// take it.
    pub fn push_prompt(&self, id: &AgentId, entry: &PromptEntry) -> Result<bool> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT recent_prompts FROM agents WHERE id = ?1")?;
        let mut rows = stmt.query(params![id.as_str()])?;
        let Some(row) = rows.next()? else {
            return Ok(false);
        };
        let mut prompts = parse_prompts(row.get::<_, Option<String>>(0)?.as_deref());
        drop(rows);
        drop(stmt);
        prompts.push(entry.clone());
        if prompts.len() > RECENT_PROMPTS_KEPT {
            prompts.drain(..prompts.len() - RECENT_PROMPTS_KEPT);
        }
        let json = serde_json::to_string(&prompts)?;
        conn.execute(
            "UPDATE agents SET recent_prompts = ?2 WHERE id = ?1",
            params![id.as_str(), json],
        )?;
        Ok(true)
    }

    pub fn agent_claude_title(&self, id: &AgentId) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT claude_title FROM agents WHERE id = ?1")?;
        let mut rows = stmt.query(params![id.as_str()])?;
        match rows.next()? {
            Some(row) => Ok(row.get(0)?),
            None => Ok(None),
        }
    }

    /// Claude's own title for the session, as just read from disk: when it
    /// is not the one last seen from Claude, the row takes it as a user
    /// rename (retiring any pending auto-title) and remembers it. The
    /// comparison is deliberately against `claude_title`, not `name`, so a
    /// name the user set in nebula since is never undone by re-reading
    /// Claude's older title. Returns whether anything changed.
    pub fn adopt_claude_title(&self, id: &AgentId, title: &str) -> Result<bool> {
        let changed = self.conn.lock().execute(
            "UPDATE agents SET name = ?2, claude_title = ?2, auto_title_pending = 0 \
             WHERE id = ?1 AND (claude_title IS NULL OR claude_title != ?2)",
            params![id.as_str(), title],
        )?;
        Ok(changed == 1)
    }

    /// Everything the hook reply needs to decide whether to push the row's
    /// name into Claude (`TitleState::to_push`); `None` for an unknown id.
    pub fn agent_title_state(&self, id: &AgentId) -> Result<Option<TitleState>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT name, claude_title, auto_title_pending, kind, custom_harness FROM agents WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id.as_str()])?;
        match rows.next()? {
            Some(row) => Ok(Some(TitleState {
                name: row.get(0)?,
                claude_title: row.get(1)?,
                auto_title_pending: row.get::<_, i64>(2)? != 0,
                kind: parse_agent_kind(&row.get::<_, String>(3)?),
                custom_harness: row.get(4)?,
            })),
            None => Ok(None),
        }
    }

    /// Re-home a session's row. A row that changes checkout takes the slot
    /// a new one there would ([`NEXT_SLOT`]); one already there keeps its
    /// place.
    pub fn set_agent_worktree(&self, id: &AgentId, worktree_id: &WorktreeId) -> Result<()> {
        self.conn.lock().execute(
            &format!(
                "UPDATE agents SET worktree_id = ?2,
                   sort_order = (SELECT {NEXT_SLOT} FROM agents WHERE worktree_id = ?2 AND id != ?1)
                 WHERE id = ?1 AND worktree_id != ?2"
            ),
            params![id.as_str(), worktree_id.as_str()],
        )?;
        Ok(())
    }

    pub fn set_agent_archived(&self, id: &AgentId, archived: bool) -> Result<()> {
        // Stamp the archive time (cleared on unarchive) so the TUI can
        // order the ARCHIVED group newest-first.
        let archived_at = if archived { now_ms() } else { 0 };
        // An archived row is out of sight by definition: nothing left to
        // go and read, so its unseen-finish flag goes with it.
        self.conn.lock().execute(
            "UPDATE agents SET archived = ?2, archived_at = ?3,
                    unseen = CASE WHEN ?2 THEN 0 ELSE unseen END
             WHERE id = ?1",
            params![id.as_str(), archived as i64, archived_at],
        )?;
        Ok(())
    }

    /// Returns the epoch-ms stamp written to `status_changed_at` and the
    /// row's `unseen` flag after the change, so the caller can broadcast
    /// exactly what it persisted.
    ///
    /// The flag is maintained here, atomically with the status it
    /// qualifies: a live turn (running or needs-feedback) landing on
    /// `finished` raises it — that is the yellow-to-green flip nobody may
    /// have been watching — staying on `finished` keeps it, and leaving
    /// `finished` (a new prompt, a restart, a disconnect) drops it, since
    /// there is no finished turn left to read. Archived rows never raise
    /// it: they are out of sight already.
    pub fn set_agent_status(&self, id: &AgentId, status: AgentStatus) -> Result<(i64, bool)> {
        let stamp = now_ms();
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE agents SET status = ?2, status_changed_at = ?3,
                    unseen = CASE
                      WHEN ?2 = 'finished' THEN
                        CASE WHEN status IN ('running', 'needs_feedback') AND archived = 0
                             THEN 1 ELSE unseen END
                      ELSE 0
                    END
             WHERE id = ?1",
            params![id.as_str(), status.as_str(), stamp],
        )?;
        let unseen: i64 = conn
            .query_row(
                "SELECT unseen FROM agents WHERE id = ?1",
                params![id.as_str()],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok((stamp, unseen != 0))
    }

    /// The agent's session is on screen: drop its unseen-finish flag.
    /// Returns whether the flag was actually set, so the caller can skip
    /// broadcasting a row that didn't change.
    pub fn mark_agent_seen(&self, id: &AgentId) -> Result<bool> {
        let changed = self.conn.lock().execute(
            "UPDATE agents SET unseen = 0 WHERE id = ?1 AND unseen = 1",
            params![id.as_str()],
        )?;
        Ok(changed > 0)
    }

    pub fn set_agent_cloud_session_id(
        &self,
        id: &AgentId,
        cloud_session_id: Option<&str>,
    ) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE agents SET cloud_session_id = ?2 WHERE id = ?1",
            params![id.as_str(), cloud_session_id],
        )?;
        Ok(())
    }

    pub fn set_agent_session_id(&self, id: &AgentId, session_id: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE agents SET claude_session_id = ?2 WHERE id = ?1",
            params![id.as_str(), session_id],
        )?;
        Ok(())
    }

    /// The account the agent runs under (`SwitchAgentAccount`): a profile
    /// name, or None for its harness's own login.
    pub fn set_agent_profile(&self, id: &AgentId, profile: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE agents SET profile = ?2 WHERE id = ?1",
            params![id.as_str(), profile],
        )?;
        Ok(())
    }

    /// Every row naming profile `old` names `new` instead
    /// (`RenameProfileSessions`), archived ones included: the rows changed.
    pub fn rename_agent_profile(&self, old: &str, new: &str) -> Result<Vec<AgentId>> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let ids = {
            let mut stmt = tx.prepare("SELECT id FROM agents WHERE trim(profile) = ?1")?;
            let rows = stmt.query_map(params![old], |r| r.get::<_, String>(0))?;
            rows.map(|r| r.map(AgentId))
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.execute(
            "UPDATE agents SET profile = ?2 WHERE trim(profile) = ?1",
            params![old, new],
        )?;
        tx.commit()?;
        Ok(ids)
    }

    pub fn set_agent_model(&self, id: &AgentId, model: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE agents SET model = ?2 WHERE id = ?1",
            params![id.as_str(), model],
        )?;
        Ok(())
    }

    /// The effort the next spawn or resume passes (`--effort`): the
    /// launch's choice, or what Claude reported it runs at since.
    pub fn set_agent_effort(&self, id: &AgentId, effort: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE agents SET effort = ?2 WHERE id = ?1",
            params![id.as_str(), effort],
        )?;
        Ok(())
    }

    pub fn delete_agent(&self, id: &AgentId) -> Result<()> {
        self.delete_by_id("agents", id.as_str())
    }

    /// Boot sweep: agents whose PTYs died with the previous daemon. Those in
    /// `carried` did not — an IN-PLACE RESTART brought them along — and
    /// keep the status they had.
    pub fn sweep_disconnected(
        &self,
        carried: &std::collections::HashSet<AgentId>,
    ) -> Result<Vec<AgentId>> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let ids: Vec<AgentId> = tx
            .prepare("SELECT id FROM agents WHERE status IN ('running', 'needs_feedback')")?
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .map(AgentId)
            .filter(|id| !carried.contains(id))
            .collect();
        let changed_at = now_ms();
        for id in &ids {
            tx.execute(
                "UPDATE agents SET status = 'disconnected', status_changed_at = ?2 WHERE id = ?1",
                params![id.as_str(), changed_at],
            )?;
        }
        tx.commit()?;
        Ok(ids)
    }

    /// Fold the WAL into the database and hold the connection, so nothing
    /// writes while the guard lives — an IN-PLACE RESTART execs with it
    /// held, and the new image opens a database no write is halfway into.
    pub fn quiesce(&self) -> parking_lot::MutexGuard<'_, Connection> {
        let conn = self.conn.lock();
        if let Err(e) = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())) {
            tracing::warn!(error = %e, "wal checkpoint before restart failed");
        }
        conn
    }

    // ---- terminals ----

    pub fn insert_terminal(&self, t: &TerminalTab) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO terminals (id, worktree_id, name, sort_order, created_at, run_command) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![t.id.as_str(), t.worktree_id.as_str(), t.name, t.sort_order, now_ms(), t.run_command],
        )?;
        Ok(())
    }

    pub fn rename_terminal(&self, id: &TerminalId, name: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE terminals SET name = ?2 WHERE id = ?1",
            params![id.as_str(), name],
        )?;
        Ok(())
    }

    pub fn delete_terminal(&self, id: &TerminalId) -> Result<()> {
        self.delete_by_id("terminals", id.as_str())
    }

    /// Point a RUN TERMINAL at the command it runs next: `.nebula.json` is
    /// read fresh at every `r`, so a restart picks up an edited file.
    pub fn set_terminal_run_command(&self, id: &TerminalId, command: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE terminals SET run_command = ?2 WHERE id = ?1",
            params![id.as_str(), command],
        )?;
        Ok(())
    }

    // ---- links ----

    /// Test seeding only: nothing creates a link any more (the request
    /// that did is gone), but rows older databases hold are still read,
    /// edited and deleted.
    #[cfg(test)]
    pub fn insert_link(&self, l: &Link) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO links (id, worktree_id, url, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                l.id.as_str(),
                l.worktree_id.as_str(),
                l.url,
                l.sort_order,
                now_ms()
            ],
        )?;
        Ok(())
    }

    /// Sort slot for a new link: after everything else on its worktree.
    #[cfg(test)]
    pub fn next_link_sort_order(&self, worktree_id: &WorktreeId) -> Result<i64> {
        Ok(self.conn.lock().query_row(
            "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM links WHERE worktree_id = ?1",
            params![worktree_id.as_str()],
            |r| r.get(0),
        )?)
    }

    pub fn set_link_url(&self, id: &LinkId, url: &str) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE links SET url = ?2 WHERE id = ?1",
            params![id.as_str(), url],
        )?;
        Ok(())
    }

    pub fn delete_link(&self, id: &LinkId) -> Result<()> {
        self.delete_by_id("links", id.as_str())
    }

    pub fn get_link(&self, id: &LinkId) -> Result<Option<Link>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!("SELECT {LINK_COLUMNS} FROM links WHERE id = ?1"))?;
        let mut rows = stmt.query(params![id.as_str()])?;
        Ok(rows.next()?.map(row_to_link).transpose()?)
    }

    /// Every link, in per-worktree list order.
    pub fn load_links(&self) -> Result<Vec<Link>> {
        let conn = self.conn.lock();
        let links = conn
            .prepare(&format!(
                "SELECT {LINK_COLUMNS} FROM links ORDER BY worktree_id, sort_order, created_at"
            ))?
            .query_map([], row_to_link)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(links)
    }

    // ---- pull-request read marks ----

    /// Remember that this pull request's conversation has been read up to
    /// `marker`. Idempotent, and an empty marker is a real answer: it says
    /// the PR was opened while nobody had posted on it yet.
    pub fn mark_pr_seen(&self, url: &str, marker: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO pr_seen (url, marker, seen_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(url) DO UPDATE SET marker = excluded.marker, seen_at = excluded.seen_at",
            params![url, marker, now_ms()],
        )?;
        Ok(())
    }

    pub fn load_pr_seen(&self) -> Result<Vec<PrSeen>> {
        let conn = self.conn.lock();
        let seen = conn
            .prepare("SELECT url, marker FROM pr_seen")?
            .query_map([], |r| {
                Ok(PrSeen {
                    url: r.get(0)?,
                    marker: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(seen)
    }

    // ---- point lookups ----

    pub fn get_project(&self, id: &ProjectId) -> Result<Option<Project>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"
        ))?;
        let mut rows = stmt.query(params![id.as_str()])?;
        Ok(rows.next()?.map(row_to_project).transpose()?)
    }

    pub fn get_worktree(&self, id: &WorktreeId) -> Result<Option<Worktree>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {WORKTREE_COLUMNS} FROM worktrees WHERE id = ?1"
        ))?;
        let mut rows = stmt.query(params![id.as_str()])?;
        Ok(rows.next()?.map(row_to_worktree).transpose()?)
    }

    // ---- worktree ports ----

    /// The PORT BLOCK slot `worktree` holds, if it has one yet.
    pub fn worktree_port_slot(&self, worktree: &WorktreeId) -> Result<Option<u32>> {
        let conn = self.conn.lock();
        let slot: Option<i64> = conn
            .query_row(
                "SELECT slot FROM worktree_ports WHERE worktree_id = ?1",
                params![worktree.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(slot.map(|s| s as u32))
    }

    /// The slot `worktree` holds, handing it the lowest free one first if it
    /// has none — so a deleted worktree's slot is reused and nobody else's
    /// block ever moves. Slots of worktrees no longer in the table are freed
    /// on the way.
    pub fn allocate_port_slot(&self, worktree: &WorktreeId) -> Result<u32> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM worktree_ports WHERE worktree_id NOT IN (SELECT id FROM worktrees)",
            [],
        )?;
        let held: Option<i64> = tx
            .query_row(
                "SELECT slot FROM worktree_ports WHERE worktree_id = ?1",
                params![worktree.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(slot) = held {
            tx.commit()?;
            return Ok(slot as u32);
        }
        let taken: Vec<i64> = {
            let mut stmt = tx.prepare("SELECT slot FROM worktree_ports ORDER BY slot")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let slot = (0..).find(|s| !taken.contains(s)).unwrap_or(0);
        tx.execute(
            "INSERT INTO worktree_ports (worktree_id, slot) VALUES (?1, ?2)",
            params![worktree.as_str(), slot],
        )?;
        tx.commit()?;
        Ok(slot as u32)
    }

    /// Give `worktree`'s slot back.
    pub fn release_port_slot(&self, worktree: &WorktreeId) -> Result<()> {
        self.conn.lock().execute(
            "DELETE FROM worktree_ports WHERE worktree_id = ?1",
            params![worktree.as_str()],
        )?;
        Ok(())
    }

    /// Every allocated slot, by worktree.
    pub fn port_slots(&self) -> Result<Vec<(WorktreeId, u32)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT worktree_id, slot FROM worktree_ports")?;
        let rows = stmt.query_map([], |r| {
            Ok((WorktreeId(r.get(0)?), r.get::<_, i64>(1)? as u32))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- recorded worktree bases ----

    /// Record the base `worktree` was cut from.
    pub fn set_worktree_base(&self, worktree: &WorktreeId, refname: &str, sha: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT OR REPLACE INTO worktree_bases (worktree_id, refname, sha) VALUES (?1, ?2, ?3)",
            params![worktree.as_str(), refname, sha],
        )?;
        Ok(())
    }

    /// Every recorded base (`ServerEvent::WorktreeBases`).
    pub fn worktree_bases(&self) -> Result<Vec<RecordedBase>> {
        let conn = self.conn.lock();
        let mut stmt = conn
            .prepare("SELECT worktree_id, refname, sha FROM worktree_bases ORDER BY worktree_id")?;
        let rows = stmt.query_map([], |r| {
            Ok(RecordedBase {
                worktree: WorktreeId(r.get(0)?),
                refname: r.get(1)?,
                sha: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    // ---- session snapshots ----

    /// `agent`'s snapshots: the worktree they were taken in, its start and
    /// its last. None before the first.
    pub fn session_snapshots(&self, agent: &AgentId) -> Result<Option<SessionSnapshots>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT worktree_id, start_head, start_tree, start_at, last_head, last_tree, last_at,
                        base_tree, tip_tree
                 FROM session_snapshots WHERE agent_id = ?1",
                params![agent.as_str()],
                |r| {
                    let last_at: Option<i64> = r.get(6)?;
                    Ok(SessionSnapshots {
                        worktree: WorktreeId(r.get(0)?),
                        start: SnapshotPoint {
                            head: r.get(1)?,
                            tree: r.get(2)?,
                            at: r.get(3)?,
                        },
                        last: match last_at {
                            Some(at) => Some(SnapshotPoint {
                                head: r.get(4)?,
                                tree: r.get(5)?,
                                at,
                            }),
                            None => None,
                        },
                        base: r.get(7)?,
                        tip: r.get(8)?,
                    })
                },
            )
            .optional()?)
    }

    /// `agent`'s start snapshot, taken in `worktree`: a new row, or one
    /// that replaces snapshots taken in another worktree (the session moved),
    /// its last one, its base and tip and its turns dropped with it.
    pub fn set_snapshot_start(
        &self,
        agent: &AgentId,
        worktree: &WorktreeId,
        point: &SnapshotPoint,
    ) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO session_snapshots
               (agent_id, worktree_id, start_head, start_tree, start_at, last_head, last_tree, last_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL)",
            params![
                agent.as_str(),
                worktree.as_str(),
                point.head,
                point.tree,
                point.at
            ],
        )?;
        tx.execute(
            "DELETE FROM session_turns WHERE agent_id = ?1",
            params![agent.as_str()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// `agent`'s folded base and tip; no tip after a turn end that could
    /// not be taken.
    pub fn set_snapshot_view(&self, agent: &AgentId, base: &str, tip: Option<&str>) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE session_snapshots SET base_tree = ?2, tip_tree = ?3 WHERE agent_id = ?1",
            params![agent.as_str(), base, tip],
        )?;
        Ok(())
    }

    /// `agent`'s turn still open (started, not ended), the latest if
    /// several.
    pub fn open_turn(&self, agent: &AgentId) -> Result<Option<SessionTurn>> {
        Ok(self
            .session_turns(agent)?
            .into_iter()
            .rfind(|t| t.end_at.is_none()))
    }

    /// Open `agent`'s next turn at `point`; its number.
    pub fn begin_turn(&self, agent: &AgentId, point: &SnapshotPoint) -> Result<u32> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row(
            "SELECT COALESCE(MAX(n), 0) + 1 FROM session_turns WHERE agent_id = ?1",
            params![agent.as_str()],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO session_turns (agent_id, n, start_tree, start_at) VALUES (?1, ?2, ?3, ?4)",
            params![agent.as_str(), n, point.tree, point.at],
        )?;
        Ok(n as u32)
    }

    /// End `agent`'s turn `n` at `point`.
    pub fn end_turn(&self, agent: &AgentId, n: u32, point: &SnapshotPoint) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE session_turns SET end_tree = ?3, end_at = ?4 WHERE agent_id = ?1 AND n = ?2",
            params![agent.as_str(), n, point.tree, point.at],
        )?;
        Ok(())
    }

    /// When the user was at work, for an account's USUAL HOURS
    /// (`nebula_core::wake::activity`): every kept turn's start and every
    /// session's creation since `since_ms` (epoch ms), with the columns
    /// that name the session's account. Archived sessions count: the
    /// work happened.
    pub fn activity_since(&self, since_ms: i64) -> Result<Vec<ActivityMoment>> {
        self.activity_query(
            "SELECT a.profile, a.kind, a.custom_harness, t.start_at
               FROM session_turns t JOIN agents a ON a.id = t.agent_id
              WHERE t.start_at >= ?1
             UNION ALL
             SELECT profile, kind, custom_harness, created_at
               FROM agents WHERE created_at >= ?1",
            since_ms,
        )
    }

    /// The latest such moment per account columns, however old.
    pub fn last_activity(&self) -> Result<Vec<ActivityMoment>> {
        self.activity_query(
            "SELECT profile, kind, custom_harness, MAX(at) FROM (
               SELECT a.profile, a.kind, a.custom_harness, t.start_at AS at
                 FROM session_turns t JOIN agents a ON a.id = t.agent_id
               UNION ALL
               SELECT profile, kind, custom_harness, created_at AS at FROM agents
             ) WHERE at >= ?1 GROUP BY profile, kind, custom_harness",
            0,
        )
    }

    fn activity_query(&self, sql: &str, since_ms: i64) -> Result<Vec<ActivityMoment>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![since_ms], |r| {
            Ok(ActivityMoment {
                profile: r.get(0)?,
                kind: parse_agent_kind(&r.get::<_, String>(1)?),
                custom_harness: r.get(2)?,
                at: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `agent`'s kept turns, oldest first.
    pub fn session_turns(&self, agent: &AgentId) -> Result<Vec<SessionTurn>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT n, start_tree, end_tree, start_at, end_at FROM session_turns
             WHERE agent_id = ?1 ORDER BY n",
        )?;
        let rows = stmt.query_map(params![agent.as_str()], |r| {
            Ok(SessionTurn {
                n: r.get::<_, i64>(0)? as u32,
                start_tree: r.get(1)?,
                end_tree: r.get(2)?,
                start_at: r.get(3)?,
                end_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Drop all but `agent`'s latest `keep` turns; the numbers dropped.
    pub fn prune_turns(&self, agent: &AgentId, keep: u32) -> Result<Vec<u32>> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let gone: Vec<u32> = {
            let mut stmt = tx.prepare(
                "SELECT n FROM session_turns WHERE agent_id = ?1
                   AND n <= (SELECT MAX(n) FROM session_turns WHERE agent_id = ?1) - ?2
                 ORDER BY n",
            )?;
            let rows = stmt.query_map(params![agent.as_str(), keep], |r| {
                Ok(r.get::<_, i64>(0)? as u32)
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        if let Some(&max) = gone.last() {
            tx.execute(
                "DELETE FROM session_turns WHERE agent_id = ?1 AND n <= ?2",
                params![agent.as_str(), max],
            )?;
        }
        tx.commit()?;
        Ok(gone)
    }

    /// This data dir's daemon id: the namespace its snapshot refs live
    /// under. Made by migration 35; made here if that row is ever gone.
    pub fn daemon_id(&self) -> Result<String> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR IGNORE INTO daemon_identity (id, daemon_id)
             VALUES (1, lower(hex(randomblob(8))))",
            [],
        )?;
        Ok(conn.query_row(
            "SELECT daemon_id FROM daemon_identity WHERE id = 1",
            [],
            |r| r.get(0),
        )?)
    }

    /// Every agent id there is, archived ones included: the sessions whose
    /// refs a sweep keeps.
    pub fn agent_ids(&self) -> Result<std::collections::HashSet<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id FROM agents")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every project's repo path: where a sweep looks.
    pub fn project_repos(&self) -> Result<Vec<PathBuf>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT repo_path FROM projects")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0).map(PathBuf::from))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `agent`'s snapshot at its latest turn end. False when it has no
    /// start yet (nothing written).
    pub fn set_snapshot_last(&self, agent: &AgentId, point: &SnapshotPoint) -> Result<bool> {
        let n = self.conn.lock().execute(
            "UPDATE session_snapshots SET last_head = ?2, last_tree = ?3, last_at = ?4
             WHERE agent_id = ?1",
            params![agent.as_str(), point.head, point.tree, point.at],
        )?;
        Ok(n > 0)
    }

    /// Sessions with snapshots taken in `worktree`.
    pub fn snapshot_sessions_in(&self, worktree: &WorktreeId) -> Result<u32> {
        Ok(self.conn.lock().query_row(
            "SELECT COUNT(*) FROM session_snapshots WHERE worktree_id = ?1",
            params![worktree.as_str()],
            |r| r.get::<_, i64>(0),
        )? as u32)
    }

    /// Every session that has snapshots, by id.
    #[cfg(test)]
    pub fn snapshot_agents(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT agent_id FROM session_snapshots")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The sessions under `worktree` (Some) or `project` (None for the
    /// worktree) that have snapshots: what a delete of either takes with it.
    pub fn snapshot_agents_under(
        &self,
        project: Option<&ProjectId>,
        worktree: Option<&WorktreeId>,
    ) -> Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT s.agent_id FROM session_snapshots s
               JOIN agents a ON a.id = s.agent_id
               JOIN worktrees w ON w.id = a.worktree_id
             WHERE (?1 IS NULL OR w.project_id = ?1) AND (?2 IS NULL OR w.id = ?2)",
        )?;
        let rows = stmt.query_map(
            params![project.map(|p| p.as_str()), worktree.map(|w| w.as_str())],
            |r| r.get::<_, String>(0),
        )?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn get_agent(&self, id: &AgentId) -> Result<Option<Agent>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare(&format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"))?;
        let mut rows = stmt.query(params![id.as_str()])?;
        Ok(rows.next()?.map(row_to_agent).transpose()?)
    }

    pub fn get_terminal(&self, id: &TerminalId) -> Result<Option<TerminalTab>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {TERMINAL_COLUMNS} FROM terminals WHERE id = ?1"
        ))?;
        let mut rows = stmt.query(params![id.as_str()])?;
        Ok(rows.next()?.map(row_to_terminal).transpose()?)
    }

    pub fn count_terminals(&self, worktree_id: &WorktreeId) -> Result<i64> {
        Ok(self.conn.lock().query_row(
            "SELECT COUNT(*) FROM terminals WHERE worktree_id = ?1",
            params![worktree_id.as_str()],
            |r| r.get(0),
        )?)
    }

    /// The worktree's RUN TERMINALS, oldest first. The DAEMON keeps one per
    /// worktree; a list, so a stray second row can still be found and stopped.
    pub fn run_terminals_in(&self, worktree_id: &WorktreeId) -> Result<Vec<TerminalTab>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {TERMINAL_COLUMNS} FROM terminals WHERE worktree_id = ?1 AND run_command IS NOT NULL ORDER BY created_at"
        ))?;
        let rows = stmt
            .query_map(params![worktree_id.as_str()], row_to_terminal)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- user order ----

    /// Write the user's order for `scope` (`ClientRequest::Reorder`): `ids`
    /// first to last, minus any id that isn't a row of the scope and any
    /// repeat, then the rows `ids` left out in their old order. A project's
    /// main checkout stays ahead of its other worktrees: refused when `ids`
    /// puts one after a linked checkout, kept first when left out. The rows
    /// get `sort_order` 0..n in one transaction. Returns the ids whose slot
    /// changed, for the caller to broadcast.
    pub fn reorder(&self, scope: &ReorderScope, ids: &[String]) -> Result<Vec<String>> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        // (id, sort_order, is_main) in the order `load_tree` lists them, so
        // "old order" means what every client already shows.
        let (table, sql, parent) = match scope {
            ReorderScope::Projects => (
                "projects",
                "SELECT id, sort_order, 0 FROM projects ORDER BY sort_order, created_at",
                None,
            ),
            ReorderScope::Worktrees(project) => (
                "worktrees",
                "SELECT id, sort_order, is_main FROM worktrees WHERE project_id = ?1
                 ORDER BY is_main DESC, sort_order, created_at",
                Some(("projects", "project", project.as_str())),
            ),
            ReorderScope::Agents(worktree) => (
                "agents",
                "SELECT id, sort_order, 0 FROM agents WHERE worktree_id = ?1
                 ORDER BY sort_order, created_at",
                Some(("worktrees", "worktree", worktree.as_str())),
            ),
        };
        let rows: Vec<(String, i64, bool)> = {
            let mut stmt = tx.prepare(sql)?;
            let map = |r: &rusqlite::Row<'_>| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)? != 0,
                ))
            };
            match parent {
                Some((parent_table, what, id)) => {
                    let exists: bool = tx.query_row(
                        &format!("SELECT EXISTS(SELECT 1 FROM {parent_table} WHERE id = ?1)"),
                        params![id],
                        |r| r.get(0),
                    )?;
                    if !exists {
                        anyhow::bail!("{what} not found");
                    }
                    stmt.query_map(params![id], map)?
                        .collect::<rusqlite::Result<_>>()?
                }
                None => stmt.query_map([], map)?.collect::<rusqlite::Result<_>>()?,
            }
        };
        let order = user_order(&rows, ids)?;
        let mut changed = Vec::new();
        {
            let mut update =
                tx.prepare(&format!("UPDATE {table} SET sort_order = ?2 WHERE id = ?1"))?;
            for (slot, i) in order.into_iter().enumerate() {
                let (id, old, _) = &rows[i];
                let slot = slot as i64;
                if *old != slot {
                    update.execute(params![id, slot])?;
                    changed.push(id.clone());
                }
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    // ---- whole tree ----

    /// Every row, in the order clients lay them out (`ReorderScope`):
    /// `sort_order`, then creation, a project's main checkout first.
    pub fn load_tree(&self) -> Result<TreeRows> {
        let conn = self.conn.lock();

        let projects = conn
            .prepare(&format!(
                "SELECT {PROJECT_COLUMNS} FROM projects ORDER BY sort_order, created_at"
            ))?
            .query_map([], row_to_project)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let worktrees = conn
            .prepare(&format!(
                "SELECT {WORKTREE_COLUMNS} FROM worktrees ORDER BY is_main DESC, sort_order, created_at"
            ))?
            .query_map([], row_to_worktree)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let agents = conn
            .prepare(&format!(
                "SELECT {AGENT_COLUMNS} FROM agents ORDER BY sort_order, created_at"
            ))?
            .query_map([], row_to_agent)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let terminals = conn
            .prepare(&format!(
                "SELECT {TERMINAL_COLUMNS} FROM terminals ORDER BY sort_order, created_at"
            ))?
            .query_map([], row_to_terminal)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok((projects, worktrees, agents, terminals))
    }

    // ---- ui state ----

    pub fn save_ui_state(&self, json: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO ui_state (id, json) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET json = excluded.json",
            params![json],
        )?;
        Ok(())
    }

    pub fn load_ui_state(&self) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT json FROM ui_state WHERE id = 1")?;
        let mut rows = stmt.query([])?;
        Ok(rows.next()?.map(|r| r.get::<_, String>(0)).transpose()?)
    }
}

// ---- row shapes ----
//
// One column list and one row mapper per entity, shared by the point
// lookups and `load_tree`, so a row can never read differently depending
// on which path fetched it. The column order is the mapper's contract.

// Column orders the `row_to_*` mappers below read.
/// The slot a new (or moved-in) worktree or session takes in its scope:
/// after the last row when the user has ordered the scope (`Reorder`
/// leaves some row above 0), else 0 with the rest, where each client's own
/// tiebreak keeps deciding the order. Projects always append instead
/// (`next_project_sort_order`): their slots have been 0..n from the start.
const NEXT_SLOT: &str = "CASE WHEN MAX(sort_order) > 0 THEN MAX(sort_order) + 1 ELSE 0 END";

const PROJECT_COLUMNS: &str = "id, name, repo_path, sort_order";
const WORKTREE_COLUMNS: &str = "id, project_id, path, branch, is_main, sort_order";
const AGENT_COLUMNS: &str = "id, worktree_id, name, status, archived, kind, \
                             claude_session_id, sort_order, status_changed_at, model, effort, \
                             archived_at, unseen, cloud_session_id, recent_prompts, custom_harness, \
                             issue_url, role, profile, parent";
const TERMINAL_COLUMNS: &str = "id, worktree_id, name, sort_order, run_command";
/// A `session_spend` row as [`row_to_spend`] reads it (table alias `s`).
const SPEND_COLUMNS: &str = "s.agent_id, s.day, s.input_tokens, s.output_tokens, \
                             s.cache_read_tokens, s.cache_write_tokens, s.cost_usd, s.tool_uses, \
                             s.turns";

fn row_to_spend(r: &rusqlite::Row) -> rusqlite::Result<SpendRow> {
    Ok(SpendRow {
        agent: AgentId(r.get(0)?),
        day: r.get(1)?,
        input_tokens: r.get::<_, i64>(2)?.max(0) as u64,
        output_tokens: r.get::<_, i64>(3)?.max(0) as u64,
        cache_read_tokens: r.get::<_, i64>(4)?.max(0) as u64,
        cache_write_tokens: r.get::<_, i64>(5)?.max(0) as u64,
        cost_usd: r.get(6)?,
        tool_uses: r.get::<_, i64>(7)?.clamp(0, u32::MAX as i64) as u32,
        turns: r.get::<_, i64>(8)?.clamp(0, u32::MAX as i64) as u32,
    })
}
const LINK_COLUMNS: &str = "id, worktree_id, url, sort_order";

fn row_to_project(r: &rusqlite::Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: ProjectId(r.get(0)?),
        name: r.get(1)?,
        repo_path: PathBuf::from(r.get::<_, String>(2)?),
        sort_order: r.get(3)?,
    })
}

fn row_to_worktree(r: &rusqlite::Row) -> rusqlite::Result<Worktree> {
    Ok(Worktree {
        id: WorktreeId(r.get(0)?),
        project_id: ProjectId(r.get(1)?),
        path: PathBuf::from(r.get::<_, String>(2)?),
        branch: r.get(3)?,
        is_main: r.get::<_, i64>(4)? != 0,
        sort_order: r.get(5)?,
    })
}

/// `alive` is daemon state, not a column: the registry fills it in from
/// its session table after the read.
fn row_to_agent(r: &rusqlite::Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        parent: r.get::<_, Option<String>>(19)?.map(AgentId),
        profile: r.get(18)?,
        id: AgentId(r.get(0)?),
        worktree_id: WorktreeId(r.get(1)?),
        name: r.get(2)?,
        status: AgentStatus::parse(&r.get::<_, String>(3)?).unwrap_or(AgentStatus::Fresh),
        archived: r.get::<_, i64>(4)? != 0,
        kind: parse_agent_kind(&r.get::<_, String>(5)?),
        session_id: r.get(6)?,
        sort_order: r.get(7)?,
        status_changed_at: r.get(8)?,
        model: r.get(9)?,
        effort: r.get(10)?,
        archived_at: r.get(11)?,
        unseen: r.get::<_, i64>(12)? != 0,
        cloud_session_id: r.get(13)?,
        alive: false,
        issue_url: r.get(16)?,
        role: r
            .get::<_, Option<String>>(17)?
            .as_deref()
            .and_then(nebula_core::AgentRole::parse)
            .unwrap_or_default(),
        recent_prompts: parse_prompts(r.get::<_, Option<String>>(14)?.as_deref()),
        custom_harness: r.get(15)?,
    })
}

/// A stored kind string back into its kind. Bare `"custom"` never parses
/// through [`AgentKind::parse`] (a custom harness is meaningless without
/// its registry id), so the row mappers name it here instead; anything
/// else unknown reads as the default rather than failing the row load.
fn parse_agent_kind(raw: &str) -> AgentKind {
    if raw.trim() == AgentKind::Custom.as_str() {
        AgentKind::Custom
    } else {
        AgentKind::parse(raw).unwrap_or_default()
    }
}

/// The `recent_prompts` column: NULL is the empty history, and a column
/// that will not parse (a hand edit, a downgrade) reads as empty too
/// rather than failing every row load.
///
/// Injected prompts are dropped on the way out as well as on the way in
/// (`prompt_history::is_injected`): a nebula that recorded them before
/// the filter existed left them in the column, and reading is where a row
/// meets that history again. Every write goes through this same mapper
/// first, so the next prompt a session takes persists the pruned list.
fn parse_prompts(json: Option<&str>) -> Vec<PromptEntry> {
    let stored: Vec<PromptEntry> = json
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    stored
        .into_iter()
        .filter(|p| !crate::prompt_history::is_injected(&p.text))
        .collect()
}

/// `alive` is daemon state, filled in by the registry like the agent's.
fn row_to_terminal(r: &rusqlite::Row) -> rusqlite::Result<TerminalTab> {
    Ok(TerminalTab {
        id: TerminalId(r.get(0)?),
        worktree_id: WorktreeId(r.get(1)?),
        name: r.get(2)?,
        sort_order: r.get(3)?,
        alive: false,
        run_command: r.get(4)?,
    })
}

fn row_to_link(r: &rusqlite::Row) -> rusqlite::Result<Link> {
    Ok(Link {
        id: LinkId(r.get(0)?),
        worktree_id: WorktreeId(r.get(1)?),
        url: r.get(2)?,
        sort_order: r.get(3)?,
    })
}

/// [`Store::reorder`]'s new order, as indices into `rows` (`(id,
/// sort_order, is_main)` in their old order): the ids that name a row,
/// first mention only, then every row left out, mains moved ahead.
fn user_order(rows: &[(String, i64, bool)], ids: &[String]) -> Result<Vec<usize>> {
    let index: std::collections::HashMap<&str, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, (id, _, _))| (id.as_str(), i))
        .collect();
    let mut taken = vec![false; rows.len()];
    let mut order = Vec::with_capacity(rows.len());
    for id in ids {
        if let Some(&i) = index.get(id.as_str()) {
            if !std::mem::replace(&mut taken[i], true) {
                order.push(i);
            }
        }
    }
    let is_main = |i: &usize| rows[*i].2;
    if let Some(linked) = order.iter().position(|i| !is_main(i)) {
        if order[linked..].iter().any(is_main) {
            anyhow::bail!("the main checkout stays first among a project's worktrees");
        }
    }
    order.extend((0..rows.len()).filter(|&i| !taken[i]));
    // Stable: only a main left out of `ids` moves, back to the front.
    order.sort_by_key(|i| !is_main(i));
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store holding one project and worktree (`w`) for agent rows.
    fn store_with_worktree() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "demo".into(),
                repo_path: "/tmp/demo".into(),
                sort_order: 0,
            })
            .unwrap();
        store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: "/tmp/demo".into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            })
            .unwrap();
        store
    }

    fn role_agent(id: &str, role: nebula_core::AgentRole) -> Agent {
        Agent {
            parent: None,
            profile: None,
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w".into()),
            name: id.into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role,
            recent_prompts: Vec::new(),
        }
    }

    /// 0.60 is migrations 36 (`agents.role_card`) and 37
    /// (`session_spend`): a fresh store ends on the last one.
    #[test]
    fn a_fresh_store_is_at_migration_37() {
        assert_eq!(MIGRATIONS.len(), 37);
        let store = Store::open_in_memory().unwrap();
        let version: i64 = store
            .conn
            .lock()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn a_role_card_round_trips_and_clears() {
        let store = store_with_worktree();
        for id in ["a", "b", "c"] {
            store
                .insert_agent(&role_agent(id, nebula_core::AgentRole::Worker))
                .unwrap();
        }
        assert!(store.agent_role_cards().unwrap().is_empty());
        store
            .set_agent_role_card(&AgentId("b".into()), Some("runner"))
            .unwrap();
        store
            .set_agent_role_card(&AgentId("c".into()), Some("triage"))
            .unwrap();
        let mut cards = store.agent_role_cards().unwrap();
        cards.sort();
        assert_eq!(
            cards,
            [
                (AgentId("b".into()), "runner".to_string()),
                (AgentId("c".into()), "triage".to_string())
            ]
        );
        store
            .set_agent_role_card(&AgentId("b".into()), None)
            .unwrap();
        assert_eq!(
            store.agent_role_cards().unwrap(),
            [(AgentId("c".into()), "triage".to_string())]
        );
    }

    fn delta(agent: &str, day: &str, input: u64, cost: f64, at: i64) -> SpendDelta {
        SpendDelta {
            agent: AgentId(agent.into()),
            day: day.into(),
            project: Some("demo".into()),
            branch: Some("main".into()),
            session: Some(agent.into()),
            model: Some("opus".into()),
            profile: None,
            role_card: None,
            input_tokens: input,
            output_tokens: input / 10,
            cache_read_tokens: 7,
            cache_write_tokens: 3,
            cost_usd: cost,
            tool_uses: 4,
            turns: 1,
            at,
        }
    }

    #[test]
    fn add_spend_sums_two_turns_into_one_row_per_day() {
        let store = Store::open_in_memory().unwrap();
        let first = store
            .add_spend(&delta("a", "2026-10-21", 1000, 0.05, 10))
            .unwrap();
        assert_eq!((first.input_tokens, first.turns), (1000, 1));
        let mut second = delta("a", "2026-10-21", 500, 0.03, 20);
        second.profile = Some("max2".into());
        let row = store.add_spend(&second).unwrap();
        assert_eq!(
            row,
            SpendRow {
                agent: AgentId("a".into()),
                day: "2026-10-21".into(),
                input_tokens: 1500,
                output_tokens: 150,
                cache_read_tokens: 14,
                cache_write_tokens: 6,
                cost_usd: row.cost_usd,
                tool_uses: 8,
                turns: 2,
            }
        );
        assert!((row.cost_usd - 0.08).abs() < 1e-9, "{}", row.cost_usd);
        let next_day = store
            .add_spend(&delta("a", "2026-10-22", 9, 0.0, 30))
            .unwrap();
        assert_eq!((next_day.input_tokens, next_day.turns), (9, 1));
        let rows = store.spend_rows(None, None, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].profile.as_deref(),
            Some("max2"),
            "a later value fills in"
        );
        assert_eq!(
            rows[0].status,
            AgentStatus::Terminated,
            "no row in the tree"
        );
        assert_eq!(rows[0].session, "a");
    }

    #[test]
    fn spend_rows_filter_by_day_and_project_and_name_live_sessions() {
        let store = store_with_worktree();
        store
            .insert_project(&Project {
                id: ProjectId("q".into()),
                name: "other".into(),
                repo_path: "/tmp/other".into(),
                sort_order: 1,
            })
            .unwrap();
        store
            .insert_agent(&role_agent("a", nebula_core::AgentRole::Worker))
            .unwrap();
        store
            .rename_agent(&AgentId("a".into()), "fix login")
            .unwrap();
        store
            .add_spend(&delta("a", "2026-10-20", 1, 0.0, 1))
            .unwrap();
        store
            .add_spend(&delta("a", "2026-10-21", 2, 0.0, 2))
            .unwrap();
        store
            .add_spend(&delta("a", "2026-10-23", 3, 0.0, 3))
            .unwrap();
        // A deleted session of the other project: only its copied name ties it.
        let mut gone = delta("gone", "2026-10-21", 4, 0.0, 4);
        gone.project = Some("other".into());
        store.add_spend(&gone).unwrap();

        let days = |rows: Vec<LedgerRow>| -> Vec<(String, String)> {
            rows.into_iter().map(|r| (r.spend.day, r.session)).collect()
        };
        assert_eq!(
            days(
                store
                    .spend_rows(Some("2026-10-21"), Some("2026-10-22"), None)
                    .unwrap()
            ),
            [
                ("2026-10-21".to_string(), "fix login".to_string()),
                ("2026-10-21".to_string(), "gone".to_string()),
            ]
        );
        assert_eq!(
            days(store.spend_rows(None, Some("2026-10-20"), None).unwrap()),
            [("2026-10-20".to_string(), "fix login".to_string())]
        );
        assert_eq!(
            days(
                store
                    .spend_rows(None, None, Some(&ProjectId("p".into())))
                    .unwrap()
            )
            .len(),
            3
        );
        assert_eq!(
            days(
                store
                    .spend_rows(None, None, Some(&ProjectId("q".into())))
                    .unwrap()
            ),
            [("2026-10-21".to_string(), "gone".to_string())]
        );
        let live = &store.spend_rows(Some("2026-10-23"), None, None).unwrap()[0];
        assert_eq!(
            (live.project.as_str(), live.status),
            ("demo", AgentStatus::Fresh)
        );
    }

    #[test]
    fn set_orchestrator_demotes_every_other_holder_and_reports_the_changes() {
        use nebula_core::AgentRole;
        let store = store_with_worktree();
        store
            .insert_agent(&role_agent("old", AgentRole::Orchestrator))
            .unwrap();
        store
            .insert_agent(&role_agent("new", AgentRole::Worker))
            .unwrap();
        store
            .set_agent_archived(&AgentId("old".into()), true)
            .unwrap();

        let changed = store
            .set_orchestrator(Some(&AgentId("new".into())))
            .unwrap();
        assert_eq!(changed, vec![AgentId("old".into()), AgentId("new".into())]);
        let old = store.get_agent(&AgentId("old".into())).unwrap().unwrap();
        assert_eq!(
            old.role,
            AgentRole::Worker,
            "archived holders are demoted too"
        );
        assert_eq!(store.orchestrator().unwrap().unwrap().id.0, "new");

        assert!(
            store
                .set_orchestrator(Some(&AgentId("new".into())))
                .unwrap()
                .is_empty(),
            "pinning the pinned row changes nothing"
        );
        assert_eq!(
            store.set_orchestrator(None).unwrap(),
            vec![AgentId("new".into())]
        );
        assert!(store.orchestrator().unwrap().is_none());

        store
            .set_orchestrator(Some(&AgentId("old".into())))
            .unwrap();
        assert!(store
            .set_orchestrator(Some(&AgentId("ghost".into())))
            .is_err());
        assert_eq!(
            store.orchestrator().unwrap().unwrap().id.0,
            "old",
            "a missing row demotes nobody"
        );
    }

    #[test]
    fn a_second_orchestrator_row_is_refused_by_the_schema() {
        use nebula_core::AgentRole;
        let store = store_with_worktree();
        store
            .insert_agent(&role_agent("one", AgentRole::Orchestrator))
            .unwrap();
        assert!(store
            .insert_agent(&role_agent("two", AgentRole::Orchestrator))
            .is_err());
        store
            .insert_agent(&role_agent("w1", AgentRole::Worker))
            .unwrap();
        store
            .insert_agent(&role_agent("w2", AgentRole::Worker))
            .unwrap();
    }

    #[test]
    fn migration_33_keeps_only_the_newest_live_orchestrator() {
        let conn = Connection::open_in_memory().unwrap();
        for (i, migration) in MIGRATIONS.iter().take(32).enumerate() {
            conn.execute_batch(&format!(
                "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                i + 1
            ))
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO projects (id, name, repo_path, sort_order, created_at)
               VALUES ('p', 'demo', '/tmp/demo', 0, 0);
             INSERT INTO worktrees (id, project_id, path, branch, is_main, sort_order, created_at)
               VALUES ('w', 'p', '/tmp/demo', 'main', 1, 0, 0);
             INSERT INTO agents (id, worktree_id, name, created_at, role, archived)
               VALUES ('a', 'w', 'a', 1, 'orchestrator', 0);
             INSERT INTO agents (id, worktree_id, name, created_at, role, archived)
               VALUES ('b', 'w', 'b', 2, 'orchestrator', 0);
             INSERT INTO agents (id, worktree_id, name, created_at, role, archived)
               VALUES ('c', 'w', 'c', 3, 'orchestrator', 1);",
        )
        .unwrap();
        let store = Store {
            conn: Mutex::new(conn),
        };
        store.migrate().unwrap();
        assert_eq!(store.orchestrator().unwrap().unwrap().id.0, "b");
    }

    /// Migration 35 from a 0.51 database: a session with both snapshots
    /// gets them as its base and tip, one with only a start gets neither,
    /// no turns, and a daemon id that stays put.
    #[test]
    fn migration_35_turns_051_snapshots_into_a_base_and_tip() {
        let conn = Connection::open_in_memory().unwrap();
        for (i, migration) in MIGRATIONS.iter().take(34).enumerate() {
            conn.execute_batch(&format!(
                "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                i + 1
            ))
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO projects (id, name, repo_path, sort_order, created_at)
               VALUES ('p', 'demo', '/tmp/demo', 0, 0);
             INSERT INTO worktrees (id, project_id, path, branch, is_main, sort_order, created_at)
               VALUES ('w', 'p', '/tmp/demo', 'main', 1, 0, 0);
             INSERT INTO agents (id, worktree_id, name, created_at) VALUES ('a', 'w', 'a', 1);
             INSERT INTO agents (id, worktree_id, name, created_at) VALUES ('b', 'w', 'b', 2);
             INSERT INTO session_snapshots
               (agent_id, worktree_id, start_head, start_tree, start_at, last_head, last_tree, last_at)
               VALUES ('a', 'w', 'h', 't1', 1, 'h', 't2', 2);
             INSERT INTO session_snapshots (agent_id, worktree_id, start_tree, start_at)
               VALUES ('b', 'w', 't1', 1);",
        )
        .unwrap();
        let store = Store {
            conn: Mutex::new(conn),
        };
        store.migrate().unwrap();
        let a = store
            .session_snapshots(&AgentId("a".into()))
            .unwrap()
            .unwrap();
        assert_eq!(
            (a.base.as_deref(), a.tip.as_deref()),
            (Some("t1"), Some("t2"))
        );
        let b = store
            .session_snapshots(&AgentId("b".into()))
            .unwrap()
            .unwrap();
        assert_eq!((b.base, b.tip), (None, None));
        assert!(store
            .session_turns(&AgentId("a".into()))
            .unwrap()
            .is_empty());
        let id = store.daemon_id().unwrap();
        assert_eq!(id.len(), 16);
        assert!(id.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(store.daemon_id().unwrap(), id);
    }

    /// Turns open, end, prune to the latest few, and go with the session's
    /// row and with a new start (the session moved).
    #[test]
    fn session_turns_open_end_prune_and_cascade() {
        let store = store_with_worktree();
        let w = WorktreeId("w".into());
        let a = AgentId("a".into());
        store
            .insert_agent(&role_agent("a", nebula_core::AgentRole::Worker))
            .unwrap();
        let point = |tree: &str, at| SnapshotPoint {
            head: None,
            tree: Some(tree.into()),
            at,
        };
        store.set_snapshot_start(&a, &w, &point("s", 0)).unwrap();
        for n in 1..=4u32 {
            assert_eq!(store.begin_turn(&a, &point("x", n as i64)).unwrap(), n);
            assert_eq!(store.open_turn(&a).unwrap().unwrap().n, n);
            store.end_turn(&a, n, &point("y", n as i64)).unwrap();
            assert_eq!(store.open_turn(&a).unwrap(), None);
        }
        assert_eq!(store.prune_turns(&a, 2).unwrap(), vec![1, 2]);
        assert_eq!(store.prune_turns(&a, 2).unwrap(), Vec::<u32>::new());
        let left: Vec<u32> = store
            .session_turns(&a)
            .unwrap()
            .iter()
            .map(|t| t.n)
            .collect();
        assert_eq!(left, vec![3, 4]);
        assert_eq!(store.begin_turn(&a, &point("x", 5)).unwrap(), 5);
        store.set_snapshot_view(&a, "b", Some("t")).unwrap();
        store.set_snapshot_start(&a, &w, &point("s2", 9)).unwrap();
        let moved = store.session_snapshots(&a).unwrap().unwrap();
        assert_eq!((moved.base, moved.tip), (None, None));
        assert!(store.session_turns(&a).unwrap().is_empty());
        store.begin_turn(&a, &point("x", 10)).unwrap();
        store.delete_agent(&a).unwrap();
        assert!(store.session_turns(&a).unwrap().is_empty());
    }

    #[test]
    fn roundtrip_tree() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "demo".into(),
            repo_path: "/tmp/demo".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        let worktree = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: "/tmp/demo".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        store.insert_worktree(&worktree).unwrap();
        let agent = Agent {
            parent: None,
            profile: None,
            id: AgentId::generate(),
            worktree_id: worktree.id.clone(),
            name: "agent-1".into(),
            status: AgentStatus::Running,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: Some("opus".into()),
            effort: Some("high".into()),
            session_id: Some("sess-123".into()),
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };
        let pr_url = "https://github.com/AgentSystemLabs/nebula/pull/42";
        store
            .insert_agent_with_launch_context(&agent, false, Some(pr_url), None)
            .unwrap();
        let codex_agent = Agent {
            parent: None,
            profile: None,
            id: AgentId::generate(),
            worktree_id: worktree.id.clone(),
            name: "agent-2".into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Codex,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 1,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };
        store.insert_agent(&codex_agent).unwrap();
        let cursor_agent = Agent {
            parent: None,
            profile: None,
            id: AgentId::generate(),
            worktree_id: worktree.id.clone(),
            name: "agent-3".into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Cursor,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 2,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };
        store.insert_agent(&cursor_agent).unwrap();
        let issue_url = "https://github.com/AgentSystemLabs/nebula/issues/15";
        let issue_agent = Agent {
            parent: None,
            profile: None,
            id: AgentId::generate(),
            worktree_id: worktree.id.clone(),
            name: "agent-4".into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 3,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };
        store
            .insert_agent_with_launch_context(&issue_agent, true, None, Some(issue_url))
            .unwrap();

        let (projects, worktrees, agents, _terms) = store.load_tree().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(agents.len(), 4);
        assert_eq!(agents[0].status, AgentStatus::Running);
        assert_eq!(agents[0].kind, AgentKind::Claude);
        assert_eq!(agents[0].session_id.as_deref(), Some("sess-123"));
        assert_eq!(agents[0].model.as_deref(), Some("opus"));
        assert_eq!(agents[0].effort.as_deref(), Some("high"));
        assert_eq!(agents[0].custom_harness, None, "built-ins store no id");
        assert_eq!(
            store.agent_pr_url(&agents[0].id).unwrap().as_deref(),
            Some(pr_url)
        );
        assert_eq!(store.agent_pr_url(&agents[1].id).unwrap(), None);

        // A Custom row round-trips its registry id beside the kind, so
        // respawns find the same entry (migration 27).
        let custom = Agent {
            parent: None,
            profile: None,
            id: AgentId::generate(),
            worktree_id: worktree.id.clone(),
            name: "agy-1".into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Custom,
            custom_harness: Some("agy".into()),
            model: Some("big-1".into()),
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };
        store.insert_agent(&custom).unwrap();
        let (_, _, reloaded, _) = store.load_tree().unwrap();
        let back = reloaded.iter().find(|a| a.id == custom.id).unwrap();
        assert_eq!(back.kind, AgentKind::Custom);
        assert_eq!(back.custom_harness.as_deref(), Some("agy"));
        assert_eq!(back.model.as_deref(), Some("big-1"));
        assert_eq!(agents[1].kind, AgentKind::Codex);
        assert_eq!(agents[1].model, None);
        assert_eq!(agents[2].kind, AgentKind::Cursor);
        // The issue context is its own column: a PR SESSION carries none,
        // an ISSUE SESSION carries no PR.
        assert_eq!(store.agent_issue_url(&agents[0].id).unwrap(), None);
        assert_eq!(
            store.agent_issue_url(&agents[3].id).unwrap().as_deref(),
            Some(issue_url)
        );
        assert_eq!(store.agent_pr_url(&agents[3].id).unwrap(), None);
        // …and the loaded row carries the issue to the TUI (`⇧I` opens it).
        assert_eq!(agents[0].issue_url, None);
        assert_eq!(agents[3].issue_url.as_deref(), Some(issue_url));
    }

    /// A `kete spawn` child keeps the session that started it across a
    /// reload; a row a person started has none.
    #[test]
    fn an_agent_row_keeps_its_parent() {
        let store = Store::open_in_memory().unwrap();
        store
            .insert_project(&Project {
                id: ProjectId("p".into()),
                name: "p".into(),
                repo_path: "/tmp/p".into(),
                sort_order: 0,
            })
            .unwrap();
        store
            .insert_worktree(&Worktree {
                id: WorktreeId("w".into()),
                project_id: ProjectId("p".into()),
                path: "/tmp/p".into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            })
            .unwrap();
        let row = |id: &str, parent: Option<&str>| Agent {
            parent: parent.map(|p| AgentId(p.into())),
            profile: None,
            id: AgentId(id.into()),
            worktree_id: WorktreeId("w".into()),
            name: id.into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };
        store.insert_agent(&row("caller", None)).unwrap();
        store
            .insert_agent_with_auto_title(&row("child", Some("caller")), true)
            .unwrap();
        let child = store.get_agent(&AgentId("child".into())).unwrap().unwrap();
        assert_eq!(child.parent, Some(AgentId("caller".into())));
        let (_, _, agents, _) = store.load_tree().unwrap();
        let caller = agents.iter().find(|a| a.id.0 == "caller").unwrap();
        assert_eq!(caller.parent, None);
        // No foreign key: deleting the parent leaves the child's link,
        // which a client draws as a root once the parent row is gone.
        store.delete_agent(&AgentId("caller".into())).unwrap();
        assert!(store.get_agent(&AgentId("child".into())).unwrap().is_some());
    }

    /// Read marks are keyed by PR URL and outlive the worktree they were
    /// noticed on, so they live in their own table with no foreign key: no
    /// row here is ever cascaded away by a checkout being deleted.
    #[test]
    fn pr_seen_marks_roundtrip_and_overwrite() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.load_pr_seen().unwrap().is_empty());

        let url = "https://github.com/o/r/pull/7";
        store.mark_pr_seen(url, "2024-04-25T19:55:42Z").unwrap();
        let seen = store.load_pr_seen().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].url, url);
        assert_eq!(seen[0].marker, "2024-04-25T19:55:42Z");

        // Opening it again moves the mark rather than adding a second row.
        store.mark_pr_seen(url, "2024-04-27T09:00:00Z").unwrap();
        let seen = store.load_pr_seen().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].marker, "2024-04-27T09:00:00Z");

        // An empty marker is a real answer: opened, nobody had posted yet.
        store.mark_pr_seen(url, "").unwrap();
        assert_eq!(store.load_pr_seen().unwrap()[0].marker, "");
    }

    #[test]
    fn link_crud_roundtrip_and_cascade() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "demo".into(),
            repo_path: "/tmp/demo".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        let worktree = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: "/tmp/demo".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        store.insert_worktree(&worktree).unwrap();

        assert_eq!(store.next_link_sort_order(&worktree.id).unwrap(), 0);
        let link = Link {
            id: LinkId::generate(),
            worktree_id: worktree.id.clone(),
            url: "https://github.com/o/r/pull/7".into(),
            sort_order: store.next_link_sort_order(&worktree.id).unwrap(),
        };
        store.insert_link(&link).unwrap();
        assert_eq!(store.next_link_sort_order(&worktree.id).unwrap(), 1);

        store
            .set_link_url(&link.id, "https://example.dev/spec")
            .unwrap();
        let read = store.get_link(&link.id).unwrap().unwrap();
        assert_eq!(read.url, "https://example.dev/spec");
        assert_eq!(read.worktree_id, worktree.id);
        assert_eq!(store.load_links().unwrap().len(), 1);

        store.delete_link(&link.id).unwrap();
        assert!(store.get_link(&link.id).unwrap().is_none());

        // Links hang off the worktree: deleting the project cascades
        // through it.
        store.insert_link(&link).unwrap();
        store.delete_project(&project.id).unwrap();
        assert!(store.load_links().unwrap().is_empty());
    }

    /// Real upgrade path: a v9 database still carrying `todos` rows walks
    /// the whole chain — 10's rebuild, 15's rename, 21's DROP — and lands
    /// with the table retired rather than erroring partway.
    #[test]
    fn migration_21_retires_notes_from_a_v9_database() {
        let path =
            std::env::temp_dir().join(format!("nebula-mig21-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            for (i, migration) in MIGRATIONS.iter().take(9).enumerate() {
                conn.execute_batch(&format!(
                    "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                    i + 1
                ))
                .unwrap();
            }
            conn.execute_batch(
                "INSERT INTO projects (id, name, repo_path, sort_order, created_at) VALUES ('p1', 'p', '/tmp/p', 0, 0);
                 INSERT INTO worktrees (id, project_id, path, branch, is_main, sort_order, created_at) VALUES ('w1', 'p1', '/tmp/p', 'main', 1, 0, 0);
                 INSERT INTO todos (id, worktree_id, text, done, sort_order, created_at) VALUES ('t1', 'w1', 'old note', 1, 3, 0);",
            )
            .unwrap();
        }

        let store = Store::open(&path).unwrap();
        // The project survived the walk; neither the original table name nor
        // the renamed one is left behind.
        assert_eq!(store.load_tree().unwrap().0.len(), 1);
        let conn = store.conn.lock();
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('notes', 'todos')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
        drop(conn);
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    /// A database already at v21 gains nullable PR launch context without
    /// rewriting or invalidating its existing AGENT rows.
    #[test]
    fn migration_22_adds_pr_context_without_backfill() {
        let path =
            std::env::temp_dir().join(format!("nebula-mig22-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            for (i, migration) in MIGRATIONS.iter().take(21).enumerate() {
                conn.execute_batch(&format!(
                    "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                    i + 1
                ))
                .unwrap();
            }
            conn.execute_batch(
                "INSERT INTO projects (id, name, repo_path, sort_order, created_at, workspace_id)
                   VALUES ('p1', 'p', '/tmp/p', 0, 0, 'default');
                 INSERT INTO worktrees (id, project_id, path, branch, is_main, sort_order, created_at, pinned)
                   VALUES ('w1', 'p1', '/tmp/p', 'main', 1, 0, 0, 0);
                 INSERT INTO agents (id, worktree_id, name, created_at)
                   VALUES ('a1', 'w1', 'existing', 0);",
            )
            .unwrap();
        }

        let store = Store::open(&path).unwrap();
        assert_eq!(store.agent_pr_url(&AgentId("a1".into())).unwrap(), None);
        // …and the later columns arrive NULL too (23: claude_title,
        // 24: recent_prompts — read as the empty history).
        assert_eq!(
            store.agent_claude_title(&AgentId("a1".into())).unwrap(),
            None
        );
        assert!(store
            .get_agent(&AgentId("a1".into()))
            .unwrap()
            .unwrap()
            .recent_prompts
            .is_empty());
        let version: i64 = store
            .conn
            .lock()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    /// Real upgrade path: a v12 database (pre-workspaces) walks the whole
    /// chain — 13 grouping its projects into 'default', 28 taking the
    /// groups away again — and its project loads untouched, with no
    /// workspace table left behind.
    #[test]
    fn migration_13_to_28_carries_a_pre_workspace_project_through() {
        let path =
            std::env::temp_dir().join(format!("nebula-mig13-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            for (i, migration) in MIGRATIONS.iter().take(12).enumerate() {
                conn.execute_batch(&format!(
                    "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                    i + 1
                ))
                .unwrap();
            }
            conn.execute_batch(
                "INSERT INTO projects (id, name, repo_path, sort_order, created_at) VALUES ('p1', 'p', '/tmp/p', 0, 0);",
            )
            .unwrap();
        }

        let store = Store::open(&path).unwrap();
        let (projects, _, _, _) = store.load_tree().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "p");
        assert_eq!(table_names(&store), table_names_without_workspaces());
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    /// Real upgrade path: a v17 database still carries the project divider
    /// columns (with data in them). Migration 18 drops them and the
    /// projects underneath load untouched.
    #[test]
    fn migration_18_drops_the_divider_columns() {
        let path =
            std::env::temp_dir().join(format!("nebula-mig18-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            for (i, migration) in MIGRATIONS.iter().take(17).enumerate() {
                conn.execute_batch(&format!(
                    "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                    i + 1
                ))
                .unwrap();
            }
            conn.execute_batch(
                "INSERT INTO projects (id, name, repo_path, sort_order, created_at, divider_after, divider_label, divider_before, divider_before_label, workspace_id)
                   VALUES ('p1', 'one', '/tmp/one', 0, 0, 1, 'work', 1, 'top', 'default');
                 INSERT INTO projects (id, name, repo_path, sort_order, created_at, workspace_id)
                   VALUES ('p2', 'two', '/tmp/two', 1, 0, 'default');",
            )
            .unwrap();
        }

        let store = Store::open(&path).unwrap();
        let (projects, _, _, _) = store.load_tree().unwrap();
        assert_eq!(
            projects.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            ["one", "two"]
        );
        assert_eq!(projects[0].sort_order, 0);
        let columns: Vec<String> = store
            .conn
            .lock()
            .prepare("PRAGMA table_info(projects)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            !columns.iter().any(|c| c.starts_with("divider")),
            "divider columns survived the migration: {columns:?}"
        );
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    /// Real upgrade path: a v13 database goes through both projects-table
    /// rebuilds (14 scoping repo uniqueness to a workspace, 28 making it
    /// global again). Each drops the old table — child rows must survive.
    #[test]
    fn migration_14_and_28_rebuilds_keep_the_children() {
        let path =
            std::env::temp_dir().join(format!("nebula-mig14-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            for (i, migration) in MIGRATIONS.iter().take(13).enumerate() {
                conn.execute_batch(&format!(
                    "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                    i + 1
                ))
                .unwrap();
            }
            conn.execute_batch(
                "INSERT INTO projects (id, name, repo_path, sort_order, created_at, workspace_id) VALUES ('p1', 'p', '/tmp/p', 0, 0, 'default');
                 INSERT INTO worktrees (id, project_id, path, branch, is_main, sort_order, created_at) VALUES ('w1', 'p1', '/tmp/p', 'main', 1, 0, 0);
                 INSERT INTO agents (id, worktree_id, name, created_at) VALUES ('a1', 'w1', 'agent', 0);",
            )
            .unwrap();
        }

        let store = Store::open(&path).unwrap();
        let (projects, worktrees, agents, _) = store.load_tree().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(worktrees.len(), 1, "worktrees must survive the rebuild");
        assert_eq!(agents.len(), 1, "agents must survive the rebuild");

        // One repo is one project again: a second row for it is refused.
        let dup = Project {
            id: ProjectId("p2".into()),
            name: "p".into(),
            repo_path: PathBuf::from("/tmp/p"),
            sort_order: 1,
        };
        assert!(store.insert_project(&dup).is_err());

        // Path lookups need no scope.
        assert_eq!(
            store.project_by_path(Path::new("/tmp/p")).unwrap(),
            Some(ProjectId("p1".into()))
        );
        assert_eq!(store.project_by_path(Path::new("/tmp/q")).unwrap(), None);
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    /// Every table in the schema, sorted — what a migration test compares
    /// to prove nothing was left behind.
    fn table_names(store: &Store) -> Vec<String> {
        store
            .conn
            .lock()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// A PORT BLOCK slot is the lowest free one, held for good, and a
    /// released or orphaned one is handed out again.
    #[test]
    fn port_slots_are_lowest_free_and_stable() {
        let store = Store::open_in_memory().unwrap();
        store
            .conn
            .lock()
            .execute_batch(
                "INSERT INTO projects (id, name, repo_path, sort_order, created_at) VALUES ('p', 'p', '/p', 0, 0);
                 INSERT INTO worktrees (id, project_id, path, branch, is_main, sort_order, created_at)
                   VALUES ('a', 'p', '/p', 'main', 1, 0, 0), ('b', 'p', '/b', 'b', 0, 1, 0), ('c', 'p', '/c', 'c', 0, 2, 0);",
            )
            .unwrap();
        let (a, b, c) = (
            WorktreeId("a".into()),
            WorktreeId("b".into()),
            WorktreeId("c".into()),
        );
        assert_eq!(store.allocate_port_slot(&a).unwrap(), 0);
        assert_eq!(store.allocate_port_slot(&b).unwrap(), 1);
        assert_eq!(store.allocate_port_slot(&a).unwrap(), 0, "held for good");
        store.release_port_slot(&a).unwrap();
        assert_eq!(store.worktree_port_slot(&a).unwrap(), None);
        assert_eq!(
            store.allocate_port_slot(&c).unwrap(),
            0,
            "a freed slot is reused"
        );
        assert_eq!(
            store.worktree_port_slot(&b).unwrap(),
            Some(1),
            "nobody else moved"
        );
        // A slot whose worktree went some other way is freed on the next ask.
        store
            .conn
            .lock()
            .execute_batch("DELETE FROM worktrees WHERE id = 'b';")
            .unwrap();
        assert_eq!(store.allocate_port_slot(&a).unwrap(), 1);
        assert_eq!(store.port_slots().unwrap().len(), 2);
    }

    fn table_names_without_workspaces() -> Vec<String> {
        [
            "agents",
            "daemon_identity",
            "links",
            "pr_seen",
            "projects",
            "session_snapshots",
            "session_spend",
            "session_turns",
            "terminals",
            "ui_state",
            "worktree_bases",
            "worktree_ports",
            "worktrees",
        ]
        .map(String::from)
        .to_vec()
    }

    /// Real upgrade path, shaped like a real install: a v27 database with
    /// two workspaces, one repo registered in both (its root checkout a row
    /// under each), a checkout only the newer row knew, and the default
    /// workspace deleted. Migration 28 folds the two rows into the older
    /// one — every session, terminal and link under the newer row's root
    /// moves onto the survivor's root, the extra checkout moves across
    /// whole — renumbers the one list oldest workspace first, and leaves
    /// no workspace behind.
    #[test]
    fn migration_28_folds_a_repo_registered_in_two_workspaces_into_one_project() {
        let path =
            std::env::temp_dir().join(format!("nebula-mig28-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            for (i, migration) in MIGRATIONS.iter().take(27).enumerate() {
                conn.execute_batch(&format!(
                    "BEGIN; {migration}; PRAGMA user_version = {}; COMMIT;",
                    i + 1
                ))
                .unwrap();
            }
            conn.execute_batch(
                "INSERT INTO workspaces (id, name, active, created_at) VALUES ('ws-work', 'work', 1, 10);
                 INSERT INTO workspaces (id, name, active, created_at) VALUES ('ws-video', 'video', 0, 20);
                 INSERT INTO projects (id, name, repo_path, sort_order, created_at, workspace_id) VALUES
                   ('site-a', 'site', '/r/site', 1, 100, 'ws-work'),
                   ('api', 'api', '/r/api', 0, 50, 'ws-work'),
                   ('site-b', 'site', '/r/site', 0, 200, 'ws-video'),
                   ('film', 'film', '/r/film', 1, 60, 'ws-video');
                 DELETE FROM workspaces WHERE id = 'default';
                 INSERT INTO worktrees (id, project_id, path, branch, is_main, sort_order, created_at) VALUES
                   ('a-root', 'site-a', '/r/site', 'main', 1, 0, 100),
                   ('b-root', 'site-b', '/r/site', 'main', 1, 0, 200),
                   ('b-feat', 'site-b', '/r/site-wt/feat', 'feat', 0, 1, 210),
                   ('api-root', 'api', '/r/api', 'main', 1, 0, 50),
                   ('film-root', 'film', '/r/film', 'main', 1, 0, 60);
                 INSERT INTO agents (id, worktree_id, name, created_at) VALUES
                   ('ag-a', 'a-root', 'on a', 0),
                   ('ag-b', 'b-root', 'on b', 0),
                   ('ag-feat', 'b-feat', 'on feat', 0);
                 INSERT INTO terminals (id, worktree_id, name, created_at) VALUES ('t-b', 'b-root', 'shell', 0);
                 INSERT INTO links (id, worktree_id, url, created_at) VALUES ('l-b', 'b-root', 'https://x.test/1', 0);",
            )
            .unwrap();
        }

        let store = Store::open(&path).unwrap();
        let (projects, worktrees, agents, terminals) = store.load_tree().unwrap();
        // One `site`, the older row; the list reads workspace by workspace
        // (work's api then site, then video's film), renumbered from 0.
        assert_eq!(
            projects
                .iter()
                .map(|p| (p.id.as_str(), p.sort_order))
                .collect::<Vec<_>>(),
            [("api", 0), ("site-a", 1), ("film", 2)]
        );
        // The newer row's root is gone; its extra checkout moved across and
        // is not a second root.
        let site: Vec<_> = worktrees
            .iter()
            .filter(|w| w.project_id.as_str() == "site-a")
            .map(|w| (w.id.as_str(), w.is_main))
            .collect();
        assert_eq!(site, [("a-root", true), ("b-feat", false)]);
        assert!(!worktrees.iter().any(|w| w.id.as_str() == "b-root"));
        // Nothing that lived under it was lost.
        let home = |id: &str| {
            agents
                .iter()
                .find(|a| a.id.as_str() == id)
                .map(|a| a.worktree_id.as_str().to_string())
        };
        assert_eq!(home("ag-a").as_deref(), Some("a-root"));
        assert_eq!(home("ag-b").as_deref(), Some("a-root"));
        assert_eq!(home("ag-feat").as_deref(), Some("b-feat"));
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals[0].worktree_id.as_str(), "a-root");
        let links = store.load_links().unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].worktree_id.as_str(), "a-root");
        // No workspace table, no scratch tables, and one repo = one row.
        assert_eq!(table_names(&store), table_names_without_workspaces());
        assert_eq!(
            store.project_by_path(Path::new("/r/site")).unwrap(),
            Some(ProjectId("site-a".into()))
        );
        let version: i64 = store
            .conn
            .lock()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        drop(store);
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    #[test]
    fn auto_title_pending_lifecycle() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "p".into(),
            repo_path: "/tmp/p".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        let wt = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: "/tmp/p".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        store.insert_worktree(&wt).unwrap();
        let agent = |id: &str| Agent {
            parent: None,
            profile: None,
            id: AgentId(id.into()),
            worktree_id: wt.id.clone(),
            name: "agent-1".into(),
            status: AgentStatus::Fresh,
            archived: false,
            archived_at: 0,
            unseen: false,
            kind: AgentKind::Claude,
            custom_harness: None,
            model: None,
            effort: None,
            session_id: None,
            cloud_session_id: None,
            sort_order: 0,
            status_changed_at: 0,
            alive: false,
            issue_url: None,
            role: nebula_core::AgentRole::Worker,
            recent_prompts: Vec::new(),
        };

        // Default-named session: pending until the agent titles it, and the
        // conditional rename fires exactly once.
        store
            .insert_agent_with_auto_title(&agent("a1"), true)
            .unwrap();
        let id = AgentId("a1".into());
        assert!(store.agent_auto_title_pending(&id).unwrap());
        assert!(store
            .rename_agent_if_auto_pending(&id, "Fix Login Redirect")
            .unwrap());
        assert!(!store.agent_auto_title_pending(&id).unwrap());
        assert!(!store
            .rename_agent_if_auto_pending(&id, "Second Attempt")
            .unwrap());
        assert_eq!(
            store.get_agent(&id).unwrap().unwrap().name,
            "Fix Login Redirect"
        );

        // A user rename retires the pending flag so a late agent attempt
        // can't clobber the user's choice.
        store
            .insert_agent_with_auto_title(&agent("a2"), true)
            .unwrap();
        let id = AgentId("a2".into());
        store.rename_agent(&id, "my session").unwrap();
        assert!(!store.agent_auto_title_pending(&id).unwrap());
        assert!(!store.rename_agent_if_auto_pending(&id, "Nope").unwrap());
        assert_eq!(store.get_agent(&id).unwrap().unwrap().name, "my session");

        // Custom-named sessions (plain insert) never pend; unknown ids
        // report not-pending instead of erroring.
        store.insert_agent(&agent("a3")).unwrap();
        assert!(!store
            .agent_auto_title_pending(&AgentId("a3".into()))
            .unwrap());
        assert!(!store
            .agent_auto_title_pending(&AgentId("ghost".into()))
            .unwrap());
    }

    /// CLAUDE TITLE SYNC bookkeeping: Claude's title is adopted only when
    /// it changed on Claude's side, a nebula rename made since survives a
    /// re-read of Claude's older title, and the reply pushes the row's
    /// name only until Claude holds it.
    #[test]
    fn claude_title_follows_claude_without_undoing_a_nebula_rename() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "p".into(),
            repo_path: "/tmp/p".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        let wt = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: "/tmp/p".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        store.insert_worktree(&wt).unwrap();
        let id = AgentId("a1".into());
        store
            .insert_agent_with_auto_title(
                &Agent {
                    parent: None,
                    profile: None,
                    id: id.clone(),
                    worktree_id: wt.id.clone(),
                    name: "agent-1".into(),
                    status: AgentStatus::Fresh,
                    archived: false,
                    archived_at: 0,
                    unseen: false,
                    kind: AgentKind::Claude,
                    custom_harness: None,
                    model: None,
                    effort: None,
                    session_id: None,
                    cloud_session_id: None,
                    sort_order: 0,
                    status_changed_at: 0,
                    alive: false,
                    issue_url: None,
                    role: nebula_core::AgentRole::Worker,
                    recent_prompts: Vec::new(),
                },
                true,
            )
            .unwrap();
        let state = |store: &Store| store.agent_title_state(&id).unwrap().unwrap();

        // Fresh: nothing seen from Claude, nothing to push while pending.
        assert_eq!(store.agent_claude_title(&id).unwrap(), None);
        assert!(state(&store).auto_title_pending);
        assert_eq!(state(&store).to_push(), None);

        // `/rename` in Claude: adopted as a user rename, once.
        assert!(store.adopt_claude_title(&id, "From Claude").unwrap());
        assert!(!store.adopt_claude_title(&id, "From Claude").unwrap());
        let agent = store.get_agent(&id).unwrap().unwrap();
        assert_eq!(agent.name, "From Claude");
        assert!(!store.agent_auto_title_pending(&id).unwrap());
        assert_eq!(
            store.agent_claude_title(&id).unwrap().as_deref(),
            Some("From Claude")
        );
        assert_eq!(state(&store).to_push(), None, "the two agree");

        // `r` in nebula: the row changes, Claude's title is unchanged, so
        // re-reading it must not revert the row — and the name is due a push.
        store.rename_agent(&id, "From Kete").unwrap();
        assert!(!store.adopt_claude_title(&id, "From Claude").unwrap());
        assert_eq!(store.get_agent(&id).unwrap().unwrap().name, "From Kete");
        assert_eq!(state(&store).to_push(), Some("From Kete"));

        // Claude took the push (or the user typed the same name there).
        assert!(store.adopt_claude_title(&id, "From Kete").unwrap());
        assert_eq!(state(&store).to_push(), None);

        // Unknown ids read as nothing rather than erroring.
        assert_eq!(
            store.agent_claude_title(&AgentId("ghost".into())).unwrap(),
            None
        );
        assert!(store
            .agent_title_state(&AgentId("ghost".into()))
            .unwrap()
            .is_none());
        assert!(!store
            .adopt_claude_title(&AgentId("ghost".into()), "x")
            .unwrap());
    }

    #[test]
    fn cascade_delete_project_removes_children() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "demo".into(),
            repo_path: "/tmp/demo".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        let worktree = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: "/tmp/demo".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        store.insert_worktree(&worktree).unwrap();
        store
            .insert_terminal(&TerminalTab {
                id: TerminalId::generate(),
                worktree_id: worktree.id.clone(),
                name: "shell".into(),
                sort_order: 0,
                alive: false,
                run_command: None,
            })
            .unwrap();

        store.delete_project(&project.id).unwrap();
        let (projects, worktrees, _agents, terminals) = store.load_tree().unwrap();
        assert!(projects.is_empty());
        assert!(worktrees.is_empty());
        assert!(terminals.is_empty());
    }

    #[test]
    fn sweep_disconnected_only_hits_live_statuses() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "p".into(),
            repo_path: "/tmp/p".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        let wt = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: "/tmp/p".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        store.insert_worktree(&wt).unwrap();
        for (name, status) in [
            ("a", AgentStatus::Running),
            ("b", AgentStatus::Finished),
            ("c", AgentStatus::NeedsFeedback),
        ] {
            store
                .insert_agent(&Agent {
                    parent: None,
                    profile: None,
                    id: AgentId(format!("agent-{name}")),
                    worktree_id: wt.id.clone(),
                    name: name.into(),
                    status,
                    archived: false,
                    archived_at: 0,
                    unseen: false,
                    kind: AgentKind::Claude,
                    custom_harness: None,
                    model: None,
                    effort: None,
                    session_id: None,
                    cloud_session_id: None,
                    sort_order: 0,
                    status_changed_at: 0,
                    alive: false,
                    issue_url: None,
                    role: nebula_core::AgentRole::Worker,
                    recent_prompts: Vec::new(),
                })
                .unwrap();
        }
        let swept = store.sweep_disconnected(&Default::default()).unwrap();
        assert_eq!(swept.len(), 2);
        let (_, _, agents, _) = store.load_tree().unwrap();
        assert_eq!(
            agents
                .iter()
                .filter(|a| a.status == AgentStatus::Disconnected)
                .count(),
            2
        );
        assert_eq!(
            agents
                .iter()
                .filter(|a| a.status == AgentStatus::Finished)
                .count(),
            1
        );
    }

    /// An agent an IN-PLACE RESTART carried over is still running: the
    /// boot sweep leaves its status alone.
    #[test]
    fn sweep_disconnected_spares_carried_agents() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "p".into(),
            repo_path: "/tmp/p".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        let wt = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: "/tmp/p".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        store.insert_worktree(&wt).unwrap();
        for name in ["kept", "gone"] {
            store
                .insert_agent(&Agent {
                    parent: None,
                    profile: None,
                    id: AgentId(format!("agent-{name}")),
                    worktree_id: wt.id.clone(),
                    name: name.into(),
                    status: AgentStatus::Running,
                    archived: false,
                    archived_at: 0,
                    unseen: false,
                    kind: AgentKind::Claude,
                    custom_harness: None,
                    model: None,
                    effort: None,
                    session_id: None,
                    cloud_session_id: None,
                    sort_order: 0,
                    status_changed_at: 0,
                    alive: false,
                    issue_url: None,
                    role: nebula_core::AgentRole::Worker,
                    recent_prompts: Vec::new(),
                })
                .unwrap();
        }
        let carried = [AgentId("agent-kept".into())].into_iter().collect();
        let swept = store.sweep_disconnected(&carried).unwrap();
        assert_eq!(swept, vec![AgentId("agent-gone".into())]);
        let status = |id: &str| {
            store
                .get_agent(&AgentId(id.into()))
                .unwrap()
                .unwrap()
                .status
        };
        assert_eq!(status("agent-kept"), AgentStatus::Running);
        assert_eq!(status("agent-gone"), AgentStatus::Disconnected);
    }

    /// `Agent::unseen` rides along with the status: a live turn landing on
    /// finished raises it, staying there keeps it, leaving drops it. Fresh
    /// and archived rows never raise it, archiving takes it away, and a
    /// daemon restart leaves finished rows — flag included — alone.
    /// `mark_agent_seen` reports whether it had anything to clear.
    #[test]
    fn unseen_follows_the_status_and_clears_on_seen() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId::generate(),
            name: "demo".into(),
            repo_path: "/tmp/demo".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        let worktree = Worktree {
            id: WorktreeId::generate(),
            project_id: project.id.clone(),
            path: "/tmp/demo".into(),
            branch: "main".into(),
            is_main: true,
            sort_order: 0,
        };
        store.insert_worktree(&worktree).unwrap();
        let seed = |name: &str, status: AgentStatus| {
            let agent = Agent {
                parent: None,
                profile: None,
                id: AgentId::generate(),
                worktree_id: worktree.id.clone(),
                name: name.into(),
                status,
                archived: false,
                archived_at: 0,
                unseen: false,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                sort_order: 0,
                status_changed_at: 0,
                alive: false,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
                recent_prompts: Vec::new(),
            };
            store.insert_agent(&agent).unwrap();
            agent.id
        };
        let unseen = |id: &AgentId| store.get_agent(id).unwrap().unwrap().unseen;
        let flip =
            |id: &AgentId, status: AgentStatus| store.set_agent_status(id, status).unwrap().1;

        let a = seed("a", AgentStatus::Running);
        assert!(!unseen(&a));
        assert!(flip(&a, AgentStatus::Finished), "yellow → green raises it");
        assert!(unseen(&a));
        assert!(flip(&a, AgentStatus::Finished), "staying finished keeps it");
        assert!(!flip(&a, AgentStatus::Running), "a new turn drops it");
        assert!(!unseen(&a));
        assert!(!flip(&a, AgentStatus::NeedsFeedback));
        assert!(
            flip(&a, AgentStatus::Finished),
            "red → green is a finish too"
        );
        assert!(
            store.mark_agent_seen(&a).unwrap(),
            "there was something to clear"
        );
        assert!(!unseen(&a));
        assert!(
            !store.mark_agent_seen(&a).unwrap(),
            "already clear: nothing to broadcast"
        );

        // The tree load carries it, same as the single-row read.
        flip(&a, AgentStatus::Running);
        flip(&a, AgentStatus::Finished);
        let (_, _, agents, _) = store.load_tree().unwrap();
        assert!(agents.iter().find(|x| x.id == a).unwrap().unseen);

        // Archiving takes it away, and an archived row never raises it.
        store.set_agent_archived(&a, true).unwrap();
        assert!(!unseen(&a));
        flip(&a, AgentStatus::Running);
        assert!(
            !flip(&a, AgentStatus::Finished),
            "archived rows are out of sight"
        );

        // A Stop nebula never saw the prompt for is not a yellow → green.
        let b = seed("b", AgentStatus::Fresh);
        assert!(!flip(&b, AgentStatus::Finished));

        // A daemon restart disconnects live rows and leaves finished ones alone.
        let c = seed("c", AgentStatus::Running);
        assert!(flip(&c, AgentStatus::Finished));
        store.sweep_disconnected(&Default::default()).unwrap();
        assert!(unseen(&c), "still waiting to be read after the restart");
    }

    /// RECENT PROMPTS: appended in order, pruned to the newest
    /// `RECENT_PROMPTS_KEPT`, read back by both row paths, and nothing
    /// for an id with no row.
    #[test]
    fn push_prompt_keeps_the_newest_bounded_history() {
        let store = Store::open_in_memory().unwrap();
        let project = Project {
            id: ProjectId("p1".into()),
            name: "p".into(),
            repo_path: "/tmp/p".into(),
            sort_order: 0,
        };
        store.insert_project(&project).unwrap();
        store
            .insert_worktree(&Worktree {
                id: WorktreeId("w1".into()),
                project_id: project.id.clone(),
                path: "/tmp/p".into(),
                branch: "main".into(),
                is_main: true,
                sort_order: 0,
            })
            .unwrap();
        let id = AgentId("a1".into());
        store
            .insert_agent(&Agent {
                parent: None,
                profile: None,
                id: id.clone(),
                worktree_id: WorktreeId("w1".into()),
                name: "agent-1".into(),
                status: AgentStatus::Fresh,
                archived: false,
                archived_at: 0,
                unseen: false,
                kind: AgentKind::Claude,
                custom_harness: None,
                model: None,
                effort: None,
                session_id: None,
                cloud_session_id: None,
                sort_order: 0,
                status_changed_at: 0,
                alive: false,
                issue_url: None,
                role: nebula_core::AgentRole::Worker,
                recent_prompts: Vec::new(),
            })
            .unwrap();
        let entry = |n: usize| PromptEntry {
            text: format!("prompt {n}"),
            submitted_at: 1_000 + n as i64,
        };
        assert!(store
            .get_agent(&id)
            .unwrap()
            .unwrap()
            .recent_prompts
            .is_empty());

        assert!(store.push_prompt(&id, &entry(1)).unwrap());
        assert!(store.push_prompt(&id, &entry(2)).unwrap());
        let got = store.get_agent(&id).unwrap().unwrap().recent_prompts;
        assert_eq!(got, vec![entry(1), entry(2)], "oldest first");

        // Past the cap the oldest fall off the front.
        for n in 3..=(RECENT_PROMPTS_KEPT + 2) {
            assert!(store.push_prompt(&id, &entry(n)).unwrap());
        }
        let got = store.get_agent(&id).unwrap().unwrap().recent_prompts;
        assert_eq!(got.len(), RECENT_PROMPTS_KEPT);
        assert_eq!(got.first(), Some(&entry(3)));
        assert_eq!(got.last(), Some(&entry(RECENT_PROMPTS_KEPT + 2)));

        // `load_tree` reads the same column through the same mapper.
        let (_, _, agents, _) = store.load_tree().unwrap();
        assert_eq!(agents[0].recent_prompts, got);

        // A history an older nebula wrote with injected prompts in it
        // reads back without them, and the next push persists the
        // pruning rather than carrying them along.
        let injected = PromptEntry {
            text: r#"<cross-session-message from="uds:/tmp/cc-socks/1.sock"> hi"#.into(),
            submitted_at: 2_000,
        };
        let typed = PromptEntry {
            text: "what broke the cards".into(),
            submitted_at: 2_001,
        };
        {
            let conn = store.conn.lock();
            let json = serde_json::to_string(&vec![injected, typed.clone()]).unwrap();
            conn.execute(
                "UPDATE agents SET recent_prompts = ?2 WHERE id = ?1",
                params![id.as_str(), json],
            )
            .unwrap();
        }
        assert_eq!(
            store.get_agent(&id).unwrap().unwrap().recent_prompts,
            vec![typed.clone()]
        );
        assert!(store.push_prompt(&id, &entry(99)).unwrap());
        assert_eq!(
            store.get_agent(&id).unwrap().unwrap().recent_prompts,
            vec![typed, entry(99)]
        );

        // No row, nothing recorded — and no error.
        assert!(!store
            .push_prompt(&AgentId("ghost".into()), &entry(1))
            .unwrap());
    }

    fn linked(store: &Store, id: &str, project: &str) {
        store
            .insert_worktree(&Worktree {
                id: WorktreeId(id.into()),
                project_id: ProjectId(project.into()),
                path: format!("/tmp/{id}").into(),
                branch: id.into(),
                is_main: false,
                sort_order: 0,
            })
            .unwrap();
    }

    /// (id, sort_order) of the rows `load_tree` lists for `scope`, in its
    /// order — what a client shows.
    fn shown(store: &Store, scope: &ReorderScope) -> Vec<(String, i64)> {
        let (projects, worktrees, agents, _) = store.load_tree().unwrap();
        match scope {
            ReorderScope::Projects => projects
                .into_iter()
                .map(|p| (p.id.0, p.sort_order))
                .collect(),
            ReorderScope::Worktrees(p) => worktrees
                .into_iter()
                .filter(|w| &w.project_id == p)
                .map(|w| (w.id.0, w.sort_order))
                .collect(),
            ReorderScope::Agents(w) => agents
                .into_iter()
                .filter(|a| &a.worktree_id == w)
                .map(|a| (a.id.0, a.sort_order))
                .collect(),
        }
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn slots(list: &[&str]) -> Vec<(String, i64)> {
        list.iter()
            .enumerate()
            .map(|(i, s)| (s.to_string(), i as i64))
            .collect()
    }

    #[test]
    fn reorder_writes_the_whole_scope_0_to_n_and_reports_what_moved() {
        let store = store_with_worktree();
        for id in ["a", "b", "c"] {
            store
                .insert_agent(&role_agent(id, nebula_core::AgentRole::Worker))
                .unwrap();
        }
        let scope = ReorderScope::Agents(WorktreeId("w".into()));
        // All three sat at 0 (legacy rows): every one gets a slot.
        let changed = store.reorder(&scope, &ids(&["c", "a", "b"])).unwrap();
        assert_eq!(shown(&store, &scope), slots(&["c", "a", "b"]));
        assert_eq!(changed, ids(&["a", "b"]), "c already sat at 0");

        // Same order again: nothing to write, nothing to broadcast.
        assert!(store
            .reorder(&scope, &ids(&["c", "a", "b"]))
            .unwrap()
            .is_empty());

        // Projects too, and their slots survive a reopen of the tree.
        for (i, id) in ["q", "r"].iter().enumerate() {
            store
                .insert_project(&Project {
                    id: ProjectId((*id).into()),
                    name: (*id).into(),
                    repo_path: format!("/tmp/{id}").into(),
                    sort_order: i as i64 + 1,
                })
                .unwrap();
        }
        store
            .reorder(&ReorderScope::Projects, &ids(&["r", "p", "q"]))
            .unwrap();
        assert_eq!(
            shown(&store, &ReorderScope::Projects),
            slots(&["r", "p", "q"])
        );
    }

    #[test]
    fn reorder_appends_rows_left_out_and_skips_ids_outside_the_scope() {
        let store = store_with_worktree();
        linked(&store, "feat", "p");
        for id in ["a", "b", "c", "d"] {
            store
                .insert_agent(&role_agent(id, nebula_core::AgentRole::Worker))
                .unwrap();
        }
        // A row of the right kind under another checkout.
        store
            .insert_agent(&Agent {
                worktree_id: WorktreeId("feat".into()),
                ..role_agent("elsewhere", nebula_core::AgentRole::Worker)
            })
            .unwrap();
        let scope = ReorderScope::Agents(WorktreeId("w".into()));
        store.reorder(&scope, &ids(&["a", "b", "c", "d"])).unwrap();

        // A partial list: d to the front, the rest follow in their old order.
        store.reorder(&scope, &ids(&["d"])).unwrap();
        assert_eq!(shown(&store, &scope), slots(&["d", "a", "b", "c"]));

        // Unknown ids, another checkout's row and repeats are skipped.
        store
            .reorder(&scope, &ids(&["ghost", "c", "elsewhere", "c", "b", "feat"]))
            .unwrap();
        assert_eq!(shown(&store, &scope), slots(&["c", "b", "d", "a"]));
        assert_eq!(
            store
                .get_agent(&AgentId("elsewhere".into()))
                .unwrap()
                .unwrap()
                .worktree_id,
            WorktreeId("feat".into()),
            "the stray id's row is untouched"
        );

        // An empty list changes nothing.
        assert!(store.reorder(&scope, &[]).unwrap().is_empty());

        // A scope whose parent is gone is an error, not a silent no-op.
        let err = store
            .reorder(
                &ReorderScope::Agents(WorktreeId("gone".into())),
                &ids(&["a"]),
            )
            .unwrap_err();
        assert!(err.to_string().contains("worktree not found"), "{err}");
        let err = store
            .reorder(&ReorderScope::Worktrees(ProjectId("gone".into())), &[])
            .unwrap_err();
        assert!(err.to_string().contains("project not found"), "{err}");
    }

    #[test]
    fn reorder_keeps_the_main_checkout_first() {
        let store = store_with_worktree();
        linked(&store, "x", "p");
        linked(&store, "y", "p");
        let scope = ReorderScope::Worktrees(ProjectId("p".into()));

        // Left out (a client listing only the linked checkouts): main stays
        // first, the rest take the order given.
        store.reorder(&scope, &ids(&["y", "x"])).unwrap();
        assert_eq!(shown(&store, &scope), slots(&["w", "y", "x"]));

        // Named first: fine.
        store.reorder(&scope, &ids(&["w", "x", "y"])).unwrap();
        assert_eq!(shown(&store, &scope), slots(&["w", "x", "y"]));

        // Named anywhere else: refused, and nothing is written.
        let err = store.reorder(&scope, &ids(&["x", "w", "y"])).unwrap_err();
        assert!(err.to_string().contains("main checkout"), "{err}");
        assert_eq!(shown(&store, &scope), slots(&["w", "x", "y"]));
    }

    #[test]
    fn new_and_moved_rows_go_last_only_in_a_scope_the_user_ordered() {
        let store = store_with_worktree();
        linked(&store, "feat", "p");
        let worktree = |id: &str| Worktree {
            id: WorktreeId(id.into()),
            project_id: ProjectId("p".into()),
            path: format!("/tmp/{id}").into(),
            branch: id.into(),
            is_main: false,
            sort_order: 0,
        };
        // Never ordered (main and feat at 0): the new row joins them at 0,
        // and the client's own tiebreak keeps deciding.
        let next = store.append_worktree(worktree("next")).unwrap();
        assert_eq!(next.sort_order, 0);
        // Ordered: last, and stored as returned.
        let scope = ReorderScope::Worktrees(ProjectId("p".into()));
        store.reorder(&scope, &ids(&["next", "feat"])).unwrap();
        let last = store.append_worktree(worktree("last")).unwrap();
        assert_eq!(last.sort_order, 3);
        assert_eq!(shown(&store, &scope), slots(&["w", "next", "feat", "last"]));

        let w = WorktreeId("w".into());
        assert_eq!(store.next_agent_sort_order(&w).unwrap(), 0, "empty");
        for id in ["a", "b"] {
            store
                .insert_agent(&role_agent(id, nebula_core::AgentRole::Worker))
                .unwrap();
        }
        assert_eq!(store.next_agent_sort_order(&w).unwrap(), 0, "never ordered");
        store
            .reorder(&ReorderScope::Agents(w.clone()), &ids(&["b", "a"]))
            .unwrap();
        assert_eq!(store.next_agent_sort_order(&w).unwrap(), 2);

        // Moved onto a never-ordered checkout: 0 with the rest there.
        let a = AgentId("a".into());
        let b = AgentId("b".into());
        store
            .insert_agent(&Agent {
                worktree_id: WorktreeId("feat".into()),
                ..role_agent("f", nebula_core::AgentRole::Worker)
            })
            .unwrap();
        store
            .set_agent_worktree(&a, &WorktreeId("feat".into()))
            .unwrap();
        assert_eq!(store.get_agent(&a).unwrap().unwrap().sort_order, 0);
        // Moved onto an ordered one: last there. Onto its own: unchanged.
        store
            .reorder(
                &ReorderScope::Agents(WorktreeId("feat".into())),
                &ids(&["a", "f"]),
            )
            .unwrap();
        store
            .set_agent_worktree(&b, &WorktreeId("feat".into()))
            .unwrap();
        assert_eq!(store.get_agent(&b).unwrap().unwrap().sort_order, 2);
        store
            .set_agent_worktree(&b, &WorktreeId("feat".into()))
            .unwrap();
        assert_eq!(store.get_agent(&b).unwrap().unwrap().sort_order, 2);
    }

    /// Migration 34: a recorded base per worktree, snapshots per session,
    /// both gone with their row.
    #[test]
    fn recorded_bases_and_session_snapshots_round_trip_and_cascade() {
        let store = store_with_worktree();
        let w = WorktreeId("w".into());
        assert!(store.worktree_bases().unwrap().is_empty());
        store.set_worktree_base(&w, "origin/main", "abc").unwrap();
        store.set_worktree_base(&w, "origin/dev", "def").unwrap();
        assert_eq!(
            store.worktree_bases().unwrap(),
            vec![RecordedBase {
                worktree: w.clone(),
                refname: "origin/dev".into(),
                sha: "def".into(),
            }]
        );

        let a = AgentId("a".into());
        store
            .insert_agent(&role_agent("a", nebula_core::AgentRole::Worker))
            .unwrap();
        store
            .insert_agent(&role_agent("b", nebula_core::AgentRole::Worker))
            .unwrap();
        let point = |tree: Option<&str>, at| SnapshotPoint {
            head: Some("h".into()),
            tree: tree.map(str::to_string),
            at,
        };
        assert_eq!(store.session_snapshots(&a).unwrap(), None);
        assert!(!store.set_snapshot_last(&a, &point(Some("t2"), 2)).unwrap());
        store
            .set_snapshot_start(&a, &w, &point(Some("t1"), 1))
            .unwrap();
        store.set_snapshot_last(&a, &point(None, 2)).unwrap();
        let got = store.session_snapshots(&a).unwrap().unwrap();
        assert_eq!(got.start.tree.as_deref(), Some("t1"));
        assert_eq!(got.last, Some(point(None, 2)), "unavailable kept as such");
        // A new start (the session moved) drops the last one.
        store
            .set_snapshot_start(&a, &w, &point(Some("t3"), 3))
            .unwrap();
        assert_eq!(store.session_snapshots(&a).unwrap().unwrap().last, None);
        store
            .set_snapshot_start(&AgentId("b".into()), &w, &point(Some("t1"), 1))
            .unwrap();
        assert_eq!(store.snapshot_sessions_in(&w).unwrap(), 2);
        let mut under = store
            .snapshot_agents_under(Some(&ProjectId("p".into())), None)
            .unwrap();
        under.sort();
        assert_eq!(under, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            store.snapshot_agents_under(None, Some(&w)).unwrap().len(),
            2
        );

        store.delete_agent(&a).unwrap();
        assert_eq!(store.snapshot_agents().unwrap(), vec!["b".to_string()]);
        store.delete_worktree(&w).unwrap();
        assert!(store.snapshot_agents().unwrap().is_empty());
        assert!(store.worktree_bases().unwrap().is_empty());
    }
    /// A database a 0.48 daemon left (migration 33), captured with its
    /// rows (`crates/nebula/tests/fixtures/upgrade`): the store brings it
    /// to the last migration with every project, worktree and session, the
    /// user's order, the accounts sessions run under and the UI state
    /// intact, and gives it a daemon id for its snapshot refs.
    #[test]
    fn a_048_database_upgrades_with_its_rows_and_order_intact() {
        let sql = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../nebula/tests/fixtures/upgrade/v0.48/nebula.sql"
        ))
        .unwrap()
        .replace("@ROOT@", "/fixture");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nebula.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&sql).unwrap();
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(version, 33, "the fixture is a 0.48 database");
        }

        let store = Store::open(&path).unwrap();
        let version: i64 = store
            .conn
            .lock()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        assert_eq!(version, 37);

        let (projects, worktrees, agents, _) = store.load_tree().unwrap();
        // beta was dragged ahead of alpha.
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["beta", "alpha"]);
        let alpha = &projects[1];
        assert_eq!(alpha.repo_path, PathBuf::from("/fixture/repos/alpha"));
        // alpha's worktrees: main first, then fix-typo dragged above feat/login.
        let branches: Vec<&str> = worktrees
            .iter()
            .filter(|w| w.project_id == alpha.id)
            .map(|w| w.branch.as_str())
            .collect();
        assert_eq!(branches, ["main", "fix-typo", "feat/login"]);
        assert_eq!(worktrees.len(), 5);
        // feat/login's sessions: tests dragged above the renamed form.
        let login = worktrees.iter().find(|w| w.branch == "feat/login").unwrap();
        let in_login: Vec<&str> = agents
            .iter()
            .filter(|a| a.worktree_id == login.id)
            .map(|a| a.name.as_str())
            .collect();
        assert_eq!(in_login, ["login tests", "login form (renamed)"]);
        assert_eq!(agents.len(), 6);
        let by_name = |n: &str| agents.iter().find(|a| a.name == n).unwrap();
        assert!(by_name("typo").archived);
        assert_eq!(by_name("login tests").profile.as_deref(), Some("work"));
        assert_eq!(by_name("spike it").profile.as_deref(), Some("spare"));
        let ui = store.load_ui_state().unwrap().unwrap();
        assert!(ui.contains("\"show_archived\":true"), "{ui}");
        // 34 and 35: no recorded bases or snapshots to invent, and an id
        // for this data dir's snapshot refs.
        assert!(store.worktree_bases().unwrap().is_empty());
        assert!(store.snapshot_agents().unwrap().is_empty());
        let id = store.daemon_id().unwrap();
        assert_eq!(id.len(), 16, "{id}");
    }
}
