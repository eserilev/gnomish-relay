//! Permissions. The game can lower the level of an agent, never raise it.
//! See `SPEC.md` 6.2 and 9.3.

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub enum Level {
    Ask,
    AutoEdit,
    FullAuto,
}

#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
pub enum Answer {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

fn rank(level: Level) -> u8 {
    match level {
        Level::Ask => 0,
        Level::AutoEdit => 1,
        Level::FullAuto => 2,
    }
}

/// The lower of the two levels.
#[must_use]
pub fn effective_level(config: Level, requested: Level) -> Level {
    if rank(requested) < rank(config) {
        requested
    } else {
        config
    }
}

/// An "allow always" from the game counts only for this session.
#[must_use]
pub fn answer_from_game(answer: Answer) -> Answer {
    match answer {
        Answer::AllowAlways => Answer::AllowOnce,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_cannot_raise_the_level() {
        assert_eq!(effective_level(Level::Ask, Level::FullAuto), Level::Ask);
    }

    #[test]
    fn the_game_can_lower_the_level() {
        assert_eq!(
            effective_level(Level::FullAuto, Level::AutoEdit),
            Level::AutoEdit
        );
    }

    #[test]
    fn allow_always_from_the_game_becomes_allow_once() {
        assert_eq!(answer_from_game(Answer::AllowAlways), Answer::AllowOnce);
    }

    #[test]
    fn reject_always_passes_unchanged() {
        assert_eq!(answer_from_game(Answer::RejectAlways), Answer::RejectAlways);
    }
}
