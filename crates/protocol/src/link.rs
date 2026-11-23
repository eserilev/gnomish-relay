//! The clock check and the ack of the link to a server (SPEC.md 11.7.3 and 11.7.5).

/// More skew lets an old slot body look new to the game.
pub const MAX_SKEW_SECONDS: u32 = 60;

#[must_use]
pub fn clocks_agree(a: u32, b: u32) -> bool {
    if a >= b {
        a - b <= MAX_SKEW_SECONDS
    } else {
        b - a <= MAX_SKEW_SECONDS
    }
}

/// The outbox holds its numbers oldest first. The ack covers the front of it.
#[must_use]
pub fn acked_count(outbox: &[u32], ack: u32) -> usize {
    let mut i = 0;
    while i < outbox.len() && outbox[i] <= ack {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_a_minute_apart_agree_and_a_second_more_does_not() {
        assert!(clocks_agree(1000, 1060));
        assert!(clocks_agree(1060, 1000));
        assert!(!clocks_agree(1000, 1061));
        assert!(!clocks_agree(1061, 1000));
    }

    #[test]
    fn clocks_at_the_ends_of_the_range_do_not_overflow() {
        assert!(!clocks_agree(0, u32::MAX));
        assert!(clocks_agree(u32::MAX, u32::MAX - 60));
    }

    #[test]
    fn an_ack_drops_the_inputs_up_to_its_number() {
        assert_eq!(acked_count(&[3, 4, 5, 6], 4), 2);
    }

    #[test]
    fn an_ack_below_the_oldest_input_drops_nothing() {
        assert_eq!(acked_count(&[3, 4], 2), 0);
    }

    #[test]
    fn an_ack_of_the_newest_input_drops_the_whole_outbox() {
        assert_eq!(acked_count(&[3, 4], 9), 2);
        assert_eq!(acked_count(&[], 9), 0);
    }
}
