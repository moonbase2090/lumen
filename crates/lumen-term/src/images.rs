//! The terminal's image path: the frames of an image reassembled in order, the
//! out-of-process decode behind the unlock rule, and the decoded pixels kept for
//! drawing.
//!
//! The rules are the decoder crate's ([`lumen_decoder::images`] and
//! [`lumen_decoder::frames`]); this module gives them a producer, a place to keep
//! pixels and the build's gate. A build that carries no gate record has every
//! image locked: the alt text is shown, no byte of an image is kept and no worker
//! is started.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lumen_decoder::frames::{Data, Placement, Receiver, Start};
use lumen_decoder::images::{
    AltReason, Decode, Gate, ImageId, ImageManager, Limits, Producer, TextureId, TextureSink,
    Trust, UploadFailed,
};
use lumen_decoder::unlock::{GateRecord, Identities};

/// Decoded pixels: 8-bit premultiplied RGBA, top-down, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The pixels.
    pub rgba: Arc<Vec<u8>>,
}

#[derive(Default)]
struct Held {
    next: u64,
    pixels: HashMap<TextureId, Pixels>,
}

/// Where the decoded pixels are kept until the document takes them.
#[derive(Clone, Default)]
pub struct Store(Arc<Mutex<Held>>);

impl TextureSink for Store {
    fn upload(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<TextureId, UploadFailed> {
        let mut held = self.0.lock().map_err(|_| UploadFailed)?;
        held.next += 1;
        let id = TextureId(held.next);
        held.pixels.insert(
            id,
            Pixels {
                width,
                height,
                rgba: Arc::new(rgba.to_vec()),
            },
        );
        Ok(id)
    }

    fn release(&mut self, texture: TextureId) {
        if let Ok(mut held) = self.0.lock() {
            held.pixels.remove(&texture);
        }
    }
}

impl Store {
    fn get(&self, texture: TextureId) -> Option<Pixels> {
        self.0.lock().ok()?.pixels.get(&texture).cloned()
    }
}

/// What the build knows about its two identities and the gate results it was
/// shipped with. The release process sets them at build time
/// (`LUMEN_DECODER_BUILD`, `LUMEN_IMAGE_PATH_BUILD`, `LUMEN_GATE_RECORDS`; the
/// crate's build script takes them from `release/gate-records.txt`); a
/// build without them has identities that never unlock and no records.
#[must_use]
pub fn build_gate(setting_on: bool) -> Gate {
    Gate {
        setting_on,
        platform: platform().to_owned(),
        identities: Identities {
            decoder_build: option_env!("LUMEN_DECODER_BUILD")
                .unwrap_or("absent")
                .to_owned(),
            image_path_build: option_env!("LUMEN_IMAGE_PATH_BUILD")
                .unwrap_or("absent")
                .to_owned(),
        },
        records: parse_records(option_env!("LUMEN_GATE_RECORDS").unwrap_or("")),
    }
}

/// The platform name a gate record is kept under.
#[must_use]
pub const fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        // No gate record is kept under this name: images stay locked.
        "unsupported"
    }
}

/// Gate records as text: one per line (or separated by `;`), `platform
/// decoder_build image_path_build decoder_gate memory_gate` separated by spaces, the two gates as `pass` or
/// anything else for not passed. A line that starts with `#` is a comment. A
/// line that is not of that shape is ignored, which only ever leaves images
/// locked.
#[must_use]
pub fn parse_records(text: &str) -> Vec<GateRecord> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(|line| line.split(';'))
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let [platform, decoder, path, dg, mg] = f.as_slice() else {
                return None;
            };
            Some(GateRecord {
                platform: (*platform).to_owned(),
                decoder_build: (*decoder).to_owned(),
                image_path_build: (*path).to_owned(),
                decoder_gate_passed: *dg == "pass",
                memory_gate_passed: *mg == "pass",
            })
        })
        .collect()
}

/// The decoder worker next to the terminal's executable, if there is one.
#[must_use]
pub fn worker_path() -> std::path::PathBuf {
    let name = if cfg!(windows) {
        "lumen-decoder-worker.exe"
    } else {
        "lumen-decoder-worker"
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| name.into())
}

/// Whether the terminal may be on an SSH session: the variables ssh sets are
/// there. The fail-closed reading, as the manager treats an unknown producer.
#[must_use]
pub fn possibly_ssh() -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        .any(|v| std::env::var_os(v).is_some())
}

/// What happened to an image, for the document to show.
#[derive(Debug, PartialEq, Eq)]
pub enum Shown {
    /// Still waiting: the alt text stands in.
    Waiting,
    /// The alt text is shown, for this reason.
    Alt(AltReason),
    /// The decoded image.
    Image(Pixels),
}

/// A change to show: the image `id` is now `shown`.
#[derive(Debug, PartialEq, Eq)]
pub struct Update {
    /// Which image.
    pub id: ImageId,
    /// What it is now.
    pub shown: Shown,
}

/// See the module documentation.
pub struct ImageHost {
    gate: Gate,
    manager: ImageManager<Store>,
    receiver: Receiver,
    producer: Producer,
    store: Store,
}

impl ImageHost {
    /// A host for one producer with the given gate and decoder.
    #[must_use]
    pub fn new(gate: Gate, decoder: impl Decode, ssh: bool) -> Self {
        let store = Store::default();
        let producer = Producer(1);
        let mut manager =
            ImageManager::new(Limits::default(), gate.clone(), decoder, store.clone());
        manager.register(producer, Trust::Unsigned { ssh_session: ssh });
        Self {
            gate,
            manager,
            receiver: Receiver::new(),
            producer,
            store,
        }
    }

    /// The host of a real terminal: this build's gate and the worker beside it.
    #[must_use]
    pub fn for_build(setting_on: bool) -> Self {
        let runner = lumen_decoder::runner::Runner::new(worker_path());
        Self::new(build_gate(setting_on), runner, possibly_ssh())
    }

    /// The local `images` setting changed. Off always wins; on only matters if
    /// the build's gate records allow it.
    pub fn set_setting(&mut self, on: bool) {
        self.gate.setting_on = on;
        self.manager.set_gate(self.gate.clone());
    }

    /// An image starts. Returns what to show for it, and the image it ended.
    pub fn start(&mut self, start: Start) -> (Shown, Option<ImageId>) {
        let s = self.receiver.start(&mut self.manager, self.producer, start);
        let shown = match s.placement {
            Placement::Receiving => Shown::Waiting,
            Placement::Alt(why) => Shown::Alt(why),
        };
        (shown, s.abandoned)
    }

    /// Image bytes arrive.
    pub fn data(&mut self, id: ImageId, offset: u32, bytes: &[u8]) -> Data {
        self.receiver
            .data(&mut self.manager, self.producer, id, offset, bytes)
    }

    /// The command ended: an incomplete image ends with its alt text.
    pub fn end_command(&mut self) -> Option<ImageId> {
        self.receiver.end(&mut self.manager, self.producer)
    }

    /// Collects the decodes that finished since the last call.
    pub fn poll(&mut self) -> Vec<Update> {
        self.manager
            .poll()
            .into_iter()
            .map(|f| Update {
                id: f.id,
                shown: match f.result {
                    Ok(ready) => self
                        .store
                        .get(ready.texture)
                        .map_or(Shown::Alt(AltReason::UploadFailed), Shown::Image),
                    Err(why) => Shown::Alt(why),
                },
            })
            .collect()
    }
}
