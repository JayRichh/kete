//! `kete _guard`: the shell-command check a role or studio session runs
//! before every Bash and PowerShell call (decision 13).
//!
//! It splits a command line on `&&`, `||`, `;`, `|`, `&`, newlines,
//! parentheses and braces (so `$(…)` and subshells are seen), drops
//! environment prefixes and wrappers (`GIT_TRACE=1`, `env`, `sudo`, …),
//! looks through `bash -c` and `pwsh -Command` strings, and walks git's
//! own options (`-C <dir>`, `-c k=v`) to the subcommand. It refuses any
//! force-push form, a `+` refspec, `--mirror`, deleting `main` or `master`
//! on a remote, and what the autonomy level denies.
//!
//! It is a second line behind the deny rules, not a sandbox: a script that
//! runs git itself (`python -c "subprocess.run([...])"`) or a git alias
//! from the user's config passes it (spec section 14).

use crate::studio::autonomy::Autonomy;
use serde_json::Value;

/// The shell command in a PreToolUse payload (`tool_input.command`).
pub fn command_of(payload: &Value) -> Option<&str> {
    payload.get("tool_input")?.get("command")?.as_str()
}

/// Refuses `command` with the reason, or lets it run. `autonomy` is the
/// repo's level; a session outside any studio repo passes `A3`, which
/// denies nothing of its own.
pub fn check(command: &str, autonomy: Autonomy) -> Result<(), String> {
    check_line(command, autonomy, 0)
}

/// How deep `bash -c "bash -c …"` is followed.
const MAX_DEPTH: usize = 4;

fn check_line(line: &str, autonomy: Autonomy, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("kete guard: the command nests shells too deeply to check".into());
    }
    // A backtick is command substitution to Bash and an escape to
    // PowerShell, and a backslash escapes to Bash but is a path separator to
    // PowerShell: read the line every way, and refuse what any reading does.
    let unticked = line.contains('`').then(|| line.replace('`', ""));
    for text in std::iter::once(line).chain(unticked.as_deref()) {
        for escapes in [true, false] {
            for words in segments(text, escapes) {
                check_words(&words, autonomy, depth)?;
            }
        }
    }
    Ok(())
}

/// Each simple command's words, quotes resolved, split on shell operators
/// outside quotes. With `escapes`, a backslash escapes as Bash reads it
/// (any character outside quotes; `"`, `\`, `$` and a backtick inside
/// double quotes); without, it is a plain character.
fn segments(line: &str, escapes: bool) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    let end_word = |words: &mut Vec<String>, word: &mut String, in_word: &mut bool| {
        if *in_word {
            words.push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    word.push(q);
                }
            }
            '"' => {
                in_word = true;
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' if escapes && chars.peek().is_some_and(|n| "\"\\$`".contains(*n)) => {
                            word.extend(chars.next());
                        }
                        _ => word.push(q),
                    }
                }
            }
            '\\' if escapes => {
                in_word = true;
                word.extend(chars.next());
            }
            ';' | '&' | '|' | '\n' | '\r' | '(' | ')' | '{' | '}' | '`' => {
                end_word(&mut words, &mut word, &mut in_word);
                if !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
            }
            c if c.is_whitespace() => end_word(&mut words, &mut word, &mut in_word),
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    end_word(&mut words, &mut word, &mut in_word);
    if !words.is_empty() {
        out.push(words);
    }
    out
}

/// A program's name as typed: basename, lowercased, `.exe` dropped.
fn program(word: &str) -> String {
    let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let base = base.to_ascii_lowercase();
    base.strip_suffix(".exe").unwrap_or(&base).to_string()
}

/// Whether `word` is a `NAME=value` environment prefix.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit())
    })
}

/// Commands that run the rest of their words as a command.
const WRAPPERS: &[&str] = &[
    "env", "sudo", "command", "exec", "nohup", "time", "nice", "xargs", "builtin", "doas",
];

fn check_words(words: &[String], autonomy: Autonomy, depth: usize) -> Result<(), String> {
    let mut i = 0;
    // Environment prefixes and wrappers (with their own options) first.
    loop {
        let Some(word) = words.get(i) else {
            return Ok(());
        };
        if is_assignment(word) {
            // `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_KEY_<n>`: config given
            // through the environment, refused like `-c`.
            if let Some((name, value)) = word.split_once('=') {
                if name.starts_with("GIT_CONFIG") && mentions_forcing_key(value) {
                    return Err(format!(
                        "kete guard: `{name}` sets git config that can force a push, refused in this session"
                    ));
                }
            }
            i += 1;
        } else if WRAPPERS.contains(&program(word).as_str()) {
            i += 1;
            while words.get(i).is_some_and(|w| w.starts_with('-')) {
                i += 1;
            }
        } else {
            break;
        }
    }
    let prog = program(&words[i]);
    let rest = &words[i + 1..];
    match prog.as_str() {
        "git" => check_git(rest, autonomy),
        "gh" => check_gh(rest, autonomy),
        "kete" | "bro" => check_kete(&prog, rest, autonomy),
        "bash" | "sh" | "zsh" | "dash" | "ksh" | "fish" => {
            match rest.iter().position(|w| {
                w == "-c" || (w.starts_with('-') && !w.starts_with("--") && w.contains('c'))
            }) {
                Some(at) => match rest.get(at + 1) {
                    Some(inner) => check_line(inner, autonomy, depth + 1),
                    None => Ok(()),
                },
                None => Ok(()),
            }
        }
        "pwsh" | "powershell" | "cmd" => {
            let at = rest.iter().position(|w| {
                let w = w.to_ascii_lowercase();
                matches!(
                    w.as_str(),
                    "-c" | "-command" | "/c" | "/k" | "-encodedcommand" | "-ec" | "-e"
                )
            });
            match at {
                Some(at) => {
                    let flag = rest[at].to_ascii_lowercase();
                    if matches!(flag.as_str(), "-encodedcommand" | "-ec" | "-e") {
                        return Err(
                            "kete guard: an encoded PowerShell command cannot be checked".into(),
                        );
                    }
                    check_line(&rest[at + 1..].join(" "), autonomy, depth + 1)
                }
                None => Ok(()),
            }
        }
        "iex" | "invoke-expression" => check_line(&rest.join(" "), autonomy, depth + 1),
        _ => Ok(()),
    }
}

/// git's own options that take the next word as their value.
const GIT_VALUE_OPTS: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
    "--config-env",
    "--exec-path",
];

fn level_refusal(autonomy: Autonomy, what: &str) -> String {
    format!("kete guard: autonomy {autonomy} for this repo does not allow `{what}`")
}

fn check_git(args: &[String], autonomy: Autonomy) -> Result<(), String> {
    let mut i = 0;
    while let Some(word) = args.get(i) {
        if !word.starts_with('-') {
            break;
        }
        let value = if word.contains('=') || !GIT_VALUE_OPTS.contains(&word.as_str()) {
            None
        } else {
            args.get(i + 1)
        };
        // `-c alias.x=…` defines a command on the spot, and `-c
        // remote.<r>.mirror=true` or `-c remote.<r>.push=+…` turns a plain
        // `git push` into a mirror or force push.
        let config = match word.as_str() {
            "-c" | "--config-env" => value.map(String::as_str),
            _ => word
                .strip_prefix("--config-env=")
                .or_else(|| word.strip_prefix("-c")),
        };
        if let Some(key) = config.filter(|c| forcing_key(c)) {
            return Err(format!(
                "kete guard: setting `{}` with -c is refused in this session",
                key.split('=').next().unwrap_or(key).trim()
            ));
        }
        i += if value.is_some() { 2 } else { 1 };
    }
    let Some(sub) = args.get(i) else {
        return Ok(());
    };
    let rest = &args[i + 1..];
    match sub.as_str() {
        "push" => check_push(rest, autonomy),
        "config" => match rest.iter().find(|w| !w.starts_with('-') && forcing_key(w)) {
            Some(key) => Err(format!(
                "kete guard: `git config {key}` can force a push, refused in this session"
            )),
            None => Ok(()),
        },
        "tag" if autonomy != Autonomy::A3 => Err(level_refusal(autonomy, "git tag")),
        _ => Ok(()),
    }
}

/// Whether `entry` (`key` or `key=value`) is git config that defines a
/// command (`alias.*`) or a mirror or forced push (`remote.<r>.mirror`,
/// `remote.<r>.push`). Keys match in any case, as git reads them.
fn forcing_key(entry: &str) -> bool {
    let key = entry
        .split('=')
        .next()
        .unwrap_or(entry)
        .trim()
        .to_ascii_lowercase();
    key.starts_with("alias.")
        || (key.starts_with("remote.") && (key.ends_with(".mirror") || key.ends_with(".push")))
}

/// [`forcing_key`] anywhere in an environment value
/// (`GIT_CONFIG_PARAMETERS` quotes its keys).
fn mentions_forcing_key(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    v.contains("alias.")
        || (v.contains("remote.") && (v.contains(".mirror") || v.contains(".push")))
}

/// Whether `name` is `full` or an abbreviation git takes for it: git
/// accepts any unambiguous prefix of a long option (`--forc`, `--mirro`).
fn abbreviates(name: &str, full: &str, min: usize) -> bool {
    name.len() >= min && full.starts_with(name)
}

/// `git push` options that take the next word as their value.
const PUSH_VALUE_OPTS: &[&str] = &["-o", "--push-option", "--repo", "--receive-pack", "--exec"];

fn check_push(args: &[String], autonomy: Autonomy) -> Result<(), String> {
    let mut delete = false;
    let mut tags = false;
    let mut positional: Vec<&str> = Vec::new();
    let mut i = 0;
    while let Some(word) = args.get(i) {
        i += 1;
        if word == "--" {
            positional.extend(args[i..].iter().map(String::as_str));
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            let name = long.split('=').next().unwrap_or(long);
            // Abbreviations count as the option they shorten; an ambiguous
            // one (`--fo`) is refused too, git would not run it anyway.
            if ["force", "force-with-lease", "force-if-includes"]
                .iter()
                .any(|full| abbreviates(name, full, 1))
            {
                return Err(format!(
                    "kete guard: force-push is refused in this session (`--{name}`)"
                ));
            }
            if abbreviates(name, "mirror", 1) {
                return Err("kete guard: `git push --mirror` is refused in this session".into());
            }
            delete |= abbreviates(name, "delete", 2);
            tags |= abbreviates(name, "tags", 2) || abbreviates(name, "follow-tags", 3);
            if !long.contains('=') && PUSH_VALUE_OPTS.contains(&word.as_str()) {
                i += 1;
            }
            continue;
        }
        if let Some(short) = word.strip_prefix('-').filter(|s| !s.is_empty()) {
            if short.contains('f') {
                return Err("kete guard: force-push is refused in this session (`-f`)".into());
            }
            if short.contains('d') {
                delete = true;
            }
            if PUSH_VALUE_OPTS.contains(&word.as_str()) {
                i += 1;
            }
            continue;
        }
        positional.push(word);
    }
    if positional.iter().any(|w| w.starts_with('+')) {
        return Err(
            "kete guard: a `+` refspec force-pushes, and is refused in this session".into(),
        );
    }
    let refspecs = positional.get(1..).unwrap_or_default();
    for spec in refspecs {
        let target = match spec.strip_prefix(':') {
            Some(target) => target,
            None if delete => spec,
            None => continue,
        };
        let branch = target.strip_prefix("refs/heads/").unwrap_or(target);
        // shortcut: main and master stand for the remote's default branch;
        // reading origin/HEAD would need git, and the guard runs none.
        if matches!(branch, "main" | "master") {
            return Err(format!(
                "kete guard: deleting `{branch}` on the remote is refused in this session"
            ));
        }
    }
    match autonomy {
        Autonomy::A0 | Autonomy::A1 => Err(level_refusal(autonomy, "git push")),
        Autonomy::A2 if tags || refspecs.iter().any(|s| s.contains("refs/tags/")) => {
            Err(level_refusal(autonomy, "git push of tags"))
        }
        _ => Ok(()),
    }
}

fn check_gh(args: &[String], autonomy: Autonomy) -> Result<(), String> {
    let Some(sub) = args.iter().find(|w| !w.starts_with('-')) else {
        return Ok(());
    };
    match (sub.as_str(), autonomy) {
        ("pr", Autonomy::A0 | Autonomy::A1) => Err(level_refusal(autonomy, "gh pr")),
        ("release", Autonomy::A0 | Autonomy::A1 | Autonomy::A2) => {
            Err(level_refusal(autonomy, "gh release"))
        }
        _ => Ok(()),
    }
}

fn check_kete(prog: &str, args: &[String], autonomy: Autonomy) -> Result<(), String> {
    if !matches!(autonomy, Autonomy::A0 | Autonomy::A1) {
        return Ok(());
    }
    let words: Vec<&str> = args
        .iter()
        .filter(|w| !w.starts_with('-'))
        .map(String::as_str)
        .collect();
    match words.as_slice() {
        ["ship", ..] => Err(level_refusal(autonomy, &format!("{prog} ship"))),
        ["ado", "pr", ..] => Err(level_refusal(autonomy, &format!("{prog} ado pr"))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Autonomy::*;

    /// Refused at every level: force forms, `+` refspecs, mirrors and
    /// default-branch deletes.
    const ALWAYS_REFUSED: &[&str] = &[
        "git push --force",
        "git push -f origin x",
        "git push origin +main",
        "git -C ../x push --force-with-lease",
        "GIT_TRACE=1 git push -f",
        "cd x && git push --mirror",
        "git push origin :main",
        "git push origin --delete master",
        "git push -d origin main",
        "git push origin :refs/heads/main",
        "git push --force-with-lease=main:abc origin main",
        "git push --force-if-includes origin x",
        "git push -uf origin x",
        "git push origin x --force",
        "git -c core.x=1 push -f",
        "git --no-pager push --force",
        "git.exe push --force",
        r#""C:\Program Files\Git\bin\git.exe" push -f"#,
        r"C:\Git\bin\git.exe push --force",
        "/usr/bin/git push -f",
        "env FOO=1 git push -f",
        "sudo -E git push --force",
        "echo hi; git push -f",
        "git status || git push -f",
        "true | git push -f",
        "echo $(git push -f)",
        "echo `git push -f`",
        "(cd x; git push -f)",
        "bash -c 'git push --force'",
        "sh -lc \"git push -f\"",
        "pwsh -Command git push --force",
        "powershell -c \"git push -f\"",
        "Invoke-Expression 'git push --force'",
        "git push \"--force\"",
        "git push -\\f",
        "git push `-f",
        "git -c alias.p='push -f' p",
        "git -c Alias.P='push -f' p",
        "pwsh -EncodedCommand ZwBpAHQA",
    ];

    #[test]
    fn force_forms_are_refused_at_every_level() {
        for level in Autonomy::ALL {
            for cmd in ALWAYS_REFUSED {
                assert!(check(cmd, level).is_err(), "{cmd} at {level}");
            }
        }
        let why = check("git push --force", A3).unwrap_err();
        assert!(why.contains("force-push"), "{why}");
    }

    /// Pushes that are fine wherever pushing is (A2 and A3).
    const SAFE_PUSHES: &[&str] = &[
        "git push",
        "git push -u origin feat",
        "git push origin feature-force-fix",
        "git push origin HEAD:feat",
        "git push --set-upstream origin lane-060-roles",
        "git push origin --delete old-lane",
        "git -C ../x push origin x",
        "git push -o ci.skip origin x",
        "git push --dry-run origin x",
        "git -c core.autocrlf=false push origin x",
        "git -c remote.origin.url=x push origin x",
    ];

    /// Never refused at any level.
    const ALWAYS_SAFE: &[&str] = &[
        "git status",
        "echo \"--force\"",
        "echo 'git push -f'",
        "git log --oneline -5",
        "git commit -m 'force push fix'",
        "cargo test -p nebula-core",
        "gh issue list",
        "kete sessions",
        "git fetch origin && git merge --ff-only origin/main",
        "git config user.name kete",
        "git config --get remote.origin.url",
        "GIT_CONFIG_NOSYSTEM=1 git status",
        "",
    ];

    /// git takes any unambiguous prefix of a long option, so each
    /// shortened force or mirror spelling is refused like the full one.
    #[test]
    fn abbreviated_force_and_mirror_options_are_refused() {
        for cmd in [
            "git push --forc origin x",
            "git push --force-w origin x",
            "git push --force-with origin x",
            "git push --force-i origin x",
            "git push --fo origin x",
            "git push origin x --forc",
            "git push --mirro origin",
            "git push --mirr",
            "git push --mi origin",
            "git push --m origin",
        ] {
            for level in Autonomy::ALL {
                assert!(check(cmd, level).is_err(), "{cmd} at {level}");
            }
        }
        assert!(check("git push --forc", A3)
            .unwrap_err()
            .contains("force-push"));
        assert!(check("git push --mirro", A3)
            .unwrap_err()
            .contains("--mirror"));
        assert!(
            check("git push origin --de main", A3).is_err(),
            "--de is --delete"
        );
    }

    /// Config that makes a plain `git push` a mirror or force push, given
    /// inline, through the environment, or written with `git config`.
    #[test]
    fn config_that_forces_a_push_is_refused() {
        for cmd in [
            "git -c remote.origin.mirror=true push origin",
            "git -c Remote.Origin.Mirror=true push origin",
            "git -c remote.origin.push=+HEAD:main push origin",
            "git -c remote.origin.push=HEAD:main push origin",
            "git -cremote.origin.mirror=true push origin",
            "git --config-env=remote.origin.push=PUSHSPEC push origin",
            "git --config-env remote.origin.mirror=MIRROR push origin",
            "git config remote.origin.mirror true",
            "git config --add remote.origin.push +HEAD:main",
            "git config alias.pf 'push --force'",
            "git config --global alias.pf 'push --force'",
            "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=remote.origin.mirror GIT_CONFIG_VALUE_0=true git push origin",
            "GIT_CONFIG_PARAMETERS=\"'remote.origin.push'='+HEAD:main'\" git push origin",
        ] {
            for level in Autonomy::ALL {
                assert!(check(cmd, level).is_err(), "{cmd} at {level}");
            }
        }
        let why = check("git -c remote.origin.mirror=true push origin", A3).unwrap_err();
        assert!(why.contains("remote.origin.mirror"), "{why}");
    }

    #[test]
    fn safe_commands_pass() {
        for level in Autonomy::ALL {
            for cmd in ALWAYS_SAFE {
                assert_eq!(check(cmd, level), Ok(()), "{cmd} at {level}");
            }
        }
        for level in [A2, A3] {
            for cmd in SAFE_PUSHES {
                assert_eq!(check(cmd, level), Ok(()), "{cmd} at {level}");
            }
        }
    }

    #[test]
    fn each_level_refuses_its_commands() {
        // (command, refused at A0, A1, A2, A3)
        let table: &[(&str, [bool; 4])] = &[
            ("git push", [true, true, false, false]),
            ("git push -u origin feat", [true, true, false, false]),
            ("git tag v1", [true, true, true, false]),
            ("git tag -l", [true, true, true, false]),
            ("git push --tags", [true, true, true, false]),
            ("git push origin --follow-tags", [true, true, true, false]),
            ("git push origin refs/tags/v1", [true, true, true, false]),
            ("gh pr create --fill", [true, true, false, false]),
            ("gh release create v1", [true, true, true, false]),
            ("kete ship", [true, true, false, false]),
            ("bro ship --yes", [true, true, false, false]),
            ("kete ado pr create", [true, true, false, false]),
            ("bro ado pr", [true, true, false, false]),
            ("kete ado sync", [false, false, false, false]),
            ("cd x && git tag v2", [true, true, true, false]),
            ("git push --tag", [true, true, true, false]),
            ("git push origin --follow", [true, true, true, false]),
        ];
        for (cmd, refused) in table {
            for (level, want) in Autonomy::ALL.iter().zip(refused) {
                assert_eq!(check(cmd, *level).is_err(), *want, "{cmd} at {level}");
            }
        }
        let why = check("gh release create v1", A2).unwrap_err();
        assert!(why.contains("A2 (Deliver)"), "{why}");
    }

    #[test]
    fn the_payload_command_is_read() {
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": "git push --force" },
        });
        assert_eq!(command_of(&payload), Some("git push --force"));
        assert_eq!(command_of(&serde_json::json!({ "tool_input": {} })), None);
    }

    #[test]
    fn nesting_is_bounded() {
        let mut cmd = "git push -f".to_string();
        for _ in 0..8 {
            cmd = format!("bash -c {}", crate::shell::single_quote(&cmd));
        }
        assert!(check(&cmd, A3).is_err());
    }
}
