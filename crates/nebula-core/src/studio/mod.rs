//! STUDIO MODE (0.60): what a repo that opts in describes about itself, and
//! what this machine allows it.
//!
//! A repo opts in with files under `<repo>/.kete/`, committed: the manifest
//! (`.kete/studio.toml`, [`manifest`]) names its inbox folder, its
//! departments, its verdict folders and the review app its pages link
//! into. What Jay allows (whether the queue runs, the autonomy level, the
//! notes it leaves alone) is machine-local, under `studio` in
//! `config.local.json` ([`settings`]), never exported, and a repo file can
//! never raise it.
//!
//! Every reader here works on the repo's own files and keeps no copy: the
//! inbox notes ([`inbox`]), the verdict files ([`verdicts`]), and the log
//! of refused launches ([`refusals`]). [`wire`] holds the structs the 0.60
//! protocol carries; [`autonomy`] the levels and what each one denies.
//!
//! The lanes of the 0.60 train own the rest: [`queue`] (the idle-note
//! runner's rules), [`board`] (the studio board), [`gauntlet`] (the
//! gauntlet view) and [`inbox_write`] (the triage desk's note writer).

pub mod autonomy;
pub mod board;
pub mod gauntlet;
pub mod inbox;
pub mod inbox_write;
pub mod manifest;
pub mod queue;
pub mod refusals;
pub mod settings;
pub mod verdicts;
pub mod wire;

pub use autonomy::Autonomy;
pub use settings::StudioSettings;
