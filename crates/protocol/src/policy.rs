//! Permissions. The game can lower the level of an agent, never raise it.
//! See `SPEC.md` 6.2 and 9.3.

#[derive(Clone, Copy)]
pub enum Level {
    Ask,
    AutoEdit,
    FullAuto,
}

#[derive(Clone, Copy)]
pub enum Answer {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

#[must_use]
pub fn effective_level(config: Level, requested: Level) -> Level {
    todo!()
}

/// An "allow always" from the game counts only for this session.
#[must_use]
pub fn answer_from_game(answer: Answer) -> Answer {
    todo!()
}
