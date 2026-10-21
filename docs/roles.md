# Roles

<sub>[← README](../README.md) · [Studio mode](studio.md) · [Commands](commands.md) · [Orchestrator](orchestrator.md) · [Profiles](profiles.md) · [Configuration](configuration.md)</sub>

A role card is a named way to start a Claude session. It sets the model, the effort, the tools the session has, its MCP servers, a prompt appended to Claude's system prompt, and how its permission prompts are handled. A repo defines its roles in `.kete/roles.toml`, a file you commit.

```sh
kete orchestrator spawn --project my-game --role runner "run the audio checks"
kete spawn --role runner "check the build"        # from inside a session
kete roles check --project my-game                 # what every role gets, before a shift
```

The quick prompt in the desktop app and the Start orchestrator form show a role picker when the repo has roles. A role session's chip shows its label beside the account.

## The roles file

```toml
version = 1

# Repo-relative paths no role session may edit.
governance = ["docs/decisions/**", "docs/QUESTIONS.md", ".kete/**"]

[roles.runner]
label = "Runner"                 # chip text; default: the id
model = "haiku"                  # alias or full model id; a --model on the launch wins
effort = "low"                   # low, medium, high, xhigh or max
department = "audio"             # board column; optional
tier = 3                         # 0 to 3; default 2
context_budget = 100000          # tokens; the board warns at 60 and 90 percent
prompt_file = ".kete/roles/runner.md"   # or prompt = "..."; at most 8,192 bytes
tools = ["Read", "Write", "Bash"]       # the session's whole tool set; leave out for Claude's default
allow = ["Bash(uv run tools/check.py*)"]  # calls that run without asking
deny = ["WebFetch", "WebSearch"]          # more calls that never run
mcp = "none"                     # none (no MCP servers) or default
permission = "allowlist"         # prompt (default), allowlist or unattended
```

Limits: 64 roles, 65,536 bytes, an 8,192-byte prompt, tiers 0 to 3. A key Kete does not know is refused, not ignored, so a misspelt `permission` never starts a session with fewer restrictions than you wrote. A file that does not parse refuses the launch and names the line. Kete never starts a role session without its restrictions.

**Kete reads the file from the last commit.** It runs `git show HEAD:.kete/roles.toml` in the project's root worktree. An uncommitted edit changes nothing. A `prompt_file` is read from the same commit.

**A card edit reaches new sessions only.** Claude records a session's system prompt on its first request and reuses it on every resume, so a running or resumed session keeps the prompt it started with. Flags and rules are applied again on every resume.

**Roles are for Claude sessions.** A role on another harness is refused with the reason.

## Permission profiles

| Profile | Claude flag | Allow rules | Deny rules |
|---|---|---|---|
| `prompt` | none (Claude's own starting mode) | the role's `allow` | built-in, governance, autonomy, the role's `deny` |
| `allowlist` | `--permission-mode dontAsk` | the role's `allow`; anything that would prompt is denied | the same, plus `AskUserQuestion` |
| `unattended` | `--permission-mode bypassPermissions` | none: Claude ignores allow rules in this mode | the same, plus `AskUserQuestion` |

Claude's own starting mode is auto mode from Claude Code 2.1.283 on: a classifier, not you, approves most actions in a `prompt` session. Allow rules from Claude's other settings (the account's user settings, the repo's `.claude/settings.json`, and Kete's own rules for `kete spawn`, `kete rename`, `kete worktree` and `kete open`) still apply on top of the role's, so an allowlist session runs those calls too.

Deny rules hold in every mode, bypass included. A role launch is refused when the Claude harness carries a permissions flag (a `harnesses` override in Settings), since that flag would skip every check. `AskUserQuestion` is denied to allowlist and unattended sessions because nobody is there to answer it. Their prompts should say to write `pending Jay: A | B` into the inbox instead.

Every role session gets its own settings file, `<data dir>/sessions/<agent id>/claude-settings.json`, passed with `--settings`. It holds Kete's status line, the rules and the guard. Rules never go on the command line.

A session the orchestrator spawns in a [studio repo](studio.md) without a role gets the same file with the built-in, governance and autonomy denies and the guard. Sessions you start yourself without a role are never restricted.

## Built-in denies

A roles file cannot remove these:

- Force-push, as Bash and PowerShell rules: `git push --force*`, `git push -f*`, `git push * --force*`, `git push * -f*`, `git push --force-with-lease*`, `git push * --force-with-lease*`, `git push * +*`, `git push --mirror*`, `git push * --mirror*`.
- Edits to Kete's data and config folders, to `.claude/**` and `.kete/**` in the session's worktree and in the root worktree, and to the session's own settings file.
- The governance paths, made absolute for the session's worktree and for the root worktree.

Path rules are written in the absolute form Claude matches: `C:\Code\my-game\DECISIONS\**` becomes `Edit(//c/Code/my-game/DECISIONS/**)`. They are `Edit` rules only. Claude never consults a `Write(path)` rule, and an `Edit` rule covers every tool that writes a file.

## The guard

`kete _guard --session <id>` runs before every Bash and PowerShell command a role or studio session makes. It is a PreToolUse hook in the session's settings file. It splits the command on `&&`, `||`, `;`, pipes and subshells, looks through `bash -c` and `pwsh -Command`, skips environment prefixes and wrappers such as `env` and `sudo`, and follows git's own options (`-C <dir>`, `-c k=v`) to the subcommand. It refuses:

- any force flag, a `+` refspec and `--mirror`, including the shortened spellings git accepts (`--forc`, `--force-w`, `--mirro`);
- deleting `main` or `master` on a remote;
- git config that defines a command or forces a push (`alias.*`, `remote.<r>.mirror`, `remote.<r>.push`), given with `-c` or `--config-env`, or written with `git config`;
- any command line that mentions `GIT_CONFIG` (in any case, so `$env:` forms too) together with `alias.`, `remote.<r>.mirror`, `remote.<r>.push`, `push.default` or a `+` refspec, wherever the variable is set in the line (`export`, `$env:` or a prefix);
- an encoded PowerShell command;
- what the repo's autonomy level denies (see [Studio mode](studio.md)).

It reads the repo's live autonomy from `config.local.json` on every command, so a level you lower applies from the next command. Without a studio entry it falls back to the level the settings file was written with. It never asks the daemon, so a hung daemon cannot let a push through.

## Bypass acceptance

Claude asks once per account before its first session in bypass mode. Accepting writes `skipDangerousModePermissionPrompt: true` into that account's Claude settings. Each Kete profile has its own Claude settings, so each account accepts once. Kete never writes that key.

An unattended launch on an account that has not accepted is refused:

```
account max2 has not accepted bypass mode: run `kete roles accept-bypass max2` in your own terminal
```

`kete roles accept-bypass max2` starts Claude in bypass mode under that account in your terminal. You answer its dialog yourself, then type `/exit`. It refuses to run inside a Kete session. The refusal is also written to `<data dir>/studio/refusals.jsonl`, which the studio board shows.

## What deny rules do not catch

The rules and the guard read the command Claude writes. They are not a sandbox.

- A script that opens files or runs git itself, such as `python -c "subprocess.run(['git', 'push', '-f'])"`, passes both. Keep branch protection on your remote's default branch: the remote is the boundary that holds.
- A git alias or a `remote.<r>.mirror` or `remote.<r>.push` entry already in a git config file (yours, or one a session edits as a file rather than with `git config`) passes the guard.
- At A2 a tag pushed by its short name (`git push origin v1.0`) passes: the guard and the rules see only `--tags`, `--follow-tags` and `refs/tags/`.
- `Edit` deny rules cover Claude's file tools, the shell file commands Claude recognises and redirects, not a program that writes files on its own.
- A session can run `kete spawn` without a role. Inside a studio repo the orchestrator's own spawns are restricted; a sibling another session starts without a role is not.
- Allow rules in your other Claude settings files pre-approve calls in an allowlist session too.
- A mod you install for Claude can approve a call a deny rule or the guard refused, unless managed settings or a Team or Enterprise login are in force.

## kete roles check

```sh
kete roles check [--project P] [--role R] [--json]
```

Prints each role's flags in launch order and the settings file a session in the root worktree would get, from the committed roles file, and the autonomy level in force. With an unattended role it also lists each Claude account and whether it accepted bypass mode.
