//! Work that starts from a session but runs as a session of its own: a
//! follow-up started as a new session, and ASK ASIDE, a side question
//! answered beside the session it is about. Both apps send either as
//! `ClientRequest::SpawnSiblingAgent` from the session, so the new one runs
//! in the same checkout on the same harness and nests under it.

/// The first prompt of an ASK ASIDE session: the question, framed so the
/// agent answers it and leaves the checkout as it found it. `session` is
/// the name of the session the question is about.
///
/// A request, not a guard: the session runs with its caller's permission
/// mode (`SpawnSiblingAgent` carries none), so the apps say it is asked not
/// to change files rather than that it cannot.
pub fn aside_prompt(session: &str, question: &str) -> String {
    let session = session.trim();
    let question = question.trim();
    format!(
        "Side question about the session \"{session}\", which is working in this checkout. \
         Answer it from what you can read here without changing any files, running \
         anything that writes, or committing. Keep the answer short.\n\n{question}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_aside_names_its_session_and_ends_with_the_question() {
        let p = aside_prompt(" fix login ", "  why is the token refreshed twice?\n");
        assert!(p.starts_with("Side question about the session \"fix login\""));
        assert!(p.contains("without changing any files"));
        assert!(p.ends_with("\n\nwhy is the token refreshed twice?"));
    }
}
