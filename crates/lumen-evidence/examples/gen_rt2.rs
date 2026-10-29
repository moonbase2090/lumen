//! Prints the `rt-2` corpus: `cargo run -p lumen-evidence --example gen_rt2 > rt-2.json`.

fn main() {
    print!("{}", lumen_evidence::rt2::rt2_json());
}
