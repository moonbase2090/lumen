//! Writes the seed images under `DIR/{png,jpeg,webp}`: `cargo run -p
//! lumen-decoder-tools --example seeds -- DIR`. An example, so the JPEG
//! encoder stays a development dependency of the workspace.

use std::error::Error;
use std::fs;
use std::path::Path;

mod generate;

fn main() -> Result<(), Box<dyn Error>> {
    let dir = std::env::args().nth(1).ok_or("usage: seeds DIR")?;
    for (name, bytes) in generate::all()? {
        let p = Path::new(&dir).join(name);
        if let Some(d) = p.parent() {
            fs::create_dir_all(d)?;
        }
        fs::write(p, bytes)?;
    }
    Ok(())
}
