//! What the holder has been told by the shell integration so far.

use crate::principal::{Creds, Principal, Terminal, Verdict, judge, judge_terminal};
use crate::protocol::{self, Malformed, Message};
use zeroize::Zeroizing;

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
    /// Keys accepted from the terminal (at most one).
    pub key: u32,
    /// A second key refused.
    pub second_key: u32,
    /// Messages on the spawn socketpair from a sender that is not the terminal.
    pub refused_spawn: u32,
    /// Malformed messages from the terminal.
    pub malformed_spawn: u32,
    /// Whether the key's memory was locked against swap (1) or not (0).
    pub locked: u32,
}

/// What happened to one message on the spawn socketpair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The layer's key, accepted for this layer id.
    Accepted {
        /// The layer the key is for.
        layer: u32,
    },
    /// A second key: refused. There is one key for a layer's whole life.
    Second,
    /// The sender was not the terminal.
    Refused(Verdict),
    /// From the terminal but not a message this holder understands.
    Malformed(Malformed),
}

/// The layer's key, held in its own locked allocation and zeroized on drop.
struct HeldKey {
    layer: u32,
    #[allow(
        dead_code,
        reason = "held for its drop; sealing comes in a later piece"
    )]
    root: Box<Zeroizing<[u8; 32]>>,
}

/// The holder's view of one layer.
pub struct Layer {
    principal: Principal,
    terminal: Terminal,
    ready: bool,
    key: Option<HeldKey>,
    counters: Counters,
}

impl std::fmt::Debug for Layer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layer")
            .field("ready", &self.ready)
            .field("has_key", &self.key.is_some())
            .field("counters", &self.counters)
            .finish_non_exhaustive()
    }
}

impl Layer {
    /// A layer for `principal`, with its key to come from `terminal`, not yet
    /// ready and with no key.
    #[must_use]
    pub fn new(principal: Principal, terminal: Terminal) -> Self {
        Self {
            principal,
            terminal,
            ready: false,
            key: None,
            counters: Counters::default(),
        }
    }

    /// Whether the shell has said it is ready. No sealing before it.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Whether the terminal's key has arrived. Sealing needs this and `ready`.
    #[must_use]
    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    /// The layer id the key was delivered for.
    #[must_use]
    pub fn key_layer(&self) -> Option<u32> {
        self.key.as_ref().map(|k| k.layer)
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

    /// Handle one datagram on the spawn socketpair. The sender is judged first:
    /// only the terminal may deliver the key, and only once. `lock` pins the
    /// key's memory against swap and says whether it could.
    pub fn handle_spawn(
        &mut self,
        creds: Option<Creds>,
        bytes: &[u8],
        truncated: bool,
        lock: &dyn Fn(&[u8]) -> bool,
    ) -> KeyOutcome {
        let verdict = judge_terminal(&self.terminal, creds);
        if verdict != Verdict::Accept {
            self.counters.refused_spawn += 1;
            return KeyOutcome::Refused(verdict);
        }
        match protocol::parse_key(bytes, truncated) {
            Err(why) => {
                self.counters.malformed_spawn += 1;
                KeyOutcome::Malformed(why)
            }
            Ok(_) if self.key.is_some() => {
                self.counters.second_key += 1;
                KeyOutcome::Second
            }
            Ok(message) => {
                let root = Box::new(Zeroizing::new(*message.root));
                self.counters.locked = u32::from(lock(&root[..]));
                self.counters.key += 1;
                self.key = Some(HeldKey {
                    layer: message.layer,
                    root,
                });
                KeyOutcome::Accepted {
                    layer: message.layer,
                }
            }
        }
    }
}
