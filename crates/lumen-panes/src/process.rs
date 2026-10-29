//! Runtime process state for panes of a Lumen-aware multiplexer.
//!
//! The multiplexer client reports process observations. This module keeps the
//! terminal's original spawn record separate from the latest observation and
//! latches changes that cannot become trusted again later. It does not decide
//! that a pane is local. Multiplexer panes remain possibly remote in v0.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

/// A process-state field that could not be represented safely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessStateError {
    /// Process ids are nonzero.
    ZeroPid,
    /// Programs are recorded by absolute path, never by a name resolved from
    /// `PATH`.
    RelativeProgram,
    /// A foreground process group has at least one member.
    EmptyForegroundGroup,
}

impl std::fmt::Display for ProcessStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroPid => f.write_str("a process id must be nonzero"),
            Self::RelativeProgram => f.write_str("a program path must be absolute"),
            Self::EmptyForegroundGroup => {
                f.write_str("a foreground process group must have at least one member")
            }
        }
    }
}

impl std::error::Error for ProcessStateError {}

/// An absolute executable path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Program(PathBuf);

impl Program {
    /// Validate and retain an executable path reported at the process
    /// boundary.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessStateError::RelativeProgram`] for a relative path.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, ProcessStateError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(ProcessStateError::RelativeProgram);
        }
        Ok(Self(path))
    }

    /// The validated path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// The identity of one observed process. The start marker disambiguates pid
/// reuse and is opaque to this crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    pid: NonZeroU32,
    start_marker: u64,
    program: Program,
}

impl ProcessIdentity {
    /// A process identity reported by the runtime boundary.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessStateError::ZeroPid`] for pid zero.
    pub fn new(pid: u32, start_marker: u64, program: Program) -> Result<Self, ProcessStateError> {
        let Some(pid) = NonZeroU32::new(pid) else {
            return Err(ProcessStateError::ZeroPid);
        };
        Ok(Self {
            pid,
            start_marker,
            program,
        })
    }

    /// The process id.
    #[must_use]
    pub fn pid(&self) -> NonZeroU32 {
        self.pid
    }

    /// The platform's opaque process start marker.
    #[must_use]
    pub fn start_marker(&self) -> u64 {
        self.start_marker
    }

    /// The absolute executable path.
    #[must_use]
    pub fn program(&self) -> &Program {
        &self.program
    }
}

/// The process observations for one pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneProcessState {
    spawned: ProcessIdentity,
    observed: ProcessIdentity,
    listed_shell: bool,
    job_control: Option<bool>,
    foreground_members: Option<NonZeroU32>,
    foreground_program: Option<Program>,
    changes: ProcessChanges,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProcessChanges(u8);

impl ProcessChanges {
    const EXEC: u8 = 1 << 0;
    const PID_REUSE: u8 = 1 << 1;
    const JOB_CONTROL_LOST: u8 = 1 << 2;
    const WIDE_FOREGROUND: u8 = 1 << 3;

    fn record(&mut self, change: u8) {
        self.0 |= change;
    }

    fn contains(self, change: u8) -> bool {
        self.0 & change != 0
    }
}

impl PaneProcessState {
    /// Start the record at the terminal-spawned process.
    #[must_use]
    pub fn new(spawned: ProcessIdentity, listed_shell: bool) -> Self {
        Self {
            observed: spawned.clone(),
            spawned,
            listed_shell,
            job_control: None,
            foreground_members: None,
            foreground_program: None,
            changes: ProcessChanges::default(),
        }
    }

    /// The identity recorded at spawn.
    #[must_use]
    pub fn spawned(&self) -> &ProcessIdentity {
        &self.spawned
    }

    /// The latest identity reported for the pane's child pid.
    #[must_use]
    pub fn observed(&self) -> &ProcessIdentity {
        &self.observed
    }

    /// Whether the original executable was a configured listed shell.
    #[must_use]
    pub fn listed_shell(&self) -> bool {
        self.listed_shell
    }

    /// Record the latest child identity. A changed executable or start marker
    /// stays recorded even if a later observation matches the spawn again.
    pub fn observe_identity(&mut self, observed: ProcessIdentity) {
        if observed.program != self.spawned.program {
            self.changes.record(ProcessChanges::EXEC);
        }
        if observed.pid != self.spawned.pid || observed.start_marker != self.spawned.start_marker {
            self.changes.record(ProcessChanges::PID_REUSE);
        }
        self.observed = observed;
    }

    /// Whether an executable other than the spawned one has been observed.
    #[must_use]
    pub fn exec_seen(&self) -> bool {
        self.changes.contains(ProcessChanges::EXEC)
    }

    /// Whether the child pid or its start marker has differed from the spawn
    /// record.
    #[must_use]
    pub fn pid_reuse_seen(&self) -> bool {
        self.changes.contains(ProcessChanges::PID_REUSE)
    }

    /// Record whether the controlling terminal has job control.
    pub fn observe_job_control(&mut self, on: bool) {
        self.job_control = Some(on);
        if !on {
            self.changes.record(ProcessChanges::JOB_CONTROL_LOST);
        }
    }

    /// The latest job-control observation, or `None` before one arrives.
    #[must_use]
    pub fn job_control(&self) -> Option<bool> {
        self.job_control
    }

    /// Whether job control has been observed off at least once.
    #[must_use]
    pub fn job_control_lost(&self) -> bool {
        self.changes.contains(ProcessChanges::JOB_CONTROL_LOST)
    }

    /// Record the size of the controlling terminal's foreground process group.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessStateError::EmptyForegroundGroup`] for zero members.
    pub fn observe_foreground_members(&mut self, members: u32) -> Result<(), ProcessStateError> {
        let Some(members) = NonZeroU32::new(members) else {
            return Err(ProcessStateError::EmptyForegroundGroup);
        };
        if members.get() > 1 {
            self.changes.record(ProcessChanges::WIDE_FOREGROUND);
        }
        self.foreground_members = Some(members);
        Ok(())
    }

    /// The latest foreground group size, or `None` before one arrives.
    #[must_use]
    pub fn foreground_members(&self) -> Option<NonZeroU32> {
        self.foreground_members
    }

    /// Whether a foreground group with more than one member has been observed.
    #[must_use]
    pub fn wide_foreground_seen(&self) -> bool {
        self.changes.contains(ProcessChanges::WIDE_FOREGROUND)
    }

    /// Record the current foreground executable, or that it could not be read.
    pub fn observe_foreground_program(&mut self, program: Option<Program>) {
        self.foreground_program = program;
    }

    /// The latest foreground executable, if one was observed.
    #[must_use]
    pub fn foreground_program(&self) -> Option<&Program> {
        self.foreground_program.as_ref()
    }
}

/// Process state keyed by the opaque pane id from the sealed layout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneProcessStates {
    panes: BTreeMap<u32, PaneProcessState>,
}

impl PaneProcessStates {
    /// Insert or replace the spawn record for a pane.
    pub fn insert(&mut self, pane: u32, state: PaneProcessState) -> Option<PaneProcessState> {
        self.panes.insert(pane, state)
    }

    /// Read one pane's process state.
    #[must_use]
    pub fn get(&self, pane: u32) -> Option<&PaneProcessState> {
        self.panes.get(&pane)
    }

    /// Update one pane's process state.
    pub fn get_mut(&mut self, pane: u32) -> Option<&mut PaneProcessState> {
        self.panes.get_mut(&pane)
    }

    /// Remove the record for a pane that closed. Hidden panes stay recorded.
    pub fn remove(&mut self, pane: u32) -> Option<PaneProcessState> {
        self.panes.remove(&pane)
    }

    /// The number of panes with a spawn record.
    #[must_use]
    pub fn len(&self) -> usize {
        self.panes.len()
    }

    /// Whether no pane has a spawn record.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }
}
