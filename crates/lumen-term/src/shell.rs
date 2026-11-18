//! Which program the window runs when none was named on the command line.
//!
//! The order is the user's choice first, then the platform's:
//!
//! 1. the `shell` setting in the settings file;
//! 2. the `SHELL` environment variable;
//! 3. on Windows, `pwsh.exe` if it is on the path, then `powershell.exe`, then
//!    the program `COMSPEC` names, then `cmd.exe`;
//! 4. elsewhere, `/bin/sh`.
//!
//! An empty `SHELL` counts as not set. An empty or blank `shell` setting never
//! reaches here from the settings file, which refuses it as a bad value and is
//! ignored whole; [`choose`] still treats a blank setting as unset for any
//! other caller. Nothing here checks that a program the user named exists:
//! starting it fails with the system's own message.

use std::path::Path;

/// The shell to run: the first of the choices above that is set. `on_path`
/// says whether a program of that file name is on the executable path, and is
/// asked only on Windows.
#[must_use]
pub fn choose(
    setting: Option<&str>,
    env_shell: Option<&str>,
    windows: bool,
    comspec: Option<&str>,
    on_path: impl Fn(&str) -> bool,
) -> String {
    let set = |v: Option<&str>| v.filter(|s| !s.trim().is_empty()).map(str::to_owned);
    if let Some(user) = set(setting).or_else(|| set(env_shell)) {
        return user;
    }
    if !windows {
        return "/bin/sh".to_owned();
    }
    ["pwsh.exe", "powershell.exe"]
        .into_iter()
        .find(|name| on_path(name))
        .map(str::to_owned)
        .or_else(|| set(comspec))
        .unwrap_or_else(|| "cmd.exe".to_owned())
}

/// The shell for this machine: [`choose`] with the real environment.
#[must_use]
pub fn default_shell(setting: Option<&str>) -> String {
    choose(
        setting,
        std::env::var("SHELL").ok().as_deref(),
        cfg!(windows),
        std::env::var("COMSPEC").ok().as_deref(),
        in_path,
    )
}

/// Whether a file called `name` is in a directory of `PATH`.
#[must_use]
pub fn in_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| Path::new(&dir).join(name).is_file())
    })
}
