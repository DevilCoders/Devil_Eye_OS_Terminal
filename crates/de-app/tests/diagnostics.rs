use std::error::Error;

use de_app::{
    ApplicationError, ApplicationErrorCode, ApplicationState, Diagnostic, DiagnosticBuffer,
    DiagnosticSeverity, ErrorCategory, LifecycleError, ServiceError,
};

#[test]
fn sensitive_context_values_are_redacted_in_log_records() {
    let diagnostic = Diagnostic::new(
        DiagnosticSeverity::Error,
        ErrorCategory::Runtime,
        ApplicationErrorCode::RuntimeFailure,
        "runtime",
        "spawn",
        "Unable to start requested process.",
    )
    .with_context("access_token", "top-secret-token")
    .with_context("API-Key", "top-secret-key")
    .with_context("password", "top-secret-password")
    .with_context("program", "/usr/bin/example");

    let record = diagnostic.as_record();

    assert!(!record.contains("top-secret"));
    assert_eq!(record.matches("[REDACTED]").count(), 3);
    assert!(record.contains("/usr/bin/example"));
    assert!(record.contains("DE_RUNTIME_FAILED"));
}

#[test]
fn detailed_source_errors_are_not_serialized_or_displayed() {
    let error = ApplicationError::new(
        ErrorCategory::Runtime,
        ApplicationErrorCode::RuntimeFailure,
        "Unable to start the requested process.",
    )
    .with_source(ServiceError::new("environment TOKEN=source-secret"));

    assert_eq!(error.to_string(), "Unable to start the requested process.");
    assert!(error.source().is_some());

    let record =
        Diagnostic::from_error(&error, DiagnosticSeverity::Error, "runtime", "spawn").as_record();

    assert!(!record.contains("source-secret"));
    assert!(record.contains("Unable to start the requested process."));
}

#[test]
fn lifecycle_errors_map_to_stable_application_error_codes() {
    let error: ApplicationError = LifecycleError::InvalidState {
        operation: "start",
        state: ApplicationState::Running,
    }
    .into();

    assert_eq!(error.category(), ErrorCategory::Lifecycle);
    assert_eq!(
        error.code(),
        ApplicationErrorCode::InvalidLifecycleTransition
    );
    assert_eq!(
        error.message(),
        "The application lifecycle operation is invalid in the current state."
    );
    assert!(error.source().is_some());
}

#[test]
fn diagnostic_text_is_single_line_and_bounded() {
    let diagnostic = Diagnostic::new(
        DiagnosticSeverity::Warning,
        ErrorCategory::Configuration,
        ApplicationErrorCode::InvalidConfiguration,
        "config\nsubsystem",
        "load\rconfig",
        "invalid\nconfiguration\tvalue",
    )
    .with_context("profile", "work\nstation");

    let record = diagnostic.as_record();

    assert!(!record.contains('\n'));
    assert!(!record.contains('\r'));
    assert!(record.contains("config subsystem"));
    assert!(record.contains("invalid configuration value"));
}

#[test]
fn diagnostic_buffer_evicts_oldest_records_at_capacity() {
    let mut buffer = DiagnosticBuffer::new(2);
    let make_record = |message: &str| {
        Diagnostic::new(
            DiagnosticSeverity::Info,
            ErrorCategory::Internal,
            ApplicationErrorCode::InternalFailure,
            "test",
            "record",
            message,
        )
    };

    buffer.push(make_record("first"));
    buffer.push(make_record("second"));
    buffer.push(make_record("third"));

    let snapshot = buffer.snapshot();
    assert_eq!(buffer.capacity(), 2);
    assert_eq!(buffer.len(), 2);
    assert!(snapshot[0].as_record().contains("second"));
    assert!(snapshot[1].as_record().contains("third"));
}

#[test]
fn diagnostic_can_be_written_to_a_standard_writer() {
    let diagnostic = Diagnostic::new(
        DiagnosticSeverity::Info,
        ErrorCategory::Internal,
        ApplicationErrorCode::InternalFailure,
        "test",
        "write",
        "safe message",
    );
    let mut output = Vec::new();

    diagnostic.write_to(&mut output).expect("write diagnostic");
    let output = String::from_utf8(output).expect("UTF-8 log record");

    assert!(output.ends_with('\n'));
    assert!(output.contains("safe message"));
}
