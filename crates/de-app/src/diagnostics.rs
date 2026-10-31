//! Typed application errors and bounded, secret-safe diagnostics.
//!
//! Error messages exposed to users must be safe summaries. Detailed causes
//! remain available through the error source chain and are never serialized
//! into diagnostic records automatically.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::io::{self, Write};

use crate::lifecycle::LifecycleError;

/// Broad subsystem category used for error routing and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Configuration,
    Lifecycle,
    Runtime,
    Terminal,
    Ffi,
    Io,
    Internal,
}

impl ErrorCategory {
    /// Stable lowercase category name for diagnostic records.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::Lifecycle => "lifecycle",
            Self::Runtime => "runtime",
            Self::Terminal => "terminal",
            Self::Ffi => "ffi",
            Self::Io => "io",
            Self::Internal => "internal",
        }
    }
}

/// Stable machine-readable application error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationErrorCode {
    InvalidConfiguration,
    InvalidLifecycleTransition,
    ServiceInitializationFailed,
    ServiceShutdownFailed,
    RuntimeFailure,
    TerminalFailure,
    FfiFailure,
    IoFailure,
    InternalFailure,
}

impl ApplicationErrorCode {
    /// Stable identifier suitable for logs and support reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidConfiguration => "DE_CONFIG_INVALID",
            Self::InvalidLifecycleTransition => "DE_LIFECYCLE_INVALID_TRANSITION",
            Self::ServiceInitializationFailed => "DE_SERVICE_INIT_FAILED",
            Self::ServiceShutdownFailed => "DE_SERVICE_SHUTDOWN_FAILED",
            Self::RuntimeFailure => "DE_RUNTIME_FAILED",
            Self::TerminalFailure => "DE_TERMINAL_FAILED",
            Self::FfiFailure => "DE_FFI_FAILED",
            Self::IoFailure => "DE_IO_FAILED",
            Self::InternalFailure => "DE_INTERNAL_FAILED",
        }
    }
}

/// An application error with a safe user-facing summary and optional source.
///
/// The source is retained for programmatic error inspection, but is not
/// included in `Display` or diagnostic records. Callers must supply a
/// non-sensitive summary as the message.
#[derive(Debug)]
pub struct ApplicationError {
    category: ErrorCategory,
    code: ApplicationErrorCode,
    message: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl ApplicationError {
    /// Creates an application error with a safe summary.
    pub fn new(
        category: ErrorCategory,
        code: ApplicationErrorCode,
        safe_message: impl Into<String>,
    ) -> Self {
        Self {
            category,
            code,
            message: sanitize_text(&safe_message.into(), MAX_MESSAGE_LENGTH),
            source: None,
        }
    }

    /// Attaches a detailed source error without exposing it in user-facing text.
    pub fn with_source<E>(mut self, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        self.source = Some(Box::new(source));
        self
    }

    /// Error category.
    pub const fn category(&self) -> ErrorCategory {
        self.category
    }

    /// Stable machine-readable error code.
    pub const fn code(&self) -> ApplicationErrorCode {
        self.code
    }

    /// Safe user-facing summary.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ApplicationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source.as_ref() as &(dyn Error + 'static))
    }
}

impl From<LifecycleError> for ApplicationError {
    fn from(error: LifecycleError) -> Self {
        let (code, message) = match &error {
            LifecycleError::InvalidState { .. } => (
                ApplicationErrorCode::InvalidLifecycleTransition,
                "The application lifecycle operation is invalid in the current state.",
            ),
            LifecycleError::StartupFailed { .. } => (
                ApplicationErrorCode::ServiceInitializationFailed,
                "An application service could not be initialized.",
            ),
            LifecycleError::ShutdownFailed { .. } => (
                ApplicationErrorCode::ServiceShutdownFailed,
                "One or more application services could not be shut down.",
            ),
        };

        Self::new(ErrorCategory::Lifecycle, code, message).with_source(error)
    }
}

/// Severity of a diagnostic record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl DiagnosticSeverity {
    /// Stable lowercase severity name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
        }
    }
}

/// One sanitized diagnostic context field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticField {
    key: String,
    value: String,
}

impl DiagnosticField {
    /// Context key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Sanitized or redacted context value.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A structured diagnostic record safe to render into logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    severity: DiagnosticSeverity,
    category: ErrorCategory,
    code: ApplicationErrorCode,
    subsystem: String,
    operation: String,
    message: String,
    context: Vec<DiagnosticField>,
}

impl Diagnostic {
    /// Creates a diagnostic with a safe summary.
    pub fn new(
        severity: DiagnosticSeverity,
        category: ErrorCategory,
        code: ApplicationErrorCode,
        subsystem: impl AsRef<str>,
        operation: impl AsRef<str>,
        safe_message: impl AsRef<str>,
    ) -> Self {
        Self {
            severity,
            category,
            code,
            subsystem: sanitize_text(subsystem.as_ref(), MAX_KEY_LENGTH),
            operation: sanitize_text(operation.as_ref(), MAX_KEY_LENGTH),
            message: sanitize_text(safe_message.as_ref(), MAX_MESSAGE_LENGTH),
            context: Vec::new(),
        }
    }

    /// Builds a diagnostic from an application error without serializing its
    /// detailed source chain.
    pub fn from_error(
        error: &ApplicationError,
        severity: DiagnosticSeverity,
        subsystem: impl AsRef<str>,
        operation: impl AsRef<str>,
    ) -> Self {
        Self::new(
            severity,
            error.category(),
            error.code(),
            subsystem,
            operation,
            error.message(),
        )
    }

    /// Adds a context field. Values under sensitive keys are always redacted.
    pub fn with_context(
        mut self,
        key: impl AsRef<str>,
        value: impl AsRef<str>,
    ) -> Self {
        let safe_key = sanitize_text(key.as_ref(), MAX_KEY_LENGTH);
        let safe_value = if is_sensitive_key(&safe_key) {
            REDACTED_VALUE.to_owned()
        } else {
            sanitize_text(value.as_ref(), MAX_CONTEXT_VALUE_LENGTH)
        };

        self.context.push(DiagnosticField {
            key: safe_key,
            value: safe_value,
        });
        self
    }

    /// Severity.
    pub const fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    /// Category.
    pub const fn category(&self) -> ErrorCategory {
        self.category
    }

    /// Error code.
    pub const fn code(&self) -> ApplicationErrorCode {
        self.code
    }

    /// Sanitized context fields.
    pub fn context(&self) -> &[DiagnosticField] {
        &self.context
    }

    /// Renders one single-line, escaped log record.
    pub fn as_record(&self) -> String {
        let mut record = format!(
            "level={} category={} code={} subsystem={} operation={} message={}",
            self.severity.as_str(),
            self.category.as_str(),
            self.code.as_str(),
            quote_record_value(&self.subsystem),
            quote_record_value(&self.operation),
            quote_record_value(&self.message),
        );

        for field in &self.context {
            use fmt::Write as FmtWrite;
            let _ = write!(
                record,
                " {}={}",
                field.key,
                quote_record_value(&field.value)
            );
        }

        record
    }

    /// Writes the sanitized record followed by a newline.
    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        writeln!(writer, "{}", self.as_record())
    }
}

/// A bounded FIFO buffer for recent diagnostics.
#[derive(Debug)]
pub struct DiagnosticBuffer {
    capacity: usize,
    entries: VecDeque<Diagnostic>,
}

impl DiagnosticBuffer {
    /// Creates a bounded buffer. A zero capacity is normalized to one record.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    /// Appends a record, evicting the oldest record when full.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(diagnostic);
    }

    /// Number of retained records.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the buffer contains no records.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Maximum number of retained records.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns retained records from oldest to newest.
    pub fn snapshot(&self) -> Vec<Diagnostic> {
        self.entries.iter().cloned().collect()
    }
}

const REDACTED_VALUE: &str = "[REDACTED]";
const MAX_KEY_LENGTH: usize = 128;
const MAX_MESSAGE_LENGTH: usize = 1024;
const MAX_CONTEXT_VALUE_LENGTH: usize = 256;

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_").replace('.', "_").replace(' ', "_");
    [
        "password",
        "passwd",
        "token",
        "secret",
        "credential",
        "authorization",
        "cookie",
        "private_key",
        "api_key",
        "access_key",\n        "session_key",\n        "signing_key",\n        "encryption_key",\n        "ssh_key",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn sanitize_text(value: &str, max_length: usize) -> String {
    let mut sanitized = String::new();
    let mut length = 0;

    for character in value.chars() {
        if length >= max_length {
            break;
        }

        if character.is_control() {
            if character.is_whitespace() {
                sanitized.push(' ');
                length += 1;
            }
        } else {
            sanitized.push(character);
            length += 1;
        }
    }

    sanitized
}

fn quote_record_value(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\"");
    format!("\"{escaped}\"")
}
