//! Shell integration: the user's own shell, started with small hook scripts that
//! mark where each prompt, command and result is.
//!
//! Bright never picks a shell for the user. It runs the one it is told to, and
//! when that is zsh, bash, fish or PowerShell and it was started directly (no
//! arguments), it adds hooks that write the standard prompt marks: OSC 133 A
//! (prompt start), B (prompt end), C (command start) and D with the exit status
//! (command end), and OSC 7 with the working directory. The hooks go in with no
//! change to the user's files:
//!
//! - zsh gets a `ZDOTDIR` of its own whose `.zshenv` puts the user's back, reads
//!   the user's `.zshenv` and adds the hooks;
//! - bash gets `--rcfile` naming a file that reads `~/.bashrc` and adds them;
//! - fish gets a `vendor_conf.d` file through `XDG_DATA_DIRS`;
//! - PowerShell gets `-NoExit -Command` that dot-sources a file after the
//!   user's profile.
//!
//! Any other shell, or any program started with arguments, runs unchanged. The
//! setting `shell_integration = false` turns it all off. The scripts are written
//! to a private directory that is removed when the session ends.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The zsh start-up file that restores the user's `ZDOTDIR` and adds the hooks.
const ZSHENV: &str = include_str!("shellint/zshenv");
/// The bash start-up file: the user's `~/.bashrc`, then the hooks.
const BASHRC: &str = include_str!("shellint/bashrc");
/// The fish vendor configuration.
const FISH: &str = include_str!("shellint/bright.fish");
/// The PowerShell hooks.
const POWERSHELL: &str = include_str!("shellint/bright.ps1");

/// How a session's shell is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellOptions {
    /// Whether the hooks are added. On unless the settings turn it off.
    pub integration: bool,
}

impl Default for ShellOptions {
    fn default() -> Self {
        Self { integration: true }
    }
}

/// The shells that have hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// zsh.
    Zsh,
    /// bash.
    Bash,
    /// fish.
    Fish,
    /// PowerShell, `pwsh` or `powershell`.
    PowerShell,
}

/// The shell a program is, from its file name, or `None` for any other.
#[must_use]
pub fn detect(program: &str) -> Option<Shell> {
    // A Windows path has backslashes, whatever system this is built for.
    let name = program.rsplit(['/', '\\']).next()?;
    // A login shell is named with a leading dash.
    let name = name.trim_start_matches('-').to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    match name {
        "zsh" => Some(Shell::Zsh),
        "bash" => Some(Shell::Bash),
        "fish" => Some(Shell::Fish),
        "pwsh" | "powershell" => Some(Shell::PowerShell),
        _ => None,
    }
}

/// A private directory of hook files, removed when it is dropped.
#[derive(Debug)]
pub struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bright-shell-{}-{n}", std::process::id()));
        std::fs::create_dir(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self(dir))
    }

    fn write(&self, relative: &str, text: &str) -> std::io::Result<PathBuf> {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text)?;
        Ok(path)
    }

    /// The directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// How to start a shell: the program, its arguments, what to add to its
/// environment, and the directory of hook files that has to outlive it.
#[derive(Debug)]
pub struct Launch {
    /// The program to run.
    pub program: String,
    /// Its arguments.
    pub arguments: Vec<String>,
    /// Environment variables to add.
    pub environment: BTreeMap<String, String>,
    /// The hook files, kept until the session ends.
    pub scratch: Option<Scratch>,
}

impl Launch {
    fn unchanged(program: &str, arguments: Vec<String>) -> Self {
        Self {
            program: program.to_owned(),
            arguments,
            environment: BTreeMap::new(),
            scratch: None,
        }
    }
}

/// How `program` with `arguments` is started. A shell that has hooks, started
/// with no arguments, gets them; anything else, or a failure to write the hook
/// files, leaves it as it was. `var` reads the environment.
#[must_use]
pub fn plan(
    program: &str,
    arguments: &[String],
    options: ShellOptions,
    var: &dyn Fn(&str) -> Option<String>,
) -> Launch {
    let unchanged = || Launch::unchanged(program, arguments.to_vec());
    if !options.integration || !arguments.is_empty() {
        return unchanged();
    }
    let Some(shell) = detect(program) else {
        return unchanged();
    };
    prepare(shell, program, var).unwrap_or_else(|_| unchanged())
}

fn prepare(
    shell: Shell,
    program: &str,
    var: &dyn Fn(&str) -> Option<String>,
) -> std::io::Result<Launch> {
    let scratch = Scratch::new()?;
    let mut environment = BTreeMap::new();
    environment.insert("BRIGHT_SHELL_INTEGRATION".to_owned(), "1".to_owned());
    let mut arguments = Vec::new();
    match shell {
        Shell::Zsh => {
            scratch.write(".zshenv", ZSHENV)?;
            // The user's own ZDOTDIR, or empty for none: the file puts it back.
            environment.insert(
                "BRIGHT_USER_ZDOTDIR".to_owned(),
                var("ZDOTDIR").unwrap_or_default(),
            );
            environment.insert("ZDOTDIR".to_owned(), scratch.path().display().to_string());
        }
        Shell::Bash => {
            let rc = scratch.write("bashrc", BASHRC)?;
            arguments.push("--rcfile".to_owned());
            arguments.push(rc.display().to_string());
        }
        Shell::Fish => {
            scratch.write("share/fish/vendor_conf.d/bright.fish", FISH)?;
            let share = scratch.path().join("share").display().to_string();
            let dirs = match var("XDG_DATA_DIRS").filter(|d| !d.is_empty()) {
                Some(existing) => format!("{share}:{existing}"),
                // The default when it is not set.
                None => format!("{share}:/usr/local/share:/usr/share"),
            };
            environment.insert("XDG_DATA_DIRS".to_owned(), dirs);
        }
        Shell::PowerShell => {
            let file = scratch.write("bright.ps1", POWERSHELL)?;
            let quoted = file.display().to_string().replace('\'', "''");
            arguments.extend([
                "-NoLogo".to_owned(),
                "-NoExit".to_owned(),
                "-Command".to_owned(),
                format!(". '{quoted}'"),
            ]);
        }
    }
    Ok(Launch {
        program: program.to_owned(),
        arguments,
        environment,
        scratch: Some(scratch),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn shells_are_known_by_file_name() {
        assert_eq!(detect("/bin/zsh"), Some(Shell::Zsh));
        assert_eq!(detect("-bash"), Some(Shell::Bash));
        assert_eq!(detect("/usr/bin/fish"), Some(Shell::Fish));
        assert_eq!(
            detect("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
            Some(Shell::PowerShell)
        );
        assert_eq!(detect("/bin/sh"), None);
        assert_eq!(detect("/usr/bin/nu"), None);
        assert_eq!(detect("/opt/zshell/run"), None);
    }

    #[test]
    fn another_shell_arguments_or_the_setting_off_leave_the_launch_alone() {
        let on = ShellOptions::default();
        for (program, args, options) in [
            ("/bin/sh", vec![], on),
            ("/bin/zsh", vec!["-c".to_owned(), "true".to_owned()], on),
            ("/bin/zsh", vec![], ShellOptions { integration: false }),
        ] {
            let launch = plan(program, &args, options, &no_env);
            assert_eq!(launch.program, program);
            assert_eq!(launch.arguments, args);
            assert!(launch.environment.is_empty() && launch.scratch.is_none());
        }
    }

    #[test]
    fn the_hook_files_go_in_a_private_directory_that_goes_with_the_session() {
        let launch = plan("/bin/bash", &[], ShellOptions::default(), &no_env);
        let dir = launch.scratch.as_ref().map(|s| s.path().to_path_buf());
        let dir = dir.unwrap_or_default();
        assert!(dir.join("bashrc").is_file());
        assert_eq!(launch.arguments[0], "--rcfile");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir).map(|m| m.permissions().mode() & 0o777);
            assert_eq!(mode.ok(), Some(0o700));
        }
        drop(launch);
        assert!(!dir.exists(), "the files are removed with the session");
    }

    #[test]
    fn zsh_gets_its_own_zdotdir_and_remembers_the_users() {
        let own = |name: &str| (name == "ZDOTDIR").then(|| "/srv/someone/zsh".to_owned());
        let launch = plan("/bin/zsh", &[], ShellOptions::default(), &own);
        assert_eq!(
            launch.environment["BRIGHT_USER_ZDOTDIR"],
            "/srv/someone/zsh"
        );
        assert_ne!(launch.environment["ZDOTDIR"], "/srv/someone/zsh");
        let unset = plan("/bin/zsh", &[], ShellOptions::default(), &no_env);
        assert_eq!(unset.environment["BRIGHT_USER_ZDOTDIR"], "");
    }

    #[test]
    fn fish_keeps_the_users_data_directories_after_its_own() {
        let own = |name: &str| (name == "XDG_DATA_DIRS").then(|| "/a:/b".to_owned());
        let launch = plan("/usr/bin/fish", &[], ShellOptions::default(), &own);
        assert!(launch.environment["XDG_DATA_DIRS"].ends_with(":/a:/b"));
        let share = launch
            .scratch
            .as_ref()
            .map(|s| s.path().join("share/fish/vendor_conf.d/bright.fish"));
        assert!(share.is_some_and(|p| p.is_file()));
    }

    #[test]
    fn powershell_dot_sources_the_file_after_the_profile_and_stays_open() {
        let launch = plan("pwsh", &[], ShellOptions::default(), &no_env);
        assert!(launch.arguments.contains(&"-NoExit".to_owned()));
        assert!(!launch.arguments.contains(&"-NoProfile".to_owned()));
        assert!(
            launch
                .arguments
                .last()
                .is_some_and(|a| a.starts_with(". '"))
        );
    }
}
