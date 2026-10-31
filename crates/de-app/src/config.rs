//! Versioned, parser-independent application configuration schema.
//!
//! File-format parsing and persistence are separate concerns. A TOML/JSON
//! adapter can translate its parsed values into ConfigValue; this module owns
//! strict key validation, defaults, version checks, and semantic validation.

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::path::PathBuf;

use de_ffi::{DeAbiVersion, ABI_VERSION};
use de_runtime::{ProgramSpec, ProgramSpecError};
use de_terminal_core::TerminalSize;

/// Current persisted configuration schema version.
pub const CURRENT_CONFIG_SCHEMA_VERSION: u64 = 1;

const DEFAULT_COLUMNS: u64 = 80;
const DEFAULT_ROWS: u64 = 24;
const DEFAULT_SCROLLBACK_LINES: usize = 10_000;
const DEFAULT_MAX_SESSIONS: usize = 32;
const DEFAULT_MAX_PENDING_INPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_PASTE_BYTES: usize = 1024 * 1024;

const MAX_COLUMNS: u64 = 500;
const MAX_ROWS: u64 = 300;
const MAX_SCROLLBACK_LINES: usize = 1_000_000;
const MAX_SESSIONS: usize = 64;
const MIN_PENDING_INPUT_BYTES: usize = 4096;
const MAX_PENDING_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PASTE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARGUMENTS: usize = 256;
const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_ENVIRONMENT_ENTRIES: usize = 256;
const MAX_ENVIRONMENT_KEY_BYTES: usize = 256;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 4096;
const MAX_EXECUTABLE_BYTES: usize = 4096;
const MAX_WORKING_DIRECTORY_BYTES: usize = 4096;

/// Format-neutral value tree used by strict configuration decoding.
///
/// The persistence layer is responsible for decoding a concrete file format
/// into this representation. Unknown object keys are rejected by AppConfig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigValue {
    Null,
    Boolean(bool),
    Integer(i64),
    String(String),
    Array(Vec<ConfigValue>),
    Table(BTreeMap<String, ConfigValue>),
}

/// Error produced by configuration decoding or validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    RootMustBeTable,
    MissingField {
        section: String,
        key: String,
    },
    UnknownKey {
        section: String,
        key: String,
    },
    WrongType {
        section: String,
        key: String,
        expected: &'static str,
    },
    UnsupportedVersion {
        found: u64,
        expected: u64,
    },
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },
    NonUtf8Value {
        field: &'static str,
    },
    InvalidProgram(ProgramSpecError),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootMustBeTable => formatter.write_str("configuration root must be a table"),
            Self::MissingField { section, key } => {
                write!(formatter, "missing configuration field {section}.{key}")
            }
            Self::UnknownKey { section, key } => {
                write!(formatter, "unknown configuration key {section}.{key}")
            }
            Self::WrongType {
                section,
                key,
                expected,
            } => write!(
                formatter,
                "configuration field {section}.{key} must be {expected}"
            ),
            Self::UnsupportedVersion { found, expected } => write!(
                formatter,
                "configuration schema version {found} is unsupported; expected {expected}"
            ),
            Self::InvalidValue { field, reason } => {
                write!(
                    formatter,
                    "invalid configuration value for {field}: {reason}"
                )
            }
            Self::NonUtf8Value { field } => {
                write!(formatter, "configuration field {field} is not valid UTF-8")
            }
            Self::InvalidProgram(error) => {
                write!(formatter, "invalid startup program specification: {error}")
            }
        }
    }
}

impl Error for ConfigError {}

impl From<ProgramSpecError> for ConfigError {
    fn from(value: ProgramSpecError) -> Self {
        Self::InvalidProgram(value)
    }
}

/// Validated application configuration for the current schema version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    schema_version: u64,
    initial_terminal_size: TerminalSize,
    initial_program: ProgramSpec,
    scrollback_lines: usize,
    max_sessions: usize,
    max_pending_input_bytes: usize,
    max_paste_bytes: usize,
}

impl AppConfig {
    /// Creates an application configuration using current resource defaults.
    pub fn new(initial_terminal_size: TerminalSize, initial_program: ProgramSpec) -> Self {
        Self {
            schema_version: CURRENT_CONFIG_SCHEMA_VERSION,
            initial_terminal_size,
            initial_program,
            scrollback_lines: DEFAULT_SCROLLBACK_LINES,
            max_sessions: DEFAULT_MAX_SESSIONS,
            max_pending_input_bytes: DEFAULT_MAX_PENDING_INPUT_BYTES,
            max_paste_bytes: DEFAULT_MAX_PASTE_BYTES,
        }
    }

    /// Strictly decodes a format-neutral value tree and validates all fields.
    pub fn from_value(value: ConfigValue) -> Result<Self, ConfigError> {
        let ConfigValue::Table(mut root) = value else {
            return Err(ConfigError::RootMustBeTable);
        };

        let version = take_required_integer(&mut root, "schema_version", "root")?;
        if version != CURRENT_CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: version,
                expected: CURRENT_CONFIG_SCHEMA_VERSION,
            });
        }

        let mut terminal = take_table(&mut root, "terminal", "root")?;
        let columns = take_integer(&mut terminal, "columns", "terminal", Some(DEFAULT_COLUMNS))?;
        let rows = take_integer(&mut terminal, "rows", "terminal", Some(DEFAULT_ROWS))?;
        let scrollback_lines = take_usize(
            &mut terminal,
            "scrollback_lines",
            "terminal",
            Some(DEFAULT_SCROLLBACK_LINES),
        )?;
        ensure_no_unknown(terminal, "terminal")?;

        let mut startup = take_table(&mut root, "startup", "root")?;
        let executable = take_string(&mut startup, "program", "startup", Some("/bin/sh"))?;
        let arguments = take_string_array(&mut startup, "args", "startup")?;
        let working_directory = take_optional_string(&mut startup, "working_directory", "startup")?;
        let environment = take_string_map(&mut startup, "environment", "startup")?;
        ensure_no_unknown(startup, "startup")?;

        let mut resources = take_table(&mut root, "resources", "root")?;
        let max_sessions = take_usize(
            &mut resources,
            "max_sessions",
            "resources",
            Some(DEFAULT_MAX_SESSIONS),
        )?;
        let max_pending_input_bytes = take_usize(
            &mut resources,
            "max_pending_input_bytes",
            "resources",
            Some(DEFAULT_MAX_PENDING_INPUT_BYTES),
        )?;
        let max_paste_bytes = take_usize(
            &mut resources,
            "max_paste_bytes",
            "resources",
            Some(DEFAULT_MAX_PASTE_BYTES),
        )?;
        ensure_no_unknown(resources, "resources")?;
        ensure_no_unknown(root, "root")?;

        let columns = u16::try_from(columns).map_err(|_| ConfigError::InvalidValue {
            field: "terminal.columns",
            reason: "must fit in a non-zero u16",
        })?;
        let rows = u16::try_from(rows).map_err(|_| ConfigError::InvalidValue {
            field: "terminal.rows",
            reason: "must fit in a non-zero u16",
        })?;
        let terminal_size =
            TerminalSize::new(columns, rows).map_err(|_| ConfigError::InvalidValue {
                field: "terminal",
                reason: "columns and rows must be non-zero",
            })?;

        let mut program = ProgramSpec::new(executable)?;
        for argument in arguments {
            program = program.with_arg(argument);
        }
        for (key, value) in environment {
            program = program.with_env(key, value);
        }
        if let Some(path) = working_directory {
            program = program.with_working_directory(PathBuf::from(path));
        }

        let config = Self {
            schema_version: version,
            initial_terminal_size: terminal_size,
            initial_program: program,
            scrollback_lines,
            max_sessions,
            max_pending_input_bytes,
            max_paste_bytes,
        };
        config.validate()?;
        Ok(config)
    }

    /// Encodes this configuration into the format-neutral value tree.
    pub fn to_value(&self) -> Result<ConfigValue, ConfigError> {
        self.validate()?;

        let executable = os_str_to_string(self.initial_program.program(), "startup.program")?;
        let mut arguments = Vec::with_capacity(self.initial_program.args().len());
        for argument in self.initial_program.args() {
            arguments.push(ConfigValue::String(os_str_to_string(
                argument,
                "startup.args",
            )?));
        }

        let mut environment = BTreeMap::new();
        for (key, value) in self.initial_program.environment() {
            environment.insert(
                os_str_to_string(key, "startup.environment.key")?,
                ConfigValue::String(os_str_to_string(value, "startup.environment.value")?),
            );
        }

        let working_directory = match self.initial_program.working_directory() {
            Some(path) => ConfigValue::String(
                path.to_str()
                    .ok_or(ConfigError::NonUtf8Value {
                        field: "startup.working_directory",
                    })?
                    .to_owned(),
            ),
            None => ConfigValue::Null,
        };

        let mut terminal = BTreeMap::new();
        terminal.insert(
            "columns".to_owned(),
            ConfigValue::Integer(i64::from(self.initial_terminal_size.columns())),
        );
        terminal.insert(
            "rows".to_owned(),
            ConfigValue::Integer(i64::from(self.initial_terminal_size.rows())),
        );
        terminal.insert(
            "scrollback_lines".to_owned(),
            usize_to_value(self.scrollback_lines, "terminal.scrollback_lines")?,
        );

        let mut startup = BTreeMap::new();
        startup.insert("program".to_owned(), ConfigValue::String(executable));
        startup.insert("args".to_owned(), ConfigValue::Array(arguments));
        startup.insert("working_directory".to_owned(), working_directory);
        startup.insert("environment".to_owned(), ConfigValue::Table(environment));

        let mut resources = BTreeMap::new();
        resources.insert(
            "max_sessions".to_owned(),
            usize_to_value(self.max_sessions, "resources.max_sessions")?,
        );
        resources.insert(
            "max_pending_input_bytes".to_owned(),
            usize_to_value(
                self.max_pending_input_bytes,
                "resources.max_pending_input_bytes",
            )?,
        );
        resources.insert(
            "max_paste_bytes".to_owned(),
            usize_to_value(self.max_paste_bytes, "resources.max_paste_bytes")?,
        );

        let mut root = BTreeMap::new();
        root.insert(
            "schema_version".to_owned(),
            ConfigValue::Integer(i64::try_from(self.schema_version).map_err(|_| {
                ConfigError::InvalidValue {
                    field: "schema_version",
                    reason: "does not fit in a signed 64-bit integer",
                }
            })?),
        );
        root.insert("terminal".to_owned(), ConfigValue::Table(terminal));
        root.insert("startup".to_owned(), ConfigValue::Table(startup));
        root.insert("resources".to_owned(), ConfigValue::Table(resources));
        Ok(ConfigValue::Table(root))
    }

    /// Validates semantic constraints not expressible by value types alone.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CURRENT_CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: self.schema_version,
                expected: CURRENT_CONFIG_SCHEMA_VERSION,
            });
        }

        let columns = u64::from(self.initial_terminal_size.columns());
        let rows = u64::from(self.initial_terminal_size.rows());
        if columns > MAX_COLUMNS {
            return Err(ConfigError::InvalidValue {
                field: "terminal.columns",
                reason: "must not exceed 500",
            });
        }
        if rows > MAX_ROWS {
            return Err(ConfigError::InvalidValue {
                field: "terminal.rows",
                reason: "must not exceed 300",
            });
        }
        if usize::from(self.initial_terminal_size.columns())
            .saturating_mul(usize::from(self.initial_terminal_size.rows()))
            > 150_000
        {
            return Err(ConfigError::InvalidValue {
                field: "terminal",
                reason: "screen grid exceeds 150000 cells",
            });
        }
        if self.scrollback_lines > MAX_SCROLLBACK_LINES {
            return Err(ConfigError::InvalidValue {
                field: "terminal.scrollback_lines",
                reason: "must not exceed 1000000",
            });
        }
        if !(1..=MAX_SESSIONS).contains(&self.max_sessions) {
            return Err(ConfigError::InvalidValue {
                field: "resources.max_sessions",
                reason: "must be between 1 and 64",
            });
        }
        if !(MIN_PENDING_INPUT_BYTES..=MAX_PENDING_INPUT_BYTES)
            .contains(&self.max_pending_input_bytes)
        {
            return Err(ConfigError::InvalidValue {
                field: "resources.max_pending_input_bytes",
                reason: "must be between 4096 and 16777216",
            });
        }
        if !(1..=MAX_PASTE_BYTES).contains(&self.max_paste_bytes) {
            return Err(ConfigError::InvalidValue {
                field: "resources.max_paste_bytes",
                reason: "must be between 1 and 16777216",
            });
        }

        if self.initial_program.program().as_encoded_bytes().len() > MAX_EXECUTABLE_BYTES {
            return Err(ConfigError::InvalidValue {
                field: "startup.program",
                reason: "must not exceed 4096 bytes",
            });
        }
        if self.initial_program.args().len() > MAX_ARGUMENTS {
            return Err(ConfigError::InvalidValue {
                field: "startup.args",
                reason: "must contain at most 256 arguments",
            });
        }
        for argument in self.initial_program.args() {
            if argument.as_encoded_bytes().len() > MAX_ARGUMENT_BYTES {
                return Err(ConfigError::InvalidValue {
                    field: "startup.args",
                    reason: "each argument must not exceed 4096 bytes",
                });
            }
        }
        if self.initial_program.environment().len() > MAX_ENVIRONMENT_ENTRIES {
            return Err(ConfigError::InvalidValue {
                field: "startup.environment",
                reason: "must contain at most 256 entries",
            });
        }
        for (key, value) in self.initial_program.environment() {
            let Some(key) = key.to_str() else {
                return Err(ConfigError::NonUtf8Value {
                    field: "startup.environment.key",
                });
            };
            if !is_valid_environment_key(key) || key.len() > MAX_ENVIRONMENT_KEY_BYTES {
                return Err(ConfigError::InvalidValue {
                    field: "startup.environment.key",
                    reason: "must be a valid environment identifier of at most 256 bytes",
                });
            }
            if value.as_encoded_bytes().len() > MAX_ENVIRONMENT_VALUE_BYTES {
                return Err(ConfigError::InvalidValue {
                    field: "startup.environment.value",
                    reason: "must not exceed 4096 bytes",
                });
            }
        }
        if let Some(path) = self.initial_program.working_directory() {
            if path.as_os_str().is_empty() {
                return Err(ConfigError::InvalidValue {
                    field: "startup.working_directory",
                    reason: "must not be an empty path",
                });
            }
            if path.as_os_str().as_encoded_bytes().len() > MAX_WORKING_DIRECTORY_BYTES {
                return Err(ConfigError::InvalidValue {
                    field: "startup.working_directory",
                    reason: "must not exceed 4096 bytes",
                });
            }
        }
        self.initial_program.validate()?;
        Ok(())
    }

    /// Initial terminal dimensions.
    pub const fn initial_terminal_size(&self) -> TerminalSize {
        self.initial_terminal_size
    }

    /// Initial program launch specification.
    pub fn initial_program(&self) -> &ProgramSpec {
        &self.initial_program
    }

    /// Scrollback line budget for each terminal session.
    pub const fn scrollback_lines(&self) -> usize {
        self.scrollback_lines
    }

    /// Maximum number of concurrent terminal sessions.
    pub const fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    /// Maximum queued input bytes per session.
    pub const fn max_pending_input_bytes(&self) -> usize {
        self.max_pending_input_bytes
    }

    /// Maximum accepted paste payload in bytes.
    pub const fn max_paste_bytes(&self) -> usize {
        self.max_paste_bytes
    }

    /// Persisted schema version.
    pub const fn schema_version(&self) -> u64 {
        self.schema_version
    }

    /// Rust/C ABI version expected by this application build.
    pub const fn abi_version(&self) -> DeAbiVersion {
        ABI_VERSION
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new(
            TerminalSize::default(),
            ProgramSpec::new("/bin/sh").expect("built-in default program is valid"),
        )
    }
}

fn take_table(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<BTreeMap<String, ConfigValue>, ConfigError> {
    match table.remove(key) {
        None => Ok(BTreeMap::new()),
        Some(ConfigValue::Table(value)) => Ok(value),
        Some(_) => Err(wrong_type(section, key, "a table")),
    }
}

fn take_required_integer(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<u64, ConfigError> {
    match table.remove(key) {
        None => Err(ConfigError::MissingField {
            section: section.to_owned(),
            key: key.to_owned(),
        }),
        Some(ConfigValue::Integer(value)) if value >= 0 => Ok(value as u64),
        Some(ConfigValue::Integer(_)) => Err(invalid_value(section, key, "must be non-negative")),
        Some(_) => Err(wrong_type(section, key, "an integer")),
    }
}

fn take_integer(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
    default: Option<u64>,
) -> Result<u64, ConfigError> {
    match table.remove(key) {
        None => default.ok_or_else(|| ConfigError::MissingField {
            section: section.to_owned(),
            key: key.to_owned(),
        }),
        Some(ConfigValue::Integer(value)) if value >= 0 => Ok(value as u64),
        Some(ConfigValue::Integer(_)) => Err(invalid_value(section, key, "must be non-negative")),
        Some(_) => Err(wrong_type(section, key, "an integer")),
    }
}

fn take_usize(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
    default: Option<usize>,
) -> Result<usize, ConfigError> {
    let default = default.map(|value| u64::try_from(value).expect("supported defaults fit u64"));
    let value = take_integer(table, key, section, default)?;
    usize::try_from(value).map_err(|_| invalid_value(section, key, "does not fit usize"))
}

fn take_string(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
    default: Option<&str>,
) -> Result<String, ConfigError> {
    match table.remove(key) {
        None => default
            .map(str::to_owned)
            .ok_or_else(|| ConfigError::MissingField {
                section: section.to_owned(),
                key: key.to_owned(),
            }),
        Some(ConfigValue::String(value)) => Ok(value),
        Some(_) => Err(wrong_type(section, key, "a string")),
    }
}

fn take_optional_string(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<Option<String>, ConfigError> {
    match table.remove(key) {
        None | Some(ConfigValue::Null) => Ok(None),
        Some(ConfigValue::String(value)) => Ok(Some(value)),
        Some(_) => Err(wrong_type(section, key, "a string or null")),
    }
}

fn take_string_array(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<Vec<String>, ConfigError> {
    match table.remove(key) {
        None => Ok(Vec::new()),
        Some(ConfigValue::Array(values)) => values
            .into_iter()
            .map(|value| match value {
                ConfigValue::String(value) => Ok(value),
                _ => Err(wrong_type(section, key, "an array of strings")),
            })
            .collect(),
        Some(_) => Err(wrong_type(section, key, "an array of strings")),
    }
}

fn take_string_map(
    table: &mut BTreeMap<String, ConfigValue>,
    key: &str,
    section: &str,
) -> Result<BTreeMap<String, String>, ConfigError> {
    match table.remove(key) {
        None => Ok(BTreeMap::new()),
        Some(ConfigValue::Table(values)) => values
            .into_iter()
            .map(|(key, value)| match value {
                ConfigValue::String(value) => Ok((key, value)),
                _ => Err(wrong_type(
                    section,
                    key.as_str(),
                    "a string environment value",
                )),
            })
            .collect(),
        Some(_) => Err(wrong_type(section, key, "a table of string values")),
    }
}

fn ensure_no_unknown(
    table: BTreeMap<String, ConfigValue>,
    section: &str,
) -> Result<(), ConfigError> {
    if let Some(key) = table.keys().next() {
        return Err(ConfigError::UnknownKey {
            section: section.to_owned(),
            key: key.clone(),
        });
    }
    Ok(())
}

fn wrong_type(section: &str, key: &str, expected: &'static str) -> ConfigError {
    ConfigError::WrongType {
        section: section.to_owned(),
        key: key.to_owned(),
        expected,
    }
}

fn invalid_value(section: &str, key: &str, reason: &'static str) -> ConfigError {
    let field = match section {
        "root" => match key {
            "schema_version" => "schema_version",
            _ => "configuration",
        },
        "terminal" => match key {
            "columns" => "terminal.columns",
            "rows" => "terminal.rows",
            "scrollback_lines" => "terminal.scrollback_lines",
            _ => "terminal",
        },
        "startup" => match key {
            "program" => "startup.program",
            "args" => "startup.args",
            "working_directory" => "startup.working_directory",
            "environment" => "startup.environment",
            _ => "startup",
        },
        "resources" => match key {
            "max_sessions" => "resources.max_sessions",
            "max_pending_input_bytes" => "resources.max_pending_input_bytes",
            "max_paste_bytes" => "resources.max_paste_bytes",
            _ => "resources",
        },
        _ => "configuration",
    };
    ConfigError::InvalidValue { field, reason }
}

fn is_valid_environment_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn os_str_to_string(value: &OsStr, field: &'static str) -> Result<String, ConfigError> {
    value
        .to_str()
        .map(str::to_owned)
        .ok_or(ConfigError::NonUtf8Value { field })
}

fn usize_to_value(value: usize, field: &'static str) -> Result<ConfigValue, ConfigError> {
    let value = i64::try_from(value).map_err(|_| ConfigError::InvalidValue {
        field,
        reason: "does not fit in a signed 64-bit integer",
    })?;
    Ok(ConfigValue::Integer(value))
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, ConfigError, ConfigValue, CURRENT_CONFIG_SCHEMA_VERSION,
        DEFAULT_MAX_PASTE_BYTES, DEFAULT_MAX_PENDING_INPUT_BYTES, DEFAULT_MAX_SESSIONS,
        DEFAULT_SCROLLBACK_LINES,
    };
    use de_runtime::{ProgramSpec, ProgramSpecError};
    use de_terminal_core::TerminalSize;
    use std::ffi::OsStr;

    fn table(entries: impl IntoIterator<Item = (&'static str, ConfigValue)>) -> ConfigValue {
        ConfigValue::Table(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    fn valid_config_value() -> ConfigValue {
        table([
            (
                "schema_version",
                ConfigValue::Integer(CURRENT_CONFIG_SCHEMA_VERSION as i64),
            ),
            (
                "terminal",
                table([
                    ("columns", ConfigValue::Integer(120)),
                    ("rows", ConfigValue::Integer(40)),
                ]),
            ),
            (
                "startup",
                table([
                    ("program", ConfigValue::String("/bin/zsh".to_owned())),
                    (
                        "args",
                        ConfigValue::Array(vec![ConfigValue::String("-l".to_owned())]),
                    ),
                    (
                        "environment",
                        table([("TERM", ConfigValue::String("xterm-256color".to_owned()))]),
                    ),
                ]),
            ),
            (
                "resources",
                table([("max_sessions", ConfigValue::Integer(12))]),
            ),
        ])
    }

    #[test]
    fn defaults_are_applied_to_omitted_schema_fields() {
        let config = AppConfig::from_value(table([(
            "schema_version",
            ConfigValue::Integer(CURRENT_CONFIG_SCHEMA_VERSION as i64),
        )]))
        .expect("valid minimal schema");

        assert_eq!(config.initial_terminal_size(), TerminalSize::default());
        assert_eq!(config.initial_program().program(), OsStr::new("/bin/sh"));
        assert_eq!(config.scrollback_lines(), DEFAULT_SCROLLBACK_LINES);
        assert_eq!(config.max_sessions(), DEFAULT_MAX_SESSIONS);
        assert_eq!(
            config.max_pending_input_bytes(),
            DEFAULT_MAX_PENDING_INPUT_BYTES
        );
        assert_eq!(config.max_paste_bytes(), DEFAULT_MAX_PASTE_BYTES);
    }

    #[test]
    fn decodes_nested_values_and_preserves_separate_program_arguments() {
        let config = AppConfig::from_value(valid_config_value()).expect("valid config");
        assert_eq!(config.initial_terminal_size().columns(), 120);
        assert_eq!(config.initial_terminal_size().rows(), 40);
        assert_eq!(config.initial_program().program(), OsStr::new("/bin/zsh"));
        assert_eq!(
            config.initial_program().args(),
            &[std::ffi::OsString::from("-l")]
        );
        assert_eq!(config.max_sessions(), 12);
    }

    #[test]
    fn rejects_unknown_keys_at_root_and_in_nested_sections() {
        let mut root = match valid_config_value() {
            ConfigValue::Table(root) => root,
            _ => unreachable!(),
        };
        root.insert("mystery".to_owned(), ConfigValue::Boolean(true));
        assert!(matches!(
            AppConfig::from_value(ConfigValue::Table(root)),
            Err(ConfigError::UnknownKey { section, key })
                if section == "root" && key == "mystery"
        ));

        let mut root = match valid_config_value() {
            ConfigValue::Table(root) => root,
            _ => unreachable!(),
        };
        let terminal = root.get_mut("terminal").expect("terminal table");
        if let ConfigValue::Table(terminal) = terminal {
            terminal.insert("mystery".to_owned(), ConfigValue::Integer(1));
        }
        assert!(matches!(
            AppConfig::from_value(ConfigValue::Table(root)),
            Err(ConfigError::UnknownKey { section, key })
                if section == "terminal" && key == "mystery"
        ));
    }

    #[test]
    fn rejects_missing_or_unsupported_schema_versions() {
        assert!(matches!(
            AppConfig::from_value(table(std::iter::empty::<(&'static str, ConfigValue)>())),
            Err(ConfigError::MissingField { section, key })
                if section == "root" && key == "schema_version"
        ));
        assert_eq!(
            AppConfig::from_value(table([("schema_version", ConfigValue::Integer(2))])),
            Err(ConfigError::UnsupportedVersion {
                found: 2,
                expected: CURRENT_CONFIG_SCHEMA_VERSION
            })
        );
    }

    #[test]
    fn rejects_invalid_dimensions_and_resource_limits() {
        let invalid_columns = table([
            (
                "schema_version",
                ConfigValue::Integer(CURRENT_CONFIG_SCHEMA_VERSION as i64),
            ),
            ("terminal", table([("columns", ConfigValue::Integer(0))])),
        ]);
        assert!(matches!(
            AppConfig::from_value(invalid_columns),
            Err(ConfigError::InvalidValue {
                field: "terminal",
                ..
            })
        ));

        let invalid_sessions = table([
            (
                "schema_version",
                ConfigValue::Integer(CURRENT_CONFIG_SCHEMA_VERSION as i64),
            ),
            (
                "resources",
                table([("max_sessions", ConfigValue::Integer(0))]),
            ),
        ]);
        assert!(matches!(
            AppConfig::from_value(invalid_sessions),
            Err(ConfigError::InvalidValue {
                field: "resources.max_sessions",
                ..
            })
        ));
    }

    #[test]
    fn rejects_invalid_environment_keys_and_excessive_arguments() {
        let invalid_environment = table([
            (
                "schema_version",
                ConfigValue::Integer(CURRENT_CONFIG_SCHEMA_VERSION as i64),
            ),
            (
                "startup",
                table([(
                    "environment",
                    table([("BAD=KEY", ConfigValue::String("value".to_owned()))]),
                )]),
            ),
        ]);
        assert!(matches!(
            AppConfig::from_value(invalid_environment),
            Err(ConfigError::InvalidValue {
                field: "startup.environment.key",
                ..
            })
        ));

        let arguments = (0..257)
            .map(|index| ConfigValue::String(format!("arg-{index}")))
            .collect();
        let excessive_arguments = table([
            (
                "schema_version",
                ConfigValue::Integer(CURRENT_CONFIG_SCHEMA_VERSION as i64),
            ),
            ("startup", table([("args", ConfigValue::Array(arguments))])),
        ]);
        assert!(matches!(
            AppConfig::from_value(excessive_arguments),
            Err(ConfigError::InvalidValue {
                field: "startup.args",
                ..
            })
        ));
    }

    #[test]
    fn rejects_empty_working_directory_path() {
        let program = ProgramSpec::new("/bin/sh")
            .expect("valid executable")
            .with_working_directory("");
        let config = AppConfig::new(
            TerminalSize::new(80, 24).expect("valid dimensions"),
            program,
        );
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue {
                field: "startup.working_directory",
                ..
            })
        ));
    }

    #[test]
    fn value_tree_round_trip_preserves_supported_configuration() {
        let config = AppConfig::from_value(valid_config_value()).expect("valid config");
        let encoded = config.to_value().expect("encode config");
        let decoded = AppConfig::from_value(encoded).expect("decode config");
        assert_eq!(decoded, config);
    }

    #[test]
    fn validates_directly_constructed_config_before_encoding() {
        let invalid_program = ProgramSpec::new("/bin/sh")
            .expect("valid executable")
            .with_arg("bad\0argument");
        let invalid = AppConfig::new(
            TerminalSize::new(80, 24).expect("valid dimensions"),
            invalid_program,
        );
        assert_eq!(
            invalid.validate(),
            Err(ConfigError::InvalidProgram(
                ProgramSpecError::ArgumentContainsNul
            ))
        );
    }
}
