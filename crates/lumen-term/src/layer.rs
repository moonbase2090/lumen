//! The arm layer as the terminal sees it: bytes in, pieces out.

use lumen_track0::Block as Track0Block;

/// A chrome notice the layer asks the terminal to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// The unauthenticated producer's budget for this command is spent.
    Degraded,
    /// The layer is pinned to grid mode after too many sealed aborts.
    Pinned,
}

/// What the terminal knows about the layer when it hands over a read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    /// The alternate screen is active on the layer.
    pub alt_screen: bool,
    /// `TMUX` or `STY` is set in the layer's environment.
    pub tmux_or_screen: bool,
    /// Monotonic time of the read, in milliseconds.
    pub now_ms: u64,
}

/// One thing the layer decided about a piece of the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Ordinary bytes for the VT parser.
    Bytes(Vec<u8>),
    /// A Track 0 block to show in the unverified-output container.
    Rich(Track0Block),
    /// A Track 0 block for the collapsed background strip, which only Arm B has.
    Background(Track0Block),
    /// Visible escaped text, in place. It holds no control character.
    Text(String),
    /// A chrome notice.
    Notice(Notice),
    /// A sealed frame that verified and opened, with its payload length.
    Sealed {
        /// The sealed frame type.
        kind: u8,
        /// Length of the decrypted payload.
        len: usize,
    },
}

/// What stands between a PTY's bytes and the grid.
pub trait Layer {
    /// Take the bytes of one read, with what the terminal knows about the layer.
    fn feed(&mut self, bytes: &[u8], ctx: Context) -> Vec<Piece>;
    /// The stream ended: release what is held back.
    fn finish(&mut self) -> Vec<Piece>;
    /// The terminal opened the container for a new command.
    fn open_container(&mut self);
    /// The terminal refused an authenticated sealed frame (a payload it could
    /// not use): count it toward the pin.
    fn sealed_abort(&mut self, now_ms: u64) -> Vec<Piece>;
}

/// No arm: every byte goes to the grid.
#[derive(Debug, Default, Clone, Copy)]
pub struct Passthrough;

impl Layer for Passthrough {
    fn feed(&mut self, bytes: &[u8], _ctx: Context) -> Vec<Piece> {
        if bytes.is_empty() {
            Vec::new()
        } else {
            vec![Piece::Bytes(bytes.to_vec())]
        }
    }

    fn finish(&mut self) -> Vec<Piece> {
        Vec::new()
    }

    fn open_container(&mut self) {}

    fn sealed_abort(&mut self, _now_ms: u64) -> Vec<Piece> {
        Vec::new()
    }
}

#[cfg(feature = "arm-a")]
mod arm_a {
    use super::{Context, Layer, Notice, Piece};
    use lumen_arm_a::pipeline as a;

    impl Layer for a::Layer {
        fn feed(&mut self, bytes: &[u8], ctx: Context) -> Vec<Piece> {
            a::Layer::feed(
                self,
                bytes,
                a::Context {
                    alt_screen: ctx.alt_screen,
                    tmux_or_screen: ctx.tmux_or_screen,
                    now_ms: ctx.now_ms,
                },
            )
            .into_iter()
            .map(piece)
            .collect()
        }

        fn finish(&mut self) -> Vec<Piece> {
            a::Layer::finish(self).into_iter().map(piece).collect()
        }

        fn open_container(&mut self) {
            a::Layer::open_container(self);
        }

        fn sealed_abort(&mut self, now_ms: u64) -> Vec<Piece> {
            a::Layer::sealed_abort(self, now_ms)
                .into_iter()
                .map(piece)
                .collect()
        }
    }

    fn piece(item: a::Item) -> Piece {
        match item {
            a::Item::Output(bytes) => Piece::Bytes(bytes),
            a::Item::Block(block) => Piece::Rich(block),
            a::Item::Text(text) => Piece::Text(text),
            a::Item::Degraded => Piece::Notice(Notice::Degraded),
            a::Item::Pinned => Piece::Notice(Notice::Pinned),
            a::Item::Sealed { header, body } => Piece::Sealed {
                kind: header.kind,
                len: body.len(),
            },
        }
    }
}

#[cfg(feature = "arm-b")]
mod arm_b {
    use super::{Context, Layer, Notice, Piece};
    use lumen_arm_b as b;

    impl Layer for b::Layer {
        fn feed(&mut self, bytes: &[u8], ctx: Context) -> Vec<Piece> {
            b::Layer::feed(
                self,
                bytes,
                b::Context {
                    alt_screen: ctx.alt_screen,
                    tmux_or_screen: ctx.tmux_or_screen,
                    now_ms: ctx.now_ms,
                },
            )
            .into_iter()
            .map(piece)
            .collect()
        }

        fn finish(&mut self) -> Vec<Piece> {
            b::Layer::finish(self).into_iter().map(piece).collect()
        }

        fn open_container(&mut self) {
            b::Layer::open_container(self);
        }

        fn sealed_abort(&mut self, now_ms: u64) -> Vec<Piece> {
            b::Layer::sealed_abort(self, now_ms)
                .into_iter()
                .map(piece)
                .collect()
        }
    }

    fn piece(item: b::Item) -> Piece {
        match item {
            b::Item::Output(bytes) => Piece::Bytes(bytes),
            b::Item::Block(block) => Piece::Rich(block),
            b::Item::Background(block) => Piece::Background(block),
            b::Item::Text(text) => Piece::Text(text),
            b::Item::Degraded => Piece::Notice(Notice::Degraded),
            b::Item::Pinned => Piece::Notice(Notice::Pinned),
            b::Item::Sealed { header, body } => Piece::Sealed {
                kind: header.kind,
                len: body.len(),
            },
        }
    }
}
