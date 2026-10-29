//! The process state a real multiplexer client reports for each pane.

use lumen_panes::{
    PaneProcessState, PaneProcessStates, ProcessIdentity, ProcessStateError, Program,
};

fn program(path: &str) -> Program {
    Program::new(path).unwrap_or_else(|e| unreachable!("fixed absolute path: {e}"))
}

fn identity(pid: u32, start: u64, path: &str) -> ProcessIdentity {
    ProcessIdentity::new(pid, start, program(path))
        .unwrap_or_else(|e| unreachable!("fixed process identity: {e}"))
}

#[test]
fn process_identity_rejects_ambiguous_boundary_values() {
    assert_eq!(Program::new("zsh"), Err(ProcessStateError::RelativeProgram));
    assert_eq!(
        ProcessIdentity::new(0, 1, program("/bin/zsh")),
        Err(ProcessStateError::ZeroPid)
    );
}

#[test]
fn an_exec_stays_recorded_after_the_original_program_returns() {
    let spawn = identity(41, 90, "/bin/zsh");
    let mut state = PaneProcessState::new(spawn.clone(), true);
    state.observe_identity(identity(41, 90, "/usr/bin/ssh"));
    assert!(state.exec_seen());
    assert!(!state.pid_reuse_seen());

    state.observe_identity(spawn);
    assert!(
        state.exec_seen(),
        "returning to the shell does not erase the exec"
    );
    assert_eq!(
        state.observed().program().as_path(),
        std::path::Path::new("/bin/zsh")
    );
}

#[test]
fn a_changed_start_marker_stays_recorded_as_pid_reuse() {
    let spawn = identity(41, 90, "/bin/zsh");
    let mut state = PaneProcessState::new(spawn.clone(), true);
    state.observe_identity(identity(41, 91, "/bin/zsh"));
    assert!(state.pid_reuse_seen());
    assert!(!state.exec_seen());

    state.observe_identity(spawn);
    assert!(state.pid_reuse_seen());
}

#[test]
fn job_control_loss_and_a_wide_group_are_latched() {
    let mut state = PaneProcessState::new(identity(41, 90, "/bin/zsh"), true);
    state.observe_job_control(false);
    state.observe_job_control(true);
    assert_eq!(state.job_control(), Some(true));
    assert!(state.job_control_lost());

    assert_eq!(
        state.observe_foreground_members(0),
        Err(ProcessStateError::EmptyForegroundGroup)
    );
    assert!(state.observe_foreground_members(2).is_ok());
    assert!(state.observe_foreground_members(1).is_ok());
    assert_eq!(
        state.foreground_members().map(std::num::NonZeroU32::get),
        Some(1)
    );
    assert!(state.wide_foreground_seen());
}

#[test]
fn the_latest_foreground_program_is_kept_without_changing_the_spawn_record() {
    let mut state = PaneProcessState::new(identity(41, 90, "/bin/zsh"), true);
    state.observe_foreground_program(Some(program("/usr/bin/ssh")));
    assert_eq!(
        state.foreground_program().map(Program::as_path),
        Some(std::path::Path::new("/usr/bin/ssh"))
    );
    assert_eq!(
        state.spawned().program().as_path(),
        std::path::Path::new("/bin/zsh")
    );
}

#[test]
fn pane_records_are_isolated_and_hidden_panes_are_not_removed() {
    let mut panes = PaneProcessStates::default();
    panes.insert(7, PaneProcessState::new(identity(41, 90, "/bin/zsh"), true));
    panes.insert(
        8,
        PaneProcessState::new(identity(52, 91, "/usr/bin/vim"), false),
    );
    panes
        .get_mut(7)
        .unwrap_or_else(|| unreachable!("pane 7 exists"))
        .observe_job_control(false);

    assert!(panes.get(7).is_some_and(PaneProcessState::job_control_lost));
    assert!(panes.get(8).is_some_and(|state| !state.job_control_lost()));
    assert_eq!(
        panes.len(),
        2,
        "a layout hiding a pane does not edit process state"
    );
    assert!(panes.remove(7).is_some());
    assert_eq!(panes.len(), 1);
}
