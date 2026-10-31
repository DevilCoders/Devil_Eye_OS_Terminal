//! Runtime-facing process configuration.
//!
//! This crate defines owned launch specifications. Actual process creation,
//! PTY allocation, signal handling, and platform adapters are implemented by
//! later runtime tasks.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// A validated description of a program to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramSpec {
    program: OsString,
    args: Vec<OsString>,
    environment: Vec<(OsString, OsString)>,
    working_directory: Option<PathBuf>,
}

/// Errors detected while constructing a process specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramSpecError {
    EmptyProgram,
}

impl ProgramSpec {
    /// Creates a process specification with a non-empty executable name.
    pub fn new(program: impl Into<OsString>) -> Result<Self, ProgramSpecError> {
        let program = program.into();
        if program.is_empty() {
            return Err(ProgramSpecError::EmptyProgram);
        }

        Ok(Self {
            program,
            args: Vec::new(),
            environment: Vec::new(),
            working_directory: None,
        })
    }

    /// Appends one argument without invoking a shell or joining a command line.
    pub fn with_arg(mut self, argument: impl Into<OsString>) -> Self {
        self.args.push(argument.into());
        self
    }

    /// Adds or replaces an environment variable in this launch specification.
    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        let key = key.into();
        let value = value.into();

        if let Some((_, existing_value)) = self
            .environment
            .iter_mut()
            .find(|(existing_key, _)| existing_key == &key)
        {
            *existing_value = value;
        } else {
            self.environment.push((key, value));
        }

        self
    }

    /// Sets the requested working directory.
    pub fn with_working_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(path.into());
        self
    }

    /// Executable path or name.
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Arguments in their original, unjoined form.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Environment overrides in insertion order.
    pub fn environment(&self) -> &[(OsString, OsString)] {
        &self.environment
    }

    /// Optional working directory.
    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::{ProgramSpec, ProgramSpecError};
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn rejects_an_empty_program() {
        assert_eq!(ProgramSpec::new(""), Err(ProgramSpecError::EmptyProgram));
    }

    #[test]
    fn preserves_arguments_as_separate_values() {
        let spec = ProgramSpec::new("/bin/echo")
            .expect("non-empty executable")
            .with_arg("two words")
            .with_arg("--literal");

        assert_eq!(spec.program(), OsStr::new("/bin/echo"));
        assert_eq!(
            spec.args(),
            &[
                OsStr::new("two words").to_owned(),
                OsStr::new("--literal").to_owned()
            ]
        );
    }

    #[test]
    fn replaces_duplicate_environment_keys() {
        let spec = ProgramSpec::new("sh")
            .expect("non-empty executable")
            .with_env("MODE", "first")
            .with_env("MODE", "second");

        assert_eq!(spec.environment().len(), 1);
        assert_eq!(
            spec.environment()[0],
            (
                OsStr::new("MODE").to_owned(),
                OsStr::new("second").to_owned()
            )
        );
    }

    #[test]
    fn stores_working_directory() {
        let spec = ProgramSpec::new("sh")
            .expect("non-empty executable")
            .with_working_directory("/tmp");

        assert_eq!(spec.working_directory(), Some(Path::new("/tmp")));
    }
}
