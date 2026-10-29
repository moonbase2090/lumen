//! Prints the `pc-1` pane corpus: `cargo run -p lumen-evidence --example gen_pc1 > pc-1.json`.

fn main() {
    print!("{}", lumen_evidence::pc1::pc1_json());
}
