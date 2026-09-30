//! What one run of an agent used: its tokens, and its cost when the agent gives it
//! (SPEC.md 9.10). No I/O here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Every input token, the cached ones too.
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    /// US dollars. Only Claude Code gives it.
    pub cost_usd: Option<f64>,
}

impl Usage {
    /// The sum of two reports. A cost of either side counts.
    #[must_use]
    pub fn plus(self, other: Usage) -> Usage {
        let cost_usd = match (self.cost_usd, other.cost_usd) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
        Usage {
            input: self.input.saturating_add(other.input),
            cached: self.cached.saturating_add(other.cached),
            output: self.output.saturating_add(other.output),
            cost_usd,
        }
    }

    /// The tokens since `earlier`, for counters that only grow.
    #[must_use]
    pub fn since(self, earlier: Usage) -> Usage {
        Usage {
            input: self.input.saturating_sub(earlier.input),
            cached: self.cached.saturating_sub(earlier.cached),
            output: self.output.saturating_sub(earlier.output),
            cost_usd: None,
        }
    }

    /// For example "1.2k in · 350 out · $0.04".
    pub fn line(&self) -> String {
        let tokens = format!("{} in · {} out", count(self.input), count(self.output));
        match self.cost_usd {
            Some(cost) => format!("{tokens} · {}", dollars(cost)),
            None => tokens,
        }
    }
}

/// A count as the game shows it: "350", "1.2k", "12k", or "1.2M".
pub fn count(n: u64) -> String {
    if n < 1000 {
        return n.to_string();
    }
    if n < 1_000_000 {
        return scaled(n, 1000, "k");
    }
    scaled(n, 1_000_000, "M")
}

/// One decimal below 10 of the unit, none above. The decimal is cut, never rounded up,
/// so 999,999 never shows as "1000k".
fn scaled(n: u64, unit: u64, suffix: &str) -> String {
    let whole = n / unit;
    if whole >= 10 {
        return format!("{whole}{suffix}");
    }
    let tenth = n % unit * 10 / unit;
    format!("{whole}.{tenth}{suffix}")
}

/// "$0.04", or "<$0.01" for a cost above 0 and below one cent.
pub fn dollars(cost: f64) -> String {
    if cost > 0.0 && cost < 0.01 {
        return "<$0.01".into();
    }
    format!("${cost:.2}")
}

/// A count of a report. Anything that is not a whole number of at least 0 counts as 0.
pub fn tokens_at(value: &Value, pointer: &str) -> u64 {
    value.pointer(pointer).and_then(Value::as_u64).unwrap_or(0)
}

/// A cost of a report, only when it is a finite number of at least 0.
pub fn cost_at(value: &Value, pointer: &str) -> Option<f64> {
    let cost = value.pointer(pointer).and_then(Value::as_f64)?;
    (cost.is_finite() && cost >= 0.0).then_some(cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_count_below_a_thousand_shows_as_it_is() {
        assert_eq!(count(0), "0");
        assert_eq!(count(350), "350");
        assert_eq!(count(999), "999");
    }

    #[test]
    fn a_larger_count_shows_in_thousands_or_millions() {
        assert_eq!(count(1000), "1.0k");
        assert_eq!(count(1234), "1.2k");
        assert_eq!(count(9_999), "9.9k");
        assert_eq!(count(12_345), "12k");
        assert_eq!(count(999_999), "999k");
        assert_eq!(count(1_234_567), "1.2M");
        assert_eq!(count(45_000_000), "45M");
    }

    #[test]
    fn a_cost_shows_with_two_decimals_and_a_tiny_cost_shows_as_below_a_cent() {
        assert_eq!(dollars(0.0412), "$0.04");
        assert_eq!(dollars(1.2), "$1.20");
        assert_eq!(dollars(0.0), "$0.00");
        assert_eq!(dollars(0.001), "<$0.01");
    }

    #[test]
    fn the_line_names_the_tokens_and_the_cost_when_there_is_one() {
        let usage = Usage {
            input: 1234,
            cached: 1000,
            output: 350,
            cost_usd: Some(0.0412),
        };
        assert_eq!(usage.line(), "1.2k in · 350 out · $0.04");
        let no_cost = Usage {
            cost_usd: None,
            ..usage
        };
        assert_eq!(no_cost.line(), "1.2k in · 350 out");
    }

    #[test]
    fn two_reports_add_up_and_a_cost_of_one_side_counts() {
        let a = Usage {
            input: 10,
            cached: 1,
            output: 2,
            cost_usd: Some(0.5),
        };
        let b = Usage {
            input: 5,
            cached: 0,
            output: 3,
            cost_usd: None,
        };

        let sum = a.plus(b);

        assert_eq!(
            sum,
            Usage {
                input: 15,
                cached: 1,
                output: 5,
                cost_usd: Some(0.5)
            }
        );
        assert_eq!(b.plus(b).cost_usd, None);
    }

    #[test]
    fn the_tokens_since_an_earlier_count_never_go_below_zero() {
        let later = Usage {
            input: 100,
            cached: 40,
            output: 10,
            cost_usd: None,
        };
        let earlier = Usage {
            input: 60,
            cached: 50,
            output: 4,
            cost_usd: None,
        };

        let since = later.since(earlier);

        assert_eq!((since.input, since.cached, since.output), (40, 0, 6));
    }

    #[test]
    fn a_bad_number_in_a_report_counts_as_zero_or_no_cost() {
        let report = json!({ "a": -3, "b": "7", "c": 2.5, "cost": -1.0, "nan": "x" });

        assert_eq!(tokens_at(&report, "/a"), 0);
        assert_eq!(tokens_at(&report, "/b"), 0);
        assert_eq!(tokens_at(&report, "/c"), 0);
        assert_eq!(tokens_at(&report, "/missing"), 0);
        assert_eq!(cost_at(&report, "/cost"), None);
        assert_eq!(cost_at(&report, "/nan"), None);
        assert_eq!(cost_at(&report, "/c"), Some(2.5));
    }
}
