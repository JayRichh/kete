# Lane K10: state map

Added 2026-10-21 from Jay's ruling (WHEKO grill G-BUS-048): "a persistent, always-built map of context and state: hierarchy, workspace tree, agent progress, files, visible to everyone".

**Goal.** One persistent state graph per registered repo, rebuilt by the daemon on every session event (start, stop, PR opened, merge) and on a 60 s timer, served to clients and written as `<repo>/.kete/state-map.json` (gitignored). Nodes: department, system, subsystem, decision, lane, PR, file, agent. Each node: id (the repo's identifier scheme where one exists), status, owner, last change, open questions count, links. Edges: owns, depends on, touched by, blocked by.

**Sources.** For WHEKO: `Design/Production/DEPARTMENTS.md`, `Design/Mechanics/SYSTEMS-MAP.md`, `Design/Analysis/ENRICHMENT/SYSTEM-CONNECTIONS.md`, `DECISIONS/README.md`, `Design/Studio/grill/ledger-*.md`, `git worktree list`, open PRs from `gh`, session and subagent status from the daemon. The repo-side extractor is a small script the repo provides (`Tools/Studio/state_map.py` for WHEKO, to be written by the WHEKO side); Kete calls it and merges the live layer.

**Surfaces.** Desktop: a tree plus graph view on the studio board (K4) with a compact text slice (under 2 KB) copyable for an agent prompt; TUI: a tree overlay. CLI: `kete state --json`, `kete state --slice <node>` (the compact slice an agent reads at session start).

**Files owned.** `crates/nebula-daemon/src/state_map.rs`, `crates/nebula-core/src/state_map.rs` (types), `desktop/src/lib/state/`, `crates/nebula-tui/src/state_overlay.rs`, `docs/state-map.md`. Links to: the board lane (K4) for the panel slot, foundation for the protocol message.

**Interfaces.** Consumes: foundation's protocol 66 message `StateMap` (add the variant in foundation if not present; coordinate). Produces: `StateMap { repo, built_at, nodes[], edges[] }`, `kete state` CLI.

**Tests.** Unit: extractor merge, slice size cap, rebuild on events. Windows e2e: a fake repo with three nodes renders in the tree.

**Stop.** Green PR into `release/0.60`, plain commits, no attribution. Wave 2, after foundation and board.
