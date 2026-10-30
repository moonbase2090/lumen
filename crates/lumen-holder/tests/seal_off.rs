//! Without the `seal-requests` feature the holder does not know a seal request:
//! it is an unknown message type, dropped and counted, and nothing is answered.

#![cfg(not(feature = "seal-requests"))]

use lumen_holder::layer::{Layer, Outcome};
use lumen_holder::principal::{Creds, Principal, Terminal};
use lumen_holder::protocol::{self, Malformed};

const TERMINAL: Terminal = Terminal {
    pid: 100,
    uid: 1000,
};
const SHELL: Principal = Principal {
    pid: 4242,
    start_time: 99_000,
    uid: 1000,
};
const FROM_SHELL: Option<Creds> = Some(Creds {
    pid: 4242,
    uid: 1000,
    gid: 1000,
});
const FROM_TERMINAL: Option<Creds> = Some(Creds {
    pid: 100,
    uid: 1000,
    gid: 1000,
});

#[test]
fn a_seal_request_is_an_unknown_type_and_is_not_answered() {
    let mut layer = Layer::new(SHELL, TERMINAL);
    layer.handle(FROM_SHELL, &protocol::ready(1), false, &|_| Some(99_000));
    let key = protocol::key_message(1, 12, &[0x5a; 32]);
    layer.handle_spawn(FROM_TERMINAL, &key[..], false, &|_| true);
    let request = protocol::seal_request(1, 0x11, &[]).unwrap_or_default();
    assert_eq!(protocol::parse(&request, false), Err(Malformed::Type));
    assert_eq!(
        layer.handle(FROM_SHELL, &request, false, &|_| Some(99_000)),
        Outcome::Malformed(Malformed::Type)
    );
    assert_eq!(layer.take_reply(), None);
    let c = layer.counters();
    assert_eq!((c.sealed, c.seal_refused, c.malformed), (0, 0, 1));
}
