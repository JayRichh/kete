//! A role session's permission rules, and the settings file that carries
//! them (spec section 7, decision 8).
//!
//! Deny rules block in every Claude permission mode, `bypassPermissions`
//! included; allow rules do nothing in that mode. Path rules are `Edit`
//! only: Claude never consults a `Write(path)` rule (it warns at startup),
//! and an `Edit` rule covers every built-in tool that writes a file.

use std::path::Path;

use serde_json::{json, Value};

use super::card::{Permission, RoleCard};
use crate::studio::autonomy::Autonomy;

/// Force-push command patterns, denied as Bash and PowerShell rules in
/// every role and studio session. A roles file cannot remove them.
pub const BUILTIN_FORCE_PUSH: &[&str] = &[
    "git push --force*",
    "git push -f*",
    "git push * --force*",
    "git push * -f*",
    "git push --force-with-lease*",
    "git push * --force-with-lease*",
    "git push * +*",
    "git push --mirror*",
    "git push * --mirror*",
];

/// The tool an unattended session would wait on a person for.
pub const ASK_USER_QUESTION: &str = "AskUserQuestion";

/// The settings `env` keys the guard reads back: the repo whose live
/// autonomy applies, and the level the session started with.
pub const ENV_STUDIO_REPO: &str = "KETE_STUDIO_REPO";
pub const ENV_STUDIO_AUTONOMY: &str = "KETE_STUDIO_AUTONOMY";

/// What a session's rules are composed from.
#[derive(Debug, Clone, Copy)]
pub struct ComposeCtx<'a> {
    /// The role; None for a session the orchestrator spawned without one
    /// (the `prompt` profile with no role rules).
    pub card: Option<&'a RoleCard>,
    /// The roles file's `governance` paths, repo-relative.
    pub governance: &'a [String],
    /// The repo's autonomy, when it has a `studio` entry.
    pub autonomy: Option<Autonomy>,
    pub worktree: &'a Path,
    pub root_worktree: &'a Path,
    /// Kete's data and config folders.
    pub data_dir: &'a Path,
    pub config_dir: &'a Path,
    /// The session's own settings file.
    pub settings_file: &'a Path,
}

/// A session's starting mode and its rules, in the order they are written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRules {
    /// `--permission-mode`, or None for Claude's own starting mode.
    pub mode: Option<&'static str>,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// A path in the absolute form Claude's path rules match: `//c/Users/x`
/// on Windows (drive lowercased, forward slashes), `//home/x` elsewhere.
/// No trailing slash.
pub fn posix_rule_path(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let raw = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
    let mut p = raw.replace('\\', "/");
    let bytes = p.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        p = format!("/{}{}", (bytes[0] as char).to_ascii_lowercase(), &p[2..]);
    }
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    format!("/{p}")
}

/// `Edit(<dir>/<rel>)` in rule form.
fn edit_under(dir: &Path, rel: &str) -> String {
    let rel = rel.trim().trim_start_matches("./").replace('\\', "/");
    format!("Edit({}/{rel})", posix_rule_path(dir))
}

fn push_unique(rules: &mut Vec<String>, rule: String) {
    if !rules.contains(&rule) {
        rules.push(rule);
    }
}

/// The built-in denies: force-push, then edits to Kete's own folders, to
/// `.claude/**` and `.kete/**` in both worktrees, and to the settings file.
pub fn builtin_denies(ctx: &ComposeCtx<'_>) -> Vec<String> {
    let mut deny = Vec::new();
    for shell in ["Bash", "PowerShell"] {
        for pattern in BUILTIN_FORCE_PUSH {
            deny.push(format!("{shell}({pattern})"));
        }
    }
    push_unique(&mut deny, edit_under(ctx.data_dir, "**"));
    push_unique(&mut deny, edit_under(ctx.config_dir, "**"));
    for dir in [ctx.worktree, ctx.root_worktree] {
        push_unique(&mut deny, edit_under(dir, ".claude/**"));
        push_unique(&mut deny, edit_under(dir, ".kete/**"));
    }
    push_unique(
        &mut deny,
        format!("Edit({})", posix_rule_path(ctx.settings_file)),
    );
    deny
}

/// Every rule a session gets: allow = the role's `allow` (none under
/// `unattended`, where allow rules do nothing); deny = built-in, then
/// governance for both worktrees, then autonomy, then the role's `deny`,
/// then `AskUserQuestion` for the allowlist and unattended profiles.
pub fn compose(ctx: &ComposeCtx<'_>) -> SessionRules {
    let permission = ctx.card.map(|c| c.permission).unwrap_or_default();
    let mut deny = builtin_denies(ctx);
    for path in ctx.governance {
        for dir in [ctx.worktree, ctx.root_worktree] {
            push_unique(&mut deny, edit_under(dir, path));
        }
    }
    if let Some(level) = ctx.autonomy {
        for rule in level.denies() {
            push_unique(&mut deny, (*rule).to_string());
        }
    }
    if let Some(card) = ctx.card {
        for rule in &card.deny {
            push_unique(&mut deny, rule.clone());
        }
    }
    if permission.unanswered() {
        push_unique(&mut deny, ASK_USER_QUESTION.to_string());
    }
    let allow = match (ctx.card, permission) {
        (Some(card), Permission::Prompt | Permission::Allowlist) => card.allow.clone(),
        _ => Vec::new(),
    };
    SessionRules {
        mode: permission.mode(),
        allow,
        deny,
    }
}

/// The guard's PreToolUse hook entry: `<exe> _guard --session <id>`, in
/// exec form on Windows (no shell between Claude and the guard), shell
/// form elsewhere.
pub fn guard_hook(exe: &str, session: &str) -> Value {
    #[cfg(windows)]
    {
        json!({ "type": "command", "command": exe, "args": ["_guard", "--session", session] })
    }
    #[cfg(not(windows))]
    {
        let q = crate::shell::single_quote;
        json!({ "type": "command", "command": format!("{} _guard --session {}", q(exe), q(session)) })
    }
}

/// The session settings file: Kete's statusLine (when on), the rules, the
/// guard on every Bash and PowerShell call, and the repo and level the
/// guard falls back to.
pub fn settings_json(
    rules: &SessionRules,
    guard: Value,
    status_line: Option<Value>,
    studio: Option<(&Path, Autonomy)>,
) -> Value {
    let mut root = serde_json::Map::new();
    if let Some(line) = status_line {
        root.insert("statusLine".into(), line);
    }
    root.insert(
        "permissions".into(),
        json!({ "allow": rules.allow, "deny": rules.deny }),
    );
    root.insert(
        "hooks".into(),
        json!({ "PreToolUse": [{ "matcher": "Bash|PowerShell", "hooks": [guard] }] }),
    );
    if let Some((repo, level)) = studio {
        root.insert(
            "env".into(),
            json!({
                ENV_STUDIO_REPO: crate::studio::settings::repo_key(repo),
                ENV_STUDIO_AUTONOMY: level.as_str(),
            }),
        );
    }
    Value::Object(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::card::parse;
    use std::path::PathBuf;

    fn force(shell: &str) -> Vec<String> {
        BUILTIN_FORCE_PUSH
            .iter()
            .map(|p| format!("{shell}({p})"))
            .collect()
    }

    struct Dirs {
        wt: PathBuf,
        root: PathBuf,
        data: PathBuf,
        config: PathBuf,
        settings: PathBuf,
    }

    fn dirs() -> Dirs {
        if cfg!(windows) {
            Dirs {
                wt: r"C:\Code\game-runner".into(),
                root: r"C:\Code\game".into(),
                data: r"C:\Users\j\AppData\Local\kete\data".into(),
                config: r"C:\Users\j\AppData\Local\kete\data".into(),
                settings: r"C:\Users\j\AppData\Local\kete\data\sessions\a1\claude-settings.json"
                    .into(),
            }
        } else {
            Dirs {
                wt: "/c/Code/game-runner".into(),
                root: "/c/Code/game".into(),
                data: "/c/Users/j/AppData/Local/kete/data".into(),
                config: "/c/Users/j/AppData/Local/kete/data".into(),
                settings: "/c/Users/j/AppData/Local/kete/data/sessions/a1/claude-settings.json"
                    .into(),
            }
        }
    }

    const FILE: &str = r#"
version = 1
governance = ["DECISIONS/**", ".kete/**"]
[roles.p]
allow = ["Bash(npm test)"]
deny = ["WebFetch"]
[roles.a]
allow = ["Bash(npm test)"]
deny = ["WebFetch"]
permission = "allowlist"
[roles.u]
allow = ["Bash(npm test)"]
deny = ["WebFetch"]
permission = "unattended"
"#;

    fn ctx<'a>(
        d: &'a Dirs,
        card: Option<&'a RoleCard>,
        gov: &'a [String],
        autonomy: Option<Autonomy>,
    ) -> ComposeCtx<'a> {
        ComposeCtx {
            card,
            governance: gov,
            autonomy,
            worktree: &d.wt,
            root_worktree: &d.root,
            data_dir: &d.data,
            config_dir: &d.config,
            settings_file: &d.settings,
        }
    }

    /// The deny list every profile starts with, in order, for [`dirs`].
    fn base_denies() -> Vec<String> {
        let mut want = force("Bash");
        want.extend(force("PowerShell"));
        want.extend(
            [
                "Edit(//c/Users/j/AppData/Local/kete/data/**)",
                "Edit(//c/Code/game-runner/.claude/**)",
                "Edit(//c/Code/game-runner/.kete/**)",
                "Edit(//c/Code/game/.claude/**)",
                "Edit(//c/Code/game/.kete/**)",
                "Edit(//c/Users/j/AppData/Local/kete/data/sessions/a1/claude-settings.json)",
                "Edit(//c/Code/game-runner/DECISIONS/**)",
                "Edit(//c/Code/game/DECISIONS/**)",
            ]
            .map(String::from),
        );
        want
    }

    #[test]
    fn windows_paths_become_posix_rules() {
        if cfg!(windows) {
            assert_eq!(
                posix_rule_path(Path::new(r"C:\Code\wheako\DECISIONS\**")),
                "//c/Code/wheako/DECISIONS/**"
            );
            assert_eq!(posix_rule_path(Path::new(r"\\?\D:\x\")), "//d/x");
        } else {
            assert_eq!(posix_rule_path(Path::new("/home/j/x/")), "//home/j/x");
        }
        // The string form works on every platform.
        assert_eq!(
            posix_rule_path(Path::new("C:/Code/wheako/DECISIONS/**")),
            "//c/Code/wheako/DECISIONS/**"
        );
    }

    #[test]
    fn each_profile_gets_its_exact_mode_and_lists() {
        let d = dirs();
        let file = parse(FILE).unwrap();
        let gov = file.governance.clone();
        // `.kete/**` in governance repeats a built-in rule: written once.
        let mut deny = base_denies();
        deny.push("WebFetch".into());

        let p = compose(&ctx(&d, Some(&file.roles["p"]), &gov, None));
        assert_eq!(p.mode, None);
        assert_eq!(p.allow, ["Bash(npm test)"]);
        assert_eq!(p.deny, deny, "prompt: no AskUserQuestion");

        let mut ask = deny.clone();
        ask.push("AskUserQuestion".into());
        let a = compose(&ctx(&d, Some(&file.roles["a"]), &gov, None));
        assert_eq!(a.mode, Some("dontAsk"));
        assert_eq!(a.allow, ["Bash(npm test)"]);
        assert_eq!(a.deny, ask);

        let u = compose(&ctx(&d, Some(&file.roles["u"]), &gov, None));
        assert_eq!(u.mode, Some("bypassPermissions"));
        assert!(u.allow.is_empty(), "allow rules do nothing under bypass");
        assert_eq!(u.deny, ask);
    }

    #[test]
    fn autonomy_denies_sit_between_governance_and_the_role() {
        let d = dirs();
        let file = parse(FILE).unwrap();
        for level in [Autonomy::A1, Autonomy::A2, Autonomy::A3] {
            let rules = compose(&ctx(
                &d,
                Some(&file.roles["a"]),
                &file.governance,
                Some(level),
            ));
            let mut want = base_denies();
            want.extend(level.denies().iter().map(|r| r.to_string()));
            want.push("WebFetch".into());
            want.push("AskUserQuestion".into());
            assert_eq!(rules.deny, want, "{level}");
        }
        let a1 = compose(&ctx(&d, None, &[], Some(Autonomy::A1)));
        assert!(a1.deny.contains(&"Bash(git push *)".to_string()));
        assert!(a1.deny.contains(&"PowerShell(bro ado pr *)".to_string()));
    }

    #[test]
    fn an_orchestrator_spawn_without_a_role_gets_only_the_fixed_denies() {
        let d = dirs();
        let rules = compose(&ctx(&d, None, &[], None));
        assert_eq!(rules.mode, None);
        assert!(rules.allow.is_empty());
        let mut want = base_denies();
        want.truncate(want.len() - 2); // no governance without a roles file
        assert_eq!(rules.deny, want);
        assert!(!rules.deny.contains(&"AskUserQuestion".to_string()));
    }

    #[test]
    fn the_settings_file_holds_rules_hook_and_studio() {
        let rules = SessionRules {
            mode: Some("dontAsk"),
            allow: vec!["Read".into()],
            deny: vec!["WebFetch".into()],
        };
        let line = json!({ "type": "command", "command": "kete _statusline", "padding": 0 });
        let v = settings_json(
            &rules,
            guard_hook("/bin/kete", "a1"),
            Some(line.clone()),
            Some((Path::new("C:\\Code\\game"), Autonomy::A1)),
        );
        assert_eq!(v["statusLine"], line);
        assert_eq!(v["permissions"]["allow"], json!(["Read"]));
        assert_eq!(v["permissions"]["deny"], json!(["WebFetch"]));
        let pre = &v["hooks"]["PreToolUse"][0];
        assert_eq!(pre["matcher"], "Bash|PowerShell");
        if cfg!(windows) {
            assert_eq!(pre["hooks"][0]["command"], "/bin/kete");
            assert_eq!(
                pre["hooks"][0]["args"],
                json!(["_guard", "--session", "a1"])
            );
        } else {
            assert_eq!(
                pre["hooks"][0]["command"],
                "'/bin/kete' _guard --session 'a1'"
            );
        }
        assert_eq!(v["env"][ENV_STUDIO_REPO], "C:/Code/game");
        assert_eq!(v["env"][ENV_STUDIO_AUTONOMY], "A1");
        let bare = settings_json(&rules, guard_hook("k", "a"), None, None);
        assert!(bare.get("statusLine").is_none() && bare.get("env").is_none());
    }
}
