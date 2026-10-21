# Gauntlet

<sub>[← README](../README.md) · [Studio mode](studio.md) · [Roles](roles.md) · [Keys](keys.md) · [Commands](commands.md) · [Configuration](configuration.md)</sub>

The Gauntlet is a read-only view of every verdict file in a repo. It groups them by kind, newest first. Each verdict shows its word, who gave it, when, its note, its tags and the evidence it points at. A button opens your Studio app on the verdict.

Kete reads the files. It never writes one.

The view shows for a repo that has a [`.kete/studio.toml`](studio.md#ketestudiotoml) with a `signoffs` folder. In the desktop app it has a Gauntlet entry in the rail. In the TUI it is the `gauntlet_view` verb, on the Repo and Global menus and in the command palette.

## What it reads from the manifest

| Key | What the Gauntlet does with it |
|---|---|
| `signoffs` | The folder of verdict files. Each folder under it is a kind: `<signoffs>/<kind>/<id>.json`. Without it the view is empty. |
| `evidence` | The folder a verdict's `evidence` field names. Without it a verdict shows no evidence. |
| `studio_url` | The address of your local Studio app. Without it there is no Open in Studio. |
| `[links]` | Kind to a route on `studio_url`. `{id}` becomes the verdict's id. |
| `[kinds]` | Kind to a department slug. The board uses it for its last-verdict cell. |

```toml
version = 1
signoffs = "reviews/signoffs"
evidence = "reviews/evidence"
studio_url = "http://127.0.0.1:5173"

[links]
art = "/art?id={id}"
sound = "/sound?id={id}"

[kinds]
art = "art-and-3d"
sound = "audio"
```

All paths are relative to the repo root and stay inside it. A path that climbs out with `..`, or names a drive or an absolute folder, is refused and the view is empty.

## Verdict files

One JSON object per file, at `<signoffs>/<kind>/<id>.json`.

```json
{
  "id": "C02",
  "verdict": "split",
  "by": "second-reviewer",
  "at": "2026-10-21T09:30:00Z",
  "tags": ["banding", "sky"],
  "evidence": ["EV1"],
  "note": "Banding in the sky.\nThe second reviewer disagrees."
}
```

| Field | Meaning |
|---|---|
| `id` | The verdict's id. The file name without `.json` when it is missing. |
| `verdict` | The word. Shown as written. Blank when missing. |
| `by` | Who gave it. |
| `at` | When. A date or a date and time. Newest `at` sorts first. |
| `note` | Free text. The row shows its first line, the detail shows all of it. |
| `tags` | A list of words. One string is one tag. |
| `evidence` | Names of folders or files under the manifest's `evidence`. One string is one item. |

The reader is lenient, so a newer Studio app never breaks the view.

- A file that is not a JSON object is skipped. So is a file over 1 MiB.
- A missing field is blank. A number where text is expected is written out as text.
- A field Kete does not know is ignored.
- A kind folder is read up to 5,000 files.
- Rows sort by `at`, latest first and none last, then by the file's modified time, then by id.

**A reconciled pair is one row.** `C02-reconciled.json` belongs to `C02.json`. It does not get a row. The detail of `C02` names it.

**`reviewer` and `date` are not read today.** The Studio app's sound verdicts use that shape, for example `{"id":"tui","verdict":"rejected","reviewer":"Jay","date":"2026-10-19"}`. Such a row shows no by and no at, and it sorts after the rows that have an `at`.

## Verdict words

The word sets the row's tone. Case and outer spaces do not matter.

| Words | Tone | Look |
|---|---|---|
| `approve`, `approved`, `pass` | Done | a check mark |
| `reject`, `rejected`, `regenerate`, `fail` | Failed | a cross |
| `split`, `third-reviewer` | Needs you | the ember, the only place the Gauntlet uses it |
| anything else, or no word | Plain | the word as written, with a dot |

## Open in Studio

The button builds a link from `studio_url` and `links[kind]`. A trailing `/` on `studio_url` is dropped. A route without a leading `/` gets one. In the route, `{id}` is replaced by the verdict's id, percent-encoded: every byte but letters, digits and `-._~` becomes `%XX`. The id `tui call/2` becomes `tui%20call%2F2`.

There is no button when the manifest has no `studio_url`, or when `links` has no route for that kind.

A link opens only when it is plain `http` on `127.0.0.1` or `localhost`, with at most a numeric port. `https://...`, any other host, a name like `localhost@evil.com` or `127.0.0.1.evil.com`, and any other scheme are refused. A cloned repo controls its manifest, so a link never leaves this machine.

## Evidence

A verdict's `evidence` names are looked up under the manifest's `evidence` folder.

- A folder lists one level: its files and subfolders, folders first, then by name.
- A folder lists at most 200 entries. Symlinks are left out.
- A name that is a single file lists that file.
- A name that climbs out of the evidence folder (`..`), or resolves outside the repo through a symlink, lists nothing.
- Each entry shows its repo-relative path and, for a file, its size.

## In the desktop app

Click Gauntlet in the rail.

- **Kind tabs.** One per folder under `signoffs`.
- **Filters.** A verdict word (all words, or one that appears in the kind) and an age: 24 hours, 7 days or all. Age is read from `at`, else from the file's modified time. A verdict with neither shows only under all.
- **Rows.** Id, verdict word with its tone, by, at, and the first line of the note.
- **Detail.** Click a row. It shows the note, the tags, the reconciled file and the evidence list. Open in Studio sits there when the link exists.
- **Evidence entries.** Open in Kete opens the file in Kete's file view. Show in Explorer reveals it in Explorer.
- **Row menu.** Right-click a row: Open in Studio, Copy id, Copy path, Show evidence. Open in Studio is absent without a link. Show evidence is absent when the verdict names no evidence.

The board's last-verdict cell opens the Gauntlet on that verdict.

## In the TUI

Open it with the `gauntlet_view` verb. Rebind it in [keys](keys.md).

| Key | Does |
|---|---|
| `Tab`, `Shift+Tab` | Next and previous kind |
| `↑`, `↓` | Move between rows |
| `Enter` | Open or close the detail |
| `PgUp`, `PgDn` | Scroll a long detail |
| `o` | Open in Studio |
| `/` | Filter the rows |
| `r` | Read the files again |
| `Esc` | Back out: the detail, then a typed filter, then the view |
| `q` | Close (it types while the filter is open) |

A click on a row does what `Enter` does.

## No write actions

The Gauntlet does not approve, reject, send a split to a third reviewer or approve a baseline. Those stay in the Studio app. Open in Studio takes you to the verdict there. This is studio design question 6. Read-only is its default, and the question is pending Jay.

## More

- [Studio mode](studio.md): the manifest, the inbox and autonomy.
- [Roles](roles.md): role cards for the sessions that run your reviews.
