//! What the holder has been told by the shell integration so far.

use crate::principal::{Creds, Principal, Terminal, Verdict, judge, judge_terminal};
use crate::protocol::{self, Malformed, Message, SealRefusal};
use lumen_sealed::types::{self, KIND_COMMAND_END, KIND_COMMAND_START, KIND_PROMPT};
use lumen_sealed::{Direction, DirectionKeys, LayerId, LayerRoot, Sender};
use zeroize::Zeroizing;

/// The frame types the holder seals for the shell: command structure. Grants
/// come from the terminal's policy and not from the shell, a marker needs the
/// `side-channel` capability that nothing grants yet, a layout is from a
/// multiplexer layer, and the types with text do not fit a seal-channel
/// message. All of those are refused as [`SealRefusal::KindNotAllowed`].
const SEALABLE: [u8; 3] = [KIND_COMMAND_START, KIND_PROMPT, KIND_COMMAND_END];

/// The first sequence number the holder uses. Sequence numbers only go up for
/// the layer's life, and the holder has one key for that life.
const FIRST_SEQ: u64 = 1;

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
    /// A frame was sealed. The answer is waiting in [`Layer::take_reply`].
    Sealed,
    /// A seal request was refused. The answer is waiting in [`Layer::take_reply`].
    SealRefused(SealRefusal),
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
    /// Frames sealed for the shell.
    pub sealed: u32,
    /// Seal requests refused for any reason.
    pub seal_refused: u32,
    /// Answers that could not be sent because the channel's queue was full.
    pub reply_dropped: u32,
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
    root: Box<Zeroizing<[u8; 32]>>,
    /// The sequence number of the next frame sealed to the terminal.
    next_seq: u64,
}

/// The holder's view of one layer.
pub struct Layer {
    principal: Principal,
    terminal: Terminal,
    ready: bool,
    key: Option<HeldKey>,
    counters: Counters,
    reply: Option<Vec<u8>>,
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
            reply: None,
        }
    }

    /// The answer to the last message, if it has one: a sealed frame or a
    /// refusal, for the caller to send on the seal channel. Taking it clears it.
    pub fn take_reply(&mut self) -> Option<Vec<u8>> {
        self.reply.take()
    }

    /// An answer could not be sent. It is counted; the shell asks again or goes
    /// without, and a request is never retried by the holder.
    pub fn note_reply_dropped(&mut self) {
        self.counters.reply_dropped += 1;
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
            Ok(Message::Seal { number, kind, body }) => self.seal(number, kind, &body),
        }
    }

    /// Answer a seal request from the shell. The checks run in this order and
    /// the first that fails refuses: ready, key, type, payload. A refused
    /// request uses no sequence number.
    fn seal(&mut self, number: u32, kind: u8, body: &[u8]) -> Outcome {
        match self.try_seal(kind, body) {
            Ok(frame) => {
                self.counters.sealed += 1;
                // A sealed frame is at most a message long for the types above.
                match protocol::sealed_answer(number, &frame) {
                    Some(answer) => {
                        self.reply = Some(answer);
                        Outcome::Sealed
                    }
                    None => self.refuse_seal(number, SealRefusal::Failed),
                }
            }
            Err(why) => self.refuse_seal(number, why),
        }
    }

    fn refuse_seal(&mut self, number: u32, why: SealRefusal) -> Outcome {
        self.counters.seal_refused += 1;
        self.reply = Some(protocol::refused_answer(number, why).to_vec());
        Outcome::SealRefused(why)
    }

    fn try_seal(&mut self, kind: u8, body: &[u8]) -> Result<Vec<u8>, SealRefusal> {
        if !self.ready {
            return Err(SealRefusal::NotReady);
        }
        let held = self.key.as_mut().ok_or(SealRefusal::NoKey)?;
        if !SEALABLE.contains(&kind) {
            return Err(SealRefusal::KindNotAllowed);
        }
        // The terminal parses the same payload; a frame it would call malformed
        // is not sealed.
        types::parse(kind, body).map_err(|_| SealRefusal::BadBody)?;
        let capability = types::capability_of(kind).ok_or(SealRefusal::KindNotAllowed)?;
        // The derived keys live only for this call and are zeroized when the
        // sender is dropped; the root stays in its locked allocation.
        let layer = LayerId(held.layer);
        let keys =
            DirectionKeys::derive(&LayerRoot::new(**held.root), layer, Direction::ToTerminal);
        let mut sender = Sender::new(keys, layer, Direction::ToTerminal, held.next_seq);
        let frame = sender
            .seal(kind, capability, body)
            .map_err(|_| SealRefusal::Failed)?;
        held.next_seq = held.next_seq.saturating_add(1);
        Ok(frame)
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
                    next_seq: FIRST_SEQ,
                });
                KeyOutcome::Accepted {
                    layer: message.layer,
                }
            }
        }
    }
}
