//! Drives real programs in a real window model and writes a PNG of the input
//! box in each state: `cargo run -p lumen-term --example input_box_demo -- OUT_DIR`.
//! Writes `box-<state>-<look>[-zoomed].png` for zsh at a prompt (empty, typed,
//! several lines), vim, less and a sudo password prompt.
//!
//! With `--record OUT_DIR` it instead drives one session and writes a numbered
//! frame every 100 ms (`frame-0001.png`, ...), which ffmpeg turns into a video.

use lumen_term::design::Appearance;
use lumen_term::{App, Key, Mods, Passthrough};
use std::time::{Duration, Instant};

fn pump_for(app: &mut App, ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < end {
        app.pump();
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        let key = if c == '\n' { Key::Enter } else { Key::Char(c) };
        let mods = Mods {
            shift: c == '\n',
            ..Mods::default()
        };
        let _ = app.key(key, mods);
    }
}

fn enter(app: &mut App) {
    let _ = app.key(Key::Enter, Mods::default());
}

fn shot(app: &App, dir: &str, name: &str) {
    let frame = app.view();
    let Ok(png) = frame.to_png() else {
        eprintln!("could not encode {name}");
        std::process::exit(1);
    };
    let path = format!("{dir}/{name}.png");
    if std::fs::write(&path, png).is_err() {
        eprintln!("could not write {path}");
        std::process::exit(1);
    }
    println!("{}x{} {path}", frame.width, frame.height);
}

struct Recorder {
    dir: String,
    n: u32,
}

impl Recorder {
    /// Pump for `ms`, writing a frame every 100 ms.
    fn run(&mut self, app: &mut App, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            let tick = Instant::now() + Duration::from_millis(100);
            while Instant::now() < tick {
                app.pump();
                std::thread::sleep(Duration::from_millis(10));
            }
            self.n += 1;
            let n = self.n;
            shot(app, &self.dir, &format!("frame-{n:04}"));
        }
    }
}

fn record(dir: &str) {
    let Ok(mut app) = App::start(
        Box::new(Passthrough),
        "/bin/zsh",
        &["-f", "-i"],
        2,
        1100,
        640,
    ) else {
        eprintln!("zsh did not start");
        std::process::exit(1);
    };
    app.set_appearance(Appearance::Light);
    app.set_focused(true);
    let mut rec = Recorder {
        dir: dir.to_owned(),
        n: 0,
    };
    rec.run(&mut app, 1500);
    for c in "echo typed in the box".chars() {
        let _ = app.key(Key::Char(c), Mods::default());
        rec.run(&mut app, 100);
    }
    rec.run(&mut app, 600);
    enter(&mut app);
    rec.run(&mut app, 800);
    type_str(&mut app, "vim -u NONE -N");
    enter(&mut app);
    rec.run(&mut app, 1500);
    type_str(&mut app, "ikeys go to vim");
    rec.run(&mut app, 800);
    type_str(&mut app, "\u{1b}:q!");
    enter(&mut app);
    rec.run(&mut app, 1000);
    type_str(&mut app, "seq 1 300 | less");
    enter(&mut app);
    rec.run(&mut app, 1200);
    type_str(&mut app, "G");
    rec.run(&mut app, 800);
    type_str(&mut app, "q");
    rec.run(&mut app, 800);
    type_str(&mut app, "sudo -k true");
    enter(&mut app);
    rec.run(&mut app, 3000);
    type_str(&mut app, "wrong-password");
    rec.run(&mut app, 1000);
    let _ = app.key(
        Key::Char('c'),
        Mods {
            ctrl: true,
            ..Mods::default()
        },
    );
    rec.run(&mut app, 1000);
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--record") {
        let Some(dir) = std::env::args().nth(2) else {
            eprintln!("usage: input_box_demo --record OUT_DIR");
            std::process::exit(2);
        };
        record(&dir);
        return;
    }
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: input_box_demo OUT_DIR");
        std::process::exit(2);
    };
    for (look, appearance) in [("light", Appearance::Light), ("dark", Appearance::Dark)] {
        for (zoom, scale) in [("", 1u32), ("-zoomed", 2)] {
            let Ok(mut app) = App::start(
                Box::new(Passthrough),
                "/bin/zsh",
                &["-f", "-i"],
                scale,
                720 * scale,
                380 * scale,
            ) else {
                eprintln!("zsh did not start");
                std::process::exit(1);
            };
            app.set_appearance(appearance);
            app.set_focused(true);
            pump_for(&mut app, 1500);
            let suffix = format!("{look}{zoom}");
            type_str(&mut app, "echo hello from the box");
            pump_for(&mut app, 200);
            shot(&app, &dir, &format!("box-typed-{suffix}"));
            enter(&mut app);
            pump_for(&mut app, 500);
            type_str(&mut app, "git status\ngit diff --stat");
            pump_for(&mut app, 200);
            shot(&app, &dir, &format!("box-lines-{suffix}"));
            // Drop the lines and run the programs.
            for _ in 0..40 {
                let _ = app.key(Key::Backspace, Mods::default());
            }
            type_str(&mut app, "vim -u NONE -N");
            enter(&mut app);
            pump_for(&mut app, 1200);
            shot(&app, &dir, &format!("box-vim-{suffix}"));
            type_str(&mut app, "\u{1b}");
            type_str(&mut app, ":q!");
            enter(&mut app);
            pump_for(&mut app, 800);
            type_str(&mut app, "seq 1 200 | less");
            enter(&mut app);
            pump_for(&mut app, 1000);
            shot(&app, &dir, &format!("box-less-{suffix}"));
            type_str(&mut app, "q");
            pump_for(&mut app, 600);
            type_str(&mut app, "sudo -k true");
            enter(&mut app);
            pump_for(&mut app, 4000);
            shot(&app, &dir, &format!("box-sudo-{suffix}"));
            let _ = app.key(
                Key::Char('c'),
                Mods {
                    ctrl: true,
                    ..Mods::default()
                },
            );
        }
    }
}
