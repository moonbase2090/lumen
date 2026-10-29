//! Prints the `pc-2` pane corpus: `cargo run -p lumen-evidence --example gen_pc1 > pane-corpus-1.json`.

fn main() {
    print!("{}", lumen_evidence::pc1::pc1_json());
}
