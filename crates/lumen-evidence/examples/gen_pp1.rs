//! Prints the `pp-1` pane process-trust corpus: `cargo run -p lumen-evidence --example gen_pp1 > pane-process-1.json`.

fn main() {
    print!("{}", lumen_evidence::pp1::pp1_json());
}
