//! # lacrymira-mock
//!
//! An in-process stand-in for [`PgExecutor`](lacrymira_pg::PgExecutor): tests
//! queue the [`Reply`] a matching [`Command`] should produce, then inspect
//! exactly which Commands the code under test issued.
//!
//! ## Matching by Command shape, never by SQL text
//!
//! The queue is matched against the **typed [`Command`] shape** — kind plus
//! the fields that identify it (table, KV namespace/op) — not against
//! rendered SQL. Text heuristics were tried in the family once and reverted:
//! a substring rule that fixed one suite silently re-routed setup `SELECT`s
//! in another (the shittim-chest migration's G18). Shape matching keeps a
//! `Command::Sql` distinguishable from a `Command::Select` no matter what
//! either one's text looks like.
//!
//! ## Grain of the queue
//!
//! Each queued item is consumed by the FIRST command whose shape matches it;
//! items that never match are left in place and reported by
//! [`MockExecutor::unmatched`], so a test that queues the wrong shape fails
//! loudly at the end instead of passing on a default.
//!
//! ```
//! use lacrymira::{Command, Reply, Value};
//! use lacrymira_mock::MockExecutor;
//!
//! # async fn demo() {
//! let mock = MockExecutor::new();
//! mock.push_reply(
//!     "count users",
//!     |cmd| matches!(cmd, Command::Count { table, .. } if table == "users"),
//!     Reply::Value(Some(Value::I64(7))),
//! );
//!
//! let n = mock.execute(&Command::Count { table: "users".into(), filter: None }).await.unwrap();
//! assert!(matches!(n, Reply::Value(Some(Value::I64(7)))));
//! assert_eq!(mock.statements().len(), 1);
//! # }
//! ```

use std::sync::{Arc, Mutex};

use lacrymira::{Command, Reply};

/// One queued expectation: a shape predicate plus the reply to hand back.
struct Expected {
    /// Predicate over the incoming Command.
    matches: Box<dyn Fn(&Command) -> bool + Send + Sync>,
    /// The reply to produce.
    reply: Reply,
    /// Human-readable label for the unmatched report.
    label: String,
}

#[derive(Default)]
struct State {
    queue: Vec<Expected>,
    log: Vec<Command>,
}

/// A mock executor: queued replies by Command shape + a statement log.
#[derive(Clone, Default)]
pub struct MockExecutor {
    state: Arc<Mutex<State>>,
}

impl MockExecutor {
    /// Create an empty mock.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a reply for the first command matching `predicate`.
    ///
    /// `label` names the expectation in [`unmatched`](Self::unmatched).
    pub fn push_reply(
        &self,
        label: impl Into<String>,
        predicate: impl Fn(&Command) -> bool + Send + Sync + 'static,
        reply: Reply,
    ) {
        self.state.lock().unwrap().queue.push(Expected {
            matches: Box::new(predicate),
            reply,
            label: label.into(),
        });
    }

    /// Queue a reply matching one exact Command (equality on the typed value).
    pub fn push_reply_for(&self, cmd: Command, reply: Reply) {
        let label = format!("{cmd:?}");
        self.push_reply(label, move |c| *c == cmd, reply);
    }

    /// Execute a command against the queue, recording it in the log.
    ///
    /// Consumes the first matching expectation. An unmatched command is
    /// still logged and its reply is [`Reply::Unit`] — the test's
    /// [`unmatched`](Self::unmatched) check is what catches the mistake.
    pub async fn execute(&self, cmd: &Command) -> Result<Reply, lacrymira::Error> {
        let mut state = self.state.lock().unwrap();
        state.log.push(cmd.clone());
        if let Some(pos) = state.queue.iter().position(|e| (e.matches)(cmd)) {
            let expected = state.queue.remove(pos);
            return Ok(expected.reply.clone());
        }
        Ok(Reply::Unit)
    }

    /// The Commands executed so far, in order.
    pub fn statements(&self) -> Vec<Command> {
        self.state.lock().unwrap().log.clone()
    }

    /// How many commands were executed.
    pub fn statement_count(&self) -> usize {
        self.state.lock().unwrap().log.len()
    }

    /// Expectations that never matched, by label.
    pub fn unmatched(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .queue
            .iter()
            .map(|e| e.label.clone())
            .collect()
    }

    /// Panic unless every queued expectation was consumed.
    ///
    /// Tests call this at the end: a leftover expectation means the code
    /// under test issued a different shape than the test assumed.
    pub fn assert_all_consumed(&self) {
        let left = self.unmatched();
        assert!(
            left.is_empty(),
            "the mock still holds {} unconsumed expectation(s) — the code \
             under test issued a different Command shape: {left:#?}",
            left.len()
        );
    }
}
