//! What the holder has been told by the shell integration so far.

use crate::principal::{Creds, Principal, Verdict, judge};
use crate::protocol::{self, Malformed, Message};

/// What happened to one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The first `ready` from the shell: sealing may begin.
    Ready,
    /// A second `ready`: refused.
    SecondReady,
    /// The sender was not the shell: refused, and to be shown.
    Refused(Verdict),
    /// From the shell but not a message this holder understands: dropped.
    Malformed(Malformed),
}

/// How many of each, for the terminal and for tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// `ready` accepted.
    pub ready: u32,
    /// A second `ready` refused.
    pub second_ready: u32,
    /// Messages from a sender that is not the shell.
    pub refused: u32,
    /// Malformed messages from the shell.
    pub malformed: u32,
}

/// The holder's view of one layer.
#[derive(Debug)]
pub struct Layer {
    principal: Principal,
    ready: bool,
    counters: Counters,
}

impl Layer {
    /// A layer for `principal`, not yet ready.
    #[must_use]
    pub fn new(principal: Principal) -> Self {
        Self {
            principal,
            ready: false,
            counters: Counters::default(),
        }
    }

    /// Whether the shell has said it is ready. No sealing before it.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// What has been counted.
    #[must_use]
    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// Handle one datagram with the credentials the kernel attached to it. The
    /// sender is judged first: nothing is read from a message that did not come
    /// from the shell.
    pub fn handle(
        &mut self,
        creds: Option<Creds>,
        bytes: &[u8],
        truncated: bool,
        start_time_of: &dyn Fn(u32) -> Option<u64>,
    ) -> Outcome {
        let verdict = judge(&self.principal, creds, start_time_of);
        if verdict != Verdict::Accept {
            self.counters.refused += 1;
            return Outcome::Refused(verdict);
        }
        match protocol::parse(bytes, truncated) {
            Err(why) => {
                self.counters.malformed += 1;
                Outcome::Malformed(why)
            }
            Ok(Message::Ready { .. }) if self.ready => {
                self.counters.second_ready += 1;
                Outcome::SecondReady
            }
            Ok(Message::Ready { .. }) => {
                self.ready = true;
                self.counters.ready += 1;
                Outcome::Ready
            }
        }
    }
}
