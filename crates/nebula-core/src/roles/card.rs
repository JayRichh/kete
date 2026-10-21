//! `.kete/roles.toml`: the role cards a repo commits.
//!
//! ```toml
//! version = 1
//! governance = ["docs/decisions/**", ".kete/**"]
//!
//! [roles.runner]
//! label = "Runner"
//! model = "haiku"
//! effort = "low"
//! tier = 3
//! prompt_file = ".kete/roles/runner.md"
//! tools = ["Read", "Write", "Bash"]
//! allow = ["Bash(uv run tools/check.py*)"]
//! mcp = "none"
//! permission = "allowlist"
//! ```
//!
//! Unknown keys are refused, not ignored: a misspelt `permission` must
//! never start a session with fewer restrictions than its author wrote.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The roles file's path, relative to the repo root.
pub const ROLES_PATH: &str = ".kete/roles.toml";
/// A roles file over this many bytes is refused before it is parsed.
pub const MAX_ROLES_BYTES: usize = 65_536;
/// A role's prompt (inline or from `prompt_file`) over this is refused.
pub const MAX_PROMPT_BYTES: usize = 8_192;
/// The most roles one file may define.
pub const MAX_ROLES: usize = 64;
/// The highest tier.
pub const MAX_TIER: u8 = 3;
/// The tier a card without one gets.
pub const DEFAULT_TIER: u8 = 2;
/// Claude's `--effort` levels (`claude --help`, 2.1.306).
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// How a role session's permission prompts are handled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Claude's own starting mode; the role's rules on top.
    #[default]
    Prompt,
    /// `dontAsk`: only the role's allow rules run, everything else is denied.
    Allowlist,
    /// `bypassPermissions`: no prompts; deny rules and the guard still hold.
    Unattended,
}

impl Permission {
    /// `prompt`, `allowlist` or `unattended`.
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::Prompt => "prompt",
            Permission::Allowlist => "allowlist",
            Permission::Unattended => "unattended",
        }
    }

    /// The `--permission-mode` the profile launches with.
    pub fn mode(self) -> Option<&'static str> {
        match self {
            Permission::Prompt => None,
            Permission::Allowlist => Some("dontAsk"),
            Permission::Unattended => Some("bypassPermissions"),
        }
    }

    /// Whether the profile runs with nobody to answer a prompt.
    pub fn unanswered(self) -> bool {
        self != Permission::Prompt
    }
}

/// Which MCP servers a role session gets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mcp {
    /// Whatever Claude would load.
    #[default]
    Default,
    /// None (`--strict-mcp-config` with no `--mcp-config`).
    None,
}

/// One role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoleCard {
    /// Its key under `[roles]`.
    pub id: String,
    /// The chip text; the id when the file names none.
    pub label: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// A department slug, for the board.
    pub department: Option<String>,
    /// 0 to 3.
    pub tier: u8,
    /// Tokens.
    pub context_budget: Option<u64>,
    /// The appended prompt. From `prompt_file` once [`super::load_committed`]
    /// has read it.
    pub prompt: Option<String>,
    /// Repo-relative; read from the same commit as the roles file.
    pub prompt_file: Option<String>,
    /// The whole built-in tool set; None keeps Claude's default.
    pub tools: Option<Vec<String>>,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub mcp: Mcp,
    pub permission: Permission,
}

/// A parsed roles file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RolesFile {
    pub version: u32,
    /// Repo-relative paths no allowlist or unattended session may edit.
    pub governance: Vec<String>,
    pub roles: BTreeMap<String, RoleCard>,
}

/// Why a roles file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleError(pub String);

impl std::fmt::Display for RoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RoleError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    version: u32,
    #[serde(default)]
    governance: Vec<String>,
    #[serde(default)]
    roles: BTreeMap<String, RawCard>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCard {
    label: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    department: Option<String>,
    tier: Option<u8>,
    context_budget: Option<u64>,
    prompt: Option<String>,
    prompt_file: Option<String>,
    tools: Option<Vec<String>>,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
    #[serde(default)]
    mcp: Mcp,
    #[serde(default)]
    permission: Permission,
}

fn refuse(why: impl std::fmt::Display) -> RoleError {
    RoleError(format!("{ROLES_PATH}: {why}"))
}

/// Parses roles-file text, refusing anything outside the limits.
pub fn parse(text: &str) -> Result<RolesFile, RoleError> {
    if text.len() > MAX_ROLES_BYTES {
        return Err(refuse("over 64 KiB"));
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // toml's message names the line and column.
    let raw: RawFile = toml::from_str(text).map_err(|e| refuse(e.to_string().trim_end()))?;
    if raw.version != 1 {
        return Err(refuse(format!(
            "version {}, but only version 1 is understood",
            raw.version
        )));
    }
    if raw.roles.len() > MAX_ROLES {
        return Err(refuse(format!(
            "{} roles, at most {MAX_ROLES}",
            raw.roles.len()
        )));
    }
    for path in &raw.governance {
        check_relative(path).map_err(|why| refuse(format!("governance `{path}` {why}")))?;
    }
    let mut roles = BTreeMap::new();
    for (id, card) in raw.roles {
        let card = card_from(&id, card).map_err(|why| refuse(format!("role `{id}`: {why}")))?;
        roles.insert(id, card);
    }
    Ok(RolesFile {
        version: raw.version,
        governance: raw.governance,
        roles,
    })
}

fn card_from(id: &str, raw: RawCard) -> Result<RoleCard, String> {
    let id_ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !id_ok {
        return Err("an id is letters, digits, -, _ and . (at most 64)".into());
    }
    let tier = raw.tier.unwrap_or(DEFAULT_TIER);
    if tier > MAX_TIER {
        return Err(format!("tier {tier}, but tiers run 0 to {MAX_TIER}"));
    }
    if let Some(effort) = &raw.effort {
        if !EFFORTS.contains(&effort.as_str()) {
            return Err(format!(
                "effort `{effort}`, but it is one of {}",
                EFFORTS.join(", ")
            ));
        }
    }
    if let Some(model) = &raw.model {
        if model.trim().is_empty() || model.chars().any(char::is_whitespace) {
            return Err(format!("model `{model}` is not a model id"));
        }
    }
    if raw.prompt.is_some() && raw.prompt_file.is_some() {
        return Err("names both prompt and prompt_file: keep one".into());
    }
    if let Some(prompt) = &raw.prompt {
        check_prompt(prompt)?;
    }
    if let Some(file) = &raw.prompt_file {
        check_relative(file).map_err(|why| format!("prompt_file `{file}` {why}"))?;
    }
    if let Some(tools) = &raw.tools {
        if tools.is_empty() {
            return Err("tools = [] leaves it no tools: list some, or leave tools out".into());
        }
        for tool in tools {
            let ok = !tool.is_empty()
                && tool
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if !ok {
                return Err(format!("tool `{tool}` is not a tool name"));
            }
        }
    }
    for rule in raw.allow.iter().chain(&raw.deny) {
        if rule.trim().is_empty() || rule.contains('\n') {
            return Err(format!("rule `{rule}` is empty or spans lines"));
        }
    }
    let label = raw
        .label
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| id.to_string());
    Ok(RoleCard {
        id: id.to_string(),
        label,
        model: raw.model,
        effort: raw.effort,
        department: raw.department.filter(|d| !d.trim().is_empty()),
        tier,
        context_budget: raw.context_budget,
        prompt: raw.prompt,
        prompt_file: raw.prompt_file,
        tools: raw.tools,
        allow: raw.allow,
        deny: raw.deny,
        mcp: raw.mcp,
        permission: raw.permission,
    })
}

/// A role prompt within [`MAX_PROMPT_BYTES`].
pub(crate) fn check_prompt(prompt: &str) -> Result<(), String> {
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err(format!(
            "prompt is {} bytes, at most {MAX_PROMPT_BYTES}",
            prompt.len()
        ));
    }
    Ok(())
}

/// A repo-relative path: not absolute, no drive, no `..`.
fn check_relative(path: &str) -> Result<(), &'static str> {
    let p = path.trim();
    let absolute = p.starts_with('/')
        || p.starts_with('\\')
        || p.starts_with('~')
        || p.as_bytes().get(1) == Some(&b':');
    if p.is_empty() || absolute {
        return Err("is not a repo-relative path");
    }
    if p.split(['/', '\\']).any(|part| part == "..") {
        return Err("leaves the repo (`..`)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
version = 1
governance = ["docs/decisions/**", "docs/QUESTIONS.md", ".kete/**"]

[roles.runner]
label = "Runner"
model = "haiku"
effort = "low"
department = "audio"
tier = 3
context_budget = 100000
prompt = "Run the checks."
tools = ["Read", "Write", "Bash"]
allow = ["Bash(uv run tools/check.py*)"]
deny = ["WebFetch", "WebSearch"]
mcp = "none"
permission = "allowlist"

[roles.lead]
prompt_file = ".kete/roles/lead.md"
permission = "unattended"
"#;

    #[test]
    fn every_key_parses() {
        let file = parse(FULL).unwrap();
        assert_eq!(file.version, 1);
        assert_eq!(
            file.governance,
            ["docs/decisions/**", "docs/QUESTIONS.md", ".kete/**"]
        );
        let runner = &file.roles["runner"];
        assert_eq!(
            runner,
            &RoleCard {
                id: "runner".into(),
                label: "Runner".into(),
                model: Some("haiku".into()),
                effort: Some("low".into()),
                department: Some("audio".into()),
                tier: 3,
                context_budget: Some(100_000),
                prompt: Some("Run the checks.".into()),
                prompt_file: None,
                tools: Some(vec!["Read".into(), "Write".into(), "Bash".into()]),
                allow: vec!["Bash(uv run tools/check.py*)".into()],
                deny: vec!["WebFetch".into(), "WebSearch".into()],
                mcp: Mcp::None,
                permission: Permission::Allowlist,
            }
        );
        let lead = &file.roles["lead"];
        assert_eq!(lead.label, "lead", "the label defaults to the id");
        assert_eq!(lead.tier, DEFAULT_TIER);
        assert_eq!(lead.mcp, Mcp::Default);
        assert_eq!(lead.permission, Permission::Unattended);
        assert_eq!(lead.prompt_file.as_deref(), Some(".kete/roles/lead.md"));
        assert_eq!(lead.tools, None);
        let bare = parse("version = 1\n[roles.x]\n").unwrap();
        assert_eq!(bare.roles["x"].permission, Permission::Prompt);
        assert!(bare.governance.is_empty());
    }

    #[test]
    fn profiles_map_to_their_modes() {
        assert_eq!(Permission::Prompt.mode(), None);
        assert_eq!(Permission::Allowlist.mode(), Some("dontAsk"));
        assert_eq!(Permission::Unattended.mode(), Some("bypassPermissions"));
    }

    fn refused(text: &str) -> String {
        parse(text).unwrap_err().0
    }

    #[test]
    fn out_of_bounds_files_are_refused() {
        assert!(refused("version = 2\n").contains("version 2"));
        assert!(
            refused("[roles.x]\n").contains("version"),
            "version is required"
        );
        let e = refused("version = 1\n[roles.x]\npermission = \"yolo\"\n");
        assert!(e.contains("line 3"), "{e}");
        assert!(refused("version = 1\n[roles.x]\nmcp = \"some\"\n").contains("line 3"));
        let big = "x".repeat(MAX_PROMPT_BYTES + 1);
        assert!(
            refused(&format!("version = 1\n[roles.x]\nprompt = \"{big}\"\n"))
                .contains("8193 bytes")
        );
        let ok = "x".repeat(MAX_PROMPT_BYTES);
        assert!(parse(&format!("version = 1\n[roles.x]\nprompt = \"{ok}\"\n")).is_ok());
        let many: String = (0..65).map(|i| format!("[roles.r{i}]\n")).collect();
        assert!(refused(&format!("version = 1\n{many}")).contains("65 roles"));
        let sixty_four: String = (0..64).map(|i| format!("[roles.r{i}]\n")).collect();
        assert!(parse(&format!("version = 1\n{sixty_four}")).is_ok());
        assert!(refused("version = 1\n[roles.x]\ntier = 4\n").contains("tier 4"));
        assert!(
            refused(&format!("version = 1\n#{}", "x".repeat(MAX_ROLES_BYTES))).contains("64 KiB")
        );
    }

    #[test]
    fn mistakes_are_refused_not_ignored() {
        assert!(
            refused("version = 1\n[roles.x]\npermision = \"allowlist\"\n").contains("permision")
        );
        assert!(refused("version = 1\n[roles.x]\neffort = \"huge\"\n").contains("effort"));
        assert!(refused("version = 1\n[roles.x]\ntools = []\n").contains("no tools"));
        assert!(refused("version = 1\n[roles.x]\ntools = [\"Read,Bash\"]\n").contains("tool"));
        assert!(
            refused("version = 1\n[roles.x]\nprompt = \"a\"\nprompt_file = \"b\"\n")
                .contains("both")
        );
        assert!(refused("version = 1\n[roles.x]\nprompt_file = \"../x.md\"\n").contains(".."));
        assert!(refused("version = 1\ngovernance = [\"C:/x/**\"]\n").contains("repo-relative"));
        assert!(refused("version = 1\ngovernance = [\"/etc/**\"]\n").contains("repo-relative"));
        assert!(refused("version = 1\n[roles.\"a b\"]\n").contains("an id"));
        assert!(refused("version = 1\n[roles.x]\nmodel = \"a b\"\n").contains("model"));
        assert!(refused("version = 1\ncolour = 1\n").contains("colour"));
    }
}
