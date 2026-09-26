//! The model calls of the story program (SPEC.md 9.7, decision 10, and 9.8). Each call
//! runs on its own thread with no tools, and returns only text. The bridge bounds the
//! open calls, and a budget with the proved limiter of S14 bounds the calls in time.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use protocol::rate::{RateLimiter, admit_message};

use crate::agent::StopSignal;
use crate::app_protocol::CallId;
use crate::model_local::LocalModel;
use crate::process::cut;
use crate::{model_claude, model_local};

/// The text of one answer for the story program. Its longest reply text is 8 KiB.
pub const MAX_ANSWER: usize = 16 * 1024;
/// One call of the companion and one of the bard.
pub const MAX_OPEN: usize = 2;

/// Which model answers, from `[story] model` in the config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelChoice {
    /// Every call fails at once.
    None,
    /// `claude -p` with no tools. `command` is `["claude"]`, never the command of a relay
    /// agent (9.7, decisions 4 and 18).
    Claude {
        command: Vec<String>,
        model: Option<String>,
    },
    Local(LocalModel),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelSpec {
    pub choice: ModelChoice,
    /// The longest wait for one answer.
    pub timeout: Duration,
    /// The limiter admits 10 calls in any window of this many minutes.
    pub budget_window_minutes: u32,
}

impl ModelSpec {
    /// No model: every call fails at once.
    pub fn none() -> ModelSpec {
        ModelSpec {
            choice: ModelChoice::None,
            timeout: Duration::from_mins(1),
            budget_window_minutes: 20,
        }
    }
}

/// Why a call failed before it ran.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Refused {
    NoModel,
    TooManyOpen,
    /// The story program sent a call number that is still open.
    SameCall,
    OverBudget,
}

/// The whole work of one call: the prompt in, the text or an error out.
pub type Ask = Arc<dyn Fn(&str, StopSignal) -> Result<String, String> + Send + Sync>;

/// A finished call: its answer text, or why it failed.
pub type Finished = (CallId, Result<String, String>);

pub struct ModelCalls {
    ask: Option<Ask>,
    open: BTreeMap<CallId, StopSignal>,
    budget: Budget,
    done: Sender<Finished>,
    finished: Receiver<Finished>,
}

impl ModelCalls {
    pub fn new(spec: &ModelSpec) -> ModelCalls {
        ModelCalls::with_ask(ask_of(spec), spec.budget_window_minutes)
    }

    /// `None` is no model. Tests give their own `ask`.
    pub fn with_ask(ask: Option<Ask>, budget_window_minutes: u32) -> ModelCalls {
        let (done, finished) = channel();
        ModelCalls {
            ask,
            open: BTreeMap::new(),
            budget: Budget::new(budget_window_minutes),
            done,
            finished,
        }
    }

    /// The budget counts only a call that runs, so it comes last.
    pub fn start(&mut self, call: CallId, prompt: String) -> Result<(), Refused> {
        let Some(ask) = self.ask.clone() else {
            return Err(Refused::NoModel);
        };
        if self.open.contains_key(&call) {
            return Err(Refused::SameCall);
        }
        if self.open.len() >= MAX_OPEN {
            return Err(Refused::TooManyOpen);
        }
        if !self.budget.admit() {
            return Err(Refused::OverBudget);
        }
        let stop = StopSignal::default();
        self.open.insert(call, stop.clone());
        let done = self.done.clone();
        thread::spawn(move || {
            let answer = ask(&prompt, stop).map(|text| clean_answer(&text));
            let _ = done.send((call, answer));
        });
        Ok(())
    }

    /// The calls that ended since the last look.
    pub fn finished(&mut self) -> Vec<Finished> {
        let ended: Vec<Finished> = self.finished.try_iter().collect();
        for (call, _) in &ended {
            self.open.remove(call);
        }
        ended
    }

    pub fn open_calls(&self) -> usize {
        self.open.len()
    }

    /// Ends every open call. A late answer of such a call never comes out, because the
    /// next story program can use the same call numbers.
    pub fn stop_all(&mut self) {
        for stop in std::mem::take(&mut self.open).into_values() {
            stop.request();
        }
        let (done, finished) = channel();
        self.done = done;
        self.finished = finished;
    }
}

impl Drop for ModelCalls {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn ask_of(spec: &ModelSpec) -> Option<Ask> {
    let timeout = spec.timeout;
    match spec.choice.clone() {
        ModelChoice::None => None,
        ModelChoice::Claude { command, model } => Some(Arc::new(move |prompt, stop| {
            model_claude::ask(&command, model.as_deref(), prompt, timeout, stop)
        })),
        ModelChoice::Local(local) => Some(Arc::new(move |prompt, stop| {
            model_local::ask(&local, prompt, timeout, stop)
        })),
    }
}

/// An answer is hostile text. Control characters other than a newline and a tab go,
/// and a long answer is cut.
pub fn clean_answer(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    cut(&clean, MAX_ANSWER).to_owned()
}

/// The proved limiter of S14 admits 10 calls in any 60 steps. One step here lasts
/// `window_minutes` seconds, so 60 steps last `window_minutes` minutes.
struct Budget {
    limiter: RateLimiter,
    started: Instant,
    window_minutes: u32,
}

impl Budget {
    fn new(window_minutes: u32) -> Budget {
        Budget {
            limiter: RateLimiter { times: Vec::new() },
            started: Instant::now(),
            window_minutes: window_minutes.max(1),
        }
    }

    fn admit(&mut self) -> bool {
        self.admit_at(self.started.elapsed())
    }

    /// The time comes from a clock that never goes back, because S14 needs the times in
    /// order.
    fn admit_at(&mut self, since_start: Duration) -> bool {
        let step = since_start.as_secs() / u64::from(self.window_minutes);
        let now = u32::try_from(step).unwrap_or(u32::MAX);
        let (admitted, limiter) = admit_message(&self.limiter, now);
        self.limiter = limiter;
        admitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn answering(text: &'static str) -> Ask {
        Arc::new(move |_, _| Ok(text.to_owned()))
    }

    /// Answers when the test says so, or ends at a stop. `stopped` counts the stops.
    fn waiting(go: Arc<Mutex<bool>>, stopped: Arc<Mutex<u32>>) -> Ask {
        Arc::new(move |_, stop: StopSignal| {
            loop {
                if stop.requested() {
                    *stopped.lock().unwrap() += 1;
                    return Err("Stopped.".into());
                }
                if *go.lock().unwrap() {
                    return Ok("done".into());
                }
                thread::sleep(Duration::from_millis(5));
            }
        })
    }

    fn wait_for(calls: &mut ModelCalls, count: usize) -> Vec<Finished> {
        let start = Instant::now();
        let mut all = Vec::new();
        while all.len() < count && start.elapsed() < Duration::from_secs(10) {
            all.extend(calls.finished());
            thread::sleep(Duration::from_millis(5));
        }
        all
    }

    #[test]
    fn with_no_model_every_call_fails_at_once() {
        let mut calls = ModelCalls::with_ask(None, 20);
        assert_eq!(calls.start(CallId(1), "x".into()), Err(Refused::NoModel));
    }

    #[test]
    fn a_call_answers_by_its_number() {
        let mut calls = ModelCalls::with_ask(Some(answering("A wolf howls.")), 20);

        calls.start(CallId(7), "tell".into()).unwrap();

        let finished = wait_for(&mut calls, 1);
        assert_eq!(finished, [(CallId(7), Ok("A wolf howls.".into()))]);
        assert_eq!(calls.open_calls(), 0);
    }

    #[test]
    fn a_third_open_call_fails_at_once_and_counts_nothing() {
        let go = Arc::new(Mutex::new(false));
        let ask = waiting(Arc::clone(&go), Arc::default());
        let mut calls = ModelCalls::with_ask(Some(ask), 20);
        calls.start(CallId(1), "a".into()).unwrap();
        calls.start(CallId(2), "b".into()).unwrap();

        assert_eq!(
            calls.start(CallId(3), "c".into()),
            Err(Refused::TooManyOpen)
        );
        assert_eq!(calls.start(CallId(2), "b".into()), Err(Refused::SameCall));

        *go.lock().unwrap() = true;
        assert_eq!(wait_for(&mut calls, 2).len(), 2);
        assert_eq!(
            calls.budget.limiter.times.len(),
            2,
            "only the calls that ran"
        );
        calls.start(CallId(3), "c".into()).unwrap();
    }

    #[test]
    fn a_stop_ends_every_open_call_and_drops_its_late_answer() {
        let stopped = Arc::new(Mutex::new(0));
        let ask = waiting(Arc::default(), Arc::clone(&stopped));
        let mut calls = ModelCalls::with_ask(Some(ask), 20);
        calls.start(CallId(1), "a".into()).unwrap();
        calls.start(CallId(2), "b".into()).unwrap();

        calls.stop_all();

        let start = Instant::now();
        while *stopped.lock().unwrap() < 2 {
            assert!(start.elapsed() < Duration::from_secs(10), "never stopped");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(calls.open_calls(), 0);
        thread::sleep(Duration::from_millis(50));
        assert!(calls.finished().is_empty());
    }

    #[test]
    fn dropping_the_calls_stops_them() {
        let stopped = Arc::new(Mutex::new(0));
        let ask = waiting(Arc::default(), Arc::clone(&stopped));
        let mut calls = ModelCalls::with_ask(Some(ask), 20);
        calls.start(CallId(1), "a".into()).unwrap();

        drop(calls);

        let start = Instant::now();
        while *stopped.lock().unwrap() < 1 {
            assert!(start.elapsed() < Duration::from_secs(10), "never stopped");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn over_budget_a_call_fails_at_once() {
        let mut calls = ModelCalls::with_ask(Some(answering("ok")), 20);
        for call in 1..=10 {
            calls.start(CallId(call), "x".into()).unwrap();
            wait_for(&mut calls, 1);
        }

        assert_eq!(
            calls.start(CallId(11), "x".into()),
            Err(Refused::OverBudget)
        );
    }

    #[test]
    fn the_budget_admits_no_eleventh_call_within_the_window_less_one_step() {
        // A window of 20 minutes: one step is 20 seconds.
        let mut budget = Budget::new(20);
        let at = Duration::from_secs;
        for _ in 0..10 {
            assert!(budget.admit_at(at(19)));
        }
        // 19 minutes 21 seconds after the first ten: still 59 steps.
        assert!(!budget.admit_at(at(19 + 19 * 60 + 21)));
        // At step 60 the first ten no longer count.
        assert!(budget.admit_at(at(20 * 60)));
    }

    #[test]
    fn a_hostile_answer_loses_its_control_characters_and_is_cut() {
        assert_eq!(clean_answer("a\u{7}b\r\nc\td\u{1b}[31m"), "ab\nc\td[31m");
        let long = "é".repeat(MAX_ANSWER);
        let cut = clean_answer(&long);
        assert_eq!(cut.len(), MAX_ANSWER);
        assert!(cut.chars().all(|c| c == 'é'));
    }
}
