# Usage ledger

The USAGE LEDGER records what each Claude session spent on each local day: input, output and cache tokens, Claude Code's estimated cost, tool uses and turns. It sits in Kete's database, so it outlives the session. `kete usage` prints it, and `kete usage --csv` exports it, for example to commit a studio's ledger at each shift change.

The cost column is Claude Code's own list-price estimate (`cost.total_cost_usd` on the statusLine). It is not a bill, and it ignores what your plan actually charges.

## What is counted

Kete adds to a session's row for today each time one of its turns ends.

| Column | Where it comes from |
|---|---|
| Input, output, cache-read, cache-write tokens | Every `assistant` line's `message.usage` in the session's transcript since the last count, and in its subagents' transcripts (`<session id>/subagents/*.jsonl` beside it). A message is counted once by its `message.id`, however many lines Claude writes for it. |
| Estimated cost | How far `cost.total_cost_usd` rose since the last count. After `/clear` the estimate starts again from 0, so the new value counts whole. |
| Tool uses | `PostToolUse` hooks during the turn. |
| Turns | One for each turn end. |

A day is your local date when the turn ends. A turn that runs past midnight lands on the new day.

Claude writes its transcript asynchronously and runs the statusLine about 300 ms after the last message. A turn's last message can therefore land after the turn has ended. Kete counts a reading that arrives while the session is idle straight away, without a turn, so nothing is lost. A turn that starts just before midnight can still put its last few tokens on the next day.

Only Claude sessions are counted. Other harnesses report no per-message usage that Kete reads.

### What is not counted twice

- When the daemon restarts, it does not read a session's transcript lines from before the restart again. Those were counted by the daemon that saw them.
- A resumed conversation's history is not counted again: only lines written after the session was created count.
- When the daemon first sees a session that is older than itself, it takes that reading's cost as already counted.

### What is missed

- With `usage_statusline` off, Claude gets no statusLine from Kete, so the cost column stays at 0. Tokens still come from the transcript.
- Spend in the moments between a daemon stopping and starting again is not counted.
- The last reading of a session that is closed in the middle of a turn is not counted.

## kete usage

```text
kete usage [--since YYYY-MM-DD] [--until YYYY-MM-DD] [--project P] [--by session|day|role] [--csv]
```

With no dates it shows today. `--since` alone runs to the latest day, and `--until` alone from the first. `--project` takes a project's name or id. The table sums the rows by session (the default), by day or by role card, with a total line.

```text
$ kete usage --since 2026-10-21 --by role
Usage, since 2026-10-21 (cost is Claude Code's estimate, not a bill)

role            input      output    cache read   cache write   est. US$  tool uses  turns
runner           1500         300             0             0     0.0800          0      2
(no role)          40          10        120000          3000     0.4100          9      3
total            1540         310        120000          3000     0.4900          9      5
```

`--csv` prints every row, one per session per day, under this header:

```text
date,project,branch,session,role,account,model,input_tokens,output_tokens,cache_read_tokens,cache_write_tokens,cost_usd_estimate,tool_uses,turns,status,pr_url
```

`role` is the session's role card, `account` its profile, and `status` and `pr_url` what the session is now (or was, once deleted). The cost has four decimals. A field holding a comma or a quote is quoted, and quotes in it are doubled.

## In the apps

- **Terminal app:** the **Export usage CSV** command (command palette, "usage csv") writes today's rows to `<data dir>/exports/usage-<YYYY-MM-DD>.csv` and says where in the footer.
- **Desktop app:** the session details rail shows today's line for the session, for example `today: 412k tokens, est. US$3.10, 58 tool uses, 9 turns`. Its tokens are input plus output; cache tokens are left out. **Export usage CSV** writes the same file as the terminal app and shows it in Explorer.
- The studio board's spend cell reads the same rows.

## Turning it off

`usage_ledger` in Settings, Sessions, Usage (`on` by default). Off keeps the rows already recorded and adds none. Turning it back on counts only what is spent from then on.
