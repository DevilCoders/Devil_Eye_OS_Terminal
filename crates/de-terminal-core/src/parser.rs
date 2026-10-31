//! Incremental, bounded parser for 7-bit VT/ANSI control sequences.
//!
//! The parser is byte-oriented. Printable bytes (including UTF-8 bytes) are
//! forwarded unchanged as events; grapheme segmentation and cell placement
//! belong to the text/screen integration layer. Control strings are discarded
//! without retaining their payload.

use std::error::Error;
use std::fmt;

/// Maximum input accepted by one call to VtParser::process.
pub const MAX_INPUT_CHUNK_BYTES: usize = 4096;
/// Maximum raw parameter bytes retained for one CSI sequence.
pub const MAX_CSI_PARAMETER_BYTES: usize = 64;
/// Maximum payload bytes accepted for one control string.
pub const MAX_CONTROL_STRING_BYTES: usize = 4096;
const MAX_ESCAPE_INTERMEDIATES: usize = 2;

/// The kind of string control sequence whose payload is intentionally ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlStringKind {
    Osc,
    Dcs,
    Sos,
    Pm,
    Apc,
}

/// A completed CSI sequence. Parameter bytes are preserved verbatim so the
/// command consumer can apply command-specific defaults and subparameter rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsiSequence {
    parameters: [u8; MAX_CSI_PARAMETER_BYTES],
    parameter_len: u8,
    intermediates: [u8; MAX_ESCAPE_INTERMEDIATES],
    intermediate_len: u8,
    final_byte: u8,
}

impl CsiSequence {
    /// Raw parameter bytes, including private markers and separators.
    pub fn parameter_bytes(&self) -> &[u8] {
        &self.parameters[..usize::from(self.parameter_len)]
    }

    /// Intermediate bytes between parameters and the final byte.
    pub fn intermediates(&self) -> &[u8] {
        &self.intermediates[..usize::from(self.intermediate_len)]
    }

    /// CSI final byte (0x40..=0x7e).
    pub const fn final_byte(self) -> u8 {
        self.final_byte
    }
}

/// A completed ESC sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EscapeSequence {
    intermediates: [u8; MAX_ESCAPE_INTERMEDIATES],
    intermediate_len: u8,
    final_byte: u8,
}

impl EscapeSequence {
    pub fn intermediates(&self) -> &[u8] {
        &self.intermediates[..usize::from(self.intermediate_len)]
    }

    pub const fn final_byte(self) -> u8 {
        self.final_byte
    }
}

/// Parser output. No event contains borrowed input memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalEvent {
    /// One byte of printable/text data, forwarded without UTF-8 decoding.
    PrintByte(u8),
    /// A C0 or DEL control byte to be executed by the terminal state layer.
    Execute(u8),
    Csi(CsiSequence),
    Escape(EscapeSequence),
    /// A control string completed and its bounded payload was discarded.
    UnsupportedControlString(ControlStringKind),
    /// A malformed or over-limit sequence was discarded safely.
    Malformed(SequenceError),
    /// CAN or SUB cancelled an in-progress sequence.
    Cancelled,
}

/// Recoverable sequence-level parser failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceError {
    InvalidByte,
    EscapeTooLong,
    CsiTooLong,
    ControlStringTooLong,
}

/// A call-level error. The parser state is unchanged when this is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserError {
    InputChunkTooLarge { actual: usize, maximum: usize },
}

impl fmt::Display for ParserError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputChunkTooLarge { actual, maximum } => write!(
                formatter,
                "input chunk of {actual} bytes exceeds parser limit of {maximum}"
            ),
        }
    }
}

impl Error for ParserError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape {
        intermediates: [u8; MAX_ESCAPE_INTERMEDIATES],
        length: u8,
    },
    EscapeIgnore,
    Csi {
        parameters: [u8; MAX_CSI_PARAMETER_BYTES],
        parameter_length: u8,
        intermediates: [u8; MAX_ESCAPE_INTERMEDIATES],
        intermediate_length: u8,
        in_intermediates: bool,
    },
    CsiIgnore,
    ControlString {
        kind: ControlStringKind,
        length: usize,
        overflowed: bool,
    },
    ControlStringEscape {
        kind: ControlStringKind,
        length: usize,
        overflowed: bool,
    },
}

/// Stateful parser for incremental 7-bit VT/ANSI input.
///
/// Calls preserve incomplete escape/CSI/control-string state across chunks.
/// Each call accepts at most MAX_INPUT_CHUNK_BYTES and emits at most one
/// event per input byte, keeping temporary memory bounded by that limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VtParser {
    state: State,
}

impl Default for VtParser {
    fn default() -> Self {
        Self::new()
    }
}

impl VtParser {
    pub const fn new() -> Self {
        Self {
            state: State::Ground,
        }
    }

    /// Returns true when no control sequence is currently incomplete.
    pub const fn is_ground(&self) -> bool {
        matches!(self.state, State::Ground)
    }

    /// Discards any incomplete sequence and returns to the ground state.
    pub fn reset(&mut self) {
        self.state = State::Ground;
    }

    /// Consumes one bounded byte chunk and returns the events produced by it.
    ///
    /// A too-large chunk is rejected before any parser state is changed.
    pub fn process(&mut self, input: &[u8]) -> Result<Vec<TerminalEvent>, ParserError> {
        if input.len() > MAX_INPUT_CHUNK_BYTES {
            return Err(ParserError::InputChunkTooLarge {
                actual: input.len(),
                maximum: MAX_INPUT_CHUNK_BYTES,
            });
        }

        let mut events = Vec::with_capacity(input.len());
        for &byte in input {
            self.consume(byte, &mut events);
        }
        Ok(events)
    }

    fn consume(&mut self, byte: u8, events: &mut Vec<TerminalEvent>) {
        let state = std::mem::replace(&mut self.state, State::Ground);
        self.state = match state {
            State::Ground => self.consume_ground(byte, events),
            State::Escape {
                intermediates,
                length,
            } => self.consume_escape(byte, intermediates, length, events),
            State::EscapeIgnore => self.consume_escape_ignore(byte, events),
            State::Csi {
                parameters,
                parameter_length,
                intermediates,
                intermediate_length,
                in_intermediates,
            } => self.consume_csi(
                byte,
                parameters,
                parameter_length,
                intermediates,
                intermediate_length,
                in_intermediates,
                events,
            ),
            State::CsiIgnore => self.consume_csi_ignore(byte, events),
            State::ControlString {
                kind,
                length,
                overflowed,
            } => self.consume_control_string(byte, kind, length, overflowed, events),
            State::ControlStringEscape {
                kind,
                length,
                overflowed,
            } => self.consume_control_string_escape(byte, kind, length, overflowed, events),
        };
    }

    fn consume_ground(&self, byte: u8, events: &mut Vec<TerminalEvent>) -> State {
        match byte {
            0x1b => State::Escape {
                intermediates: [0; MAX_ESCAPE_INTERMEDIATES],
                length: 0,
            },
            0x00..=0x1f | 0x7f => {
                events.push(TerminalEvent::Execute(byte));
                State::Ground
            }
            _ => {
                events.push(TerminalEvent::PrintByte(byte));
                State::Ground
            }
        }
    }

    fn consume_escape(
        &self,
        byte: u8,
        mut intermediates: [u8; MAX_ESCAPE_INTERMEDIATES],
        length: u8,
        events: &mut Vec<TerminalEvent>,
    ) -> State {
        match byte {
            0x18 | 0x1a => {
                events.push(TerminalEvent::Cancelled);
                State::Ground
            }
            0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => {
                events.push(TerminalEvent::Execute(byte));
                State::Escape {
                    intermediates,
                    length,
                }
            }
            b'[' if length == 0 => State::Csi {
                parameters: [0; MAX_CSI_PARAMETER_BYTES],
                parameter_length: 0,
                intermediates: [0; MAX_ESCAPE_INTERMEDIATES],
                intermediate_length: 0,
                in_intermediates: false,
            },
            b']' if length == 0 => State::ControlString {
                kind: ControlStringKind::Osc,
                length: 0,
                overflowed: false,
            },
            b'P' if length == 0 => State::ControlString {
                kind: ControlStringKind::Dcs,
                length: 0,
                overflowed: false,
            },
            b'X' if length == 0 => State::ControlString {
                kind: ControlStringKind::Sos,
                length: 0,
                overflowed: false,
            },
            b'^' if length == 0 => State::ControlString {
                kind: ControlStringKind::Pm,
                length: 0,
                overflowed: false,
            },
            b'_' if length == 0 => State::ControlString {
                kind: ControlStringKind::Apc,
                length: 0,
                overflowed: false,
            },
            0x20..=0x2f => {
                if usize::from(length) >= MAX_ESCAPE_INTERMEDIATES {
                    events.push(TerminalEvent::Malformed(SequenceError::EscapeTooLong));
                    State::EscapeIgnore
                } else {
                    intermediates[usize::from(length)] = byte;
                    State::Escape {
                        intermediates,
                        length: length + 1,
                    }
                }
            }
            0x30..=0x7e => {
                events.push(TerminalEvent::Escape(EscapeSequence {
                    intermediates,
                    intermediate_len: length,
                    final_byte: byte,
                }));
                State::Ground
            }
            0x1b => {
                events.push(TerminalEvent::Malformed(SequenceError::InvalidByte));
                State::Escape {
                    intermediates: [0; MAX_ESCAPE_INTERMEDIATES],
                    length: 0,
                }
            }
            _ => {
                events.push(TerminalEvent::Malformed(SequenceError::InvalidByte));
                State::Ground
            }
        }
    }

    fn consume_escape_ignore(&self, byte: u8, events: &mut Vec<TerminalEvent>) -> State {
        match byte {
            0x18 | 0x1a => {
                events.push(TerminalEvent::Cancelled);
                State::Ground
            }
            0x1b => State::Escape {
                intermediates: [0; MAX_ESCAPE_INTERMEDIATES],
                length: 0,
            },
            0x30..=0x7e => State::Ground,
            _ => State::EscapeIgnore,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn consume_csi(
        &self,
        byte: u8,
        parameters: [u8; MAX_CSI_PARAMETER_BYTES],
        parameter_length: u8,
        mut intermediates: [u8; MAX_ESCAPE_INTERMEDIATES],
        intermediate_length: u8,
        in_intermediates: bool,
        events: &mut Vec<TerminalEvent>,
    ) -> State {
        match byte {
            0x18 | 0x1a => {
                events.push(TerminalEvent::Cancelled);
                State::Ground
            }
            0x1b => {
                events.push(TerminalEvent::Malformed(SequenceError::InvalidByte));
                State::Escape {
                    intermediates: [0; MAX_ESCAPE_INTERMEDIATES],
                    length: 0,
                }
            }
            0x00..=0x17 | 0x19 | 0x1c..=0x1f | 0x7f => {
                events.push(TerminalEvent::Execute(byte));
                State::Csi {
                    parameters,
                    parameter_length,
                    intermediates,
                    intermediate_length,
                    in_intermediates,
                }
            }
            0x20..=0x2f => {
                if usize::from(intermediate_length) >= MAX_ESCAPE_INTERMEDIATES {
                    events.push(TerminalEvent::Malformed(SequenceError::CsiTooLong));
                    State::CsiIgnore
                } else {
                    intermediates[usize::from(intermediate_length)] = byte;
                    State::Csi {
                        parameters,
                        parameter_length,
                        intermediates,
                        intermediate_length: intermediate_length + 1,
                        in_intermediates: true,
                    }
                }
            }
            0x30..=0x3f => {
                if in_intermediates {
                    events.push(TerminalEvent::Malformed(SequenceError::InvalidByte));
                    State::CsiIgnore
                } else if usize::from(parameter_length) >= MAX_CSI_PARAMETER_BYTES {
                    events.push(TerminalEvent::Malformed(SequenceError::CsiTooLong));
                    State::CsiIgnore
                } else {
                    let mut parameters = parameters;
                    parameters[usize::from(parameter_length)] = byte;
                    State::Csi {
                        parameters,
                        parameter_length: parameter_length + 1,
                        intermediates,
                        intermediate_length,
                        in_intermediates,
                    }
                }
            }
            0x40..=0x7e => {
                events.push(TerminalEvent::Csi(CsiSequence {
                    parameters,
                    parameter_len: parameter_length,
                    intermediates,
                    intermediate_len: intermediate_length,
                    final_byte: byte,
                }));
                State::Ground
            }
            _ => {
                events.push(TerminalEvent::Malformed(SequenceError::InvalidByte));
                State::Ground
            }
        }
    }

    fn consume_csi_ignore(&self, byte: u8, events: &mut Vec<TerminalEvent>) -> State {
        match byte {
            0x18 | 0x1a => {
                events.push(TerminalEvent::Cancelled);
                State::Ground
            }
            0x1b => State::Escape {
                intermediates: [0; MAX_ESCAPE_INTERMEDIATES],
                length: 0,
            },
            0x40..=0x7e => State::Ground,
            _ => State::CsiIgnore,
        }
    }

    fn consume_control_string(
        &self,
        byte: u8,
        kind: ControlStringKind,
        length: usize,
        overflowed: bool,
        events: &mut Vec<TerminalEvent>,
    ) -> State {
        match byte {
            0x18 | 0x1a => {
                events.push(TerminalEvent::Cancelled);
                State::Ground
            }
            0x07 if kind == ControlStringKind::Osc => {
                finish_control_string(kind, overflowed, events);
                State::Ground
            }
            0x1b => State::ControlStringEscape {
                kind,
                length,
                overflowed,
            },
            _ => {
                let new_length = length.saturating_add(1);
                State::ControlString {
                    kind,
                    length: new_length,
                    overflowed: overflowed || new_length > MAX_CONTROL_STRING_BYTES,
                }
            }
        }
    }

    fn consume_control_string_escape(
        &self,
        byte: u8,
        kind: ControlStringKind,
        length: usize,
        overflowed: bool,
        events: &mut Vec<TerminalEvent>,
    ) -> State {
        match byte {
            b'\\' => {
                finish_control_string(kind, overflowed, events);
                State::Ground
            }
            0x07 if kind == ControlStringKind::Osc => {
                let new_length = length.saturating_add(1);
                finish_control_string(
                    kind,
                    overflowed || new_length > MAX_CONTROL_STRING_BYTES,
                    events,
                );
                State::Ground
            }
            0x18 | 0x1a => {
                events.push(TerminalEvent::Cancelled);
                State::Ground
            }
            0x1b => {
                let new_length = length.saturating_add(1);
                State::ControlStringEscape {
                    kind,
                    length: new_length,
                    overflowed: overflowed || new_length > MAX_CONTROL_STRING_BYTES,
                }
            }
            _ => {
                let new_length = length.saturating_add(2);
                State::ControlString {
                    kind,
                    length: new_length,
                    overflowed: overflowed || new_length > MAX_CONTROL_STRING_BYTES,
                }
            }
        }
    }
}

fn finish_control_string(
    kind: ControlStringKind,
    overflowed: bool,
    events: &mut Vec<TerminalEvent>,
) {
    if overflowed {
        events.push(TerminalEvent::Malformed(
            SequenceError::ControlStringTooLong,
        ));
    } else {
        events.push(TerminalEvent::UnsupportedControlString(kind));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ControlStringKind, ParserError, SequenceError, TerminalEvent, VtParser,
        MAX_CONTROL_STRING_BYTES, MAX_CSI_PARAMETER_BYTES, MAX_INPUT_CHUNK_BYTES,
    };

    #[test]
    fn forwards_text_and_executes_c0_controls() {
        let mut parser = VtParser::new();
        let events = parser.process(b"hi\r\n\t!").expect("bounded input chunk");
        assert_eq!(
            events,
            vec![
                TerminalEvent::PrintByte(b'h'),
                TerminalEvent::PrintByte(b'i'),
                TerminalEvent::Execute(b'\r'),
                TerminalEvent::Execute(b'\n'),
                TerminalEvent::Execute(b'\t'),
                TerminalEvent::PrintByte(b'!'),
            ]
        );
        assert!(parser.is_ground());
    }

    #[test]
    fn parses_csi_incrementally_across_input_chunks() {
        let mut parser = VtParser::new();
        assert!(parser.process(b"\x1b[31;").expect("first chunk").is_empty());
        let events = parser.process(b"1m").expect("second chunk");
        let [TerminalEvent::Csi(sequence)] = events.as_slice() else {
            panic!("expected one CSI event: {events:?}");
        };
        assert_eq!(sequence.parameter_bytes(), b"31;1");
        assert_eq!(sequence.final_byte(), b'm');
        assert!(sequence.intermediates().is_empty());
        assert!(parser.is_ground());
    }

    #[test]
    fn preserves_private_parameters_and_intermediates() {
        let mut parser = VtParser::new();
        let events = parser.process(b"\x1b[?25;1$y").expect("private CSI");
        let [TerminalEvent::Csi(sequence)] = events.as_slice() else {
            panic!("expected CSI event: {events:?}");
        };
        assert_eq!(sequence.parameter_bytes(), b"?25;1");
        assert_eq!(sequence.intermediates(), b"$");
        assert_eq!(sequence.final_byte(), b'y');
    }

    #[test]
    fn parses_escape_sequences_with_split_intermediates() {
        let mut parser = VtParser::new();
        assert!(parser.process(b"\x1b(").expect("escape prefix").is_empty());
        let events = parser.process(b"B").expect("escape final");
        let [TerminalEvent::Escape(sequence)] = events.as_slice() else {
            panic!("expected ESC event: {events:?}");
        };
        assert_eq!(sequence.intermediates(), b"(");
        assert_eq!(sequence.final_byte(), b'B');
    }

    #[test]
    fn escape_final_bytes_are_not_reinterpreted_after_intermediates() {
        let mut parser = VtParser::new();
        let events = parser.process(b"\x1b([ ").expect("ESC intermediate");
        assert_eq!(
            events,
            vec![
                TerminalEvent::Escape(super::EscapeSequence {
                    intermediates: [b'(', 0],
                    intermediate_len: 1,
                    final_byte: b'[',
                }),
                TerminalEvent::PrintByte(b' ')
            ]
        );
    }

    #[test]
    fn discards_osc_on_bel_and_dcs_on_split_st() {
        let mut parser = VtParser::new();
        let osc = parser.process(b"\x1b]0;title\x07").expect("OSC");
        assert_eq!(
            osc,
            vec![TerminalEvent::UnsupportedControlString(
                ControlStringKind::Osc
            )]
        );

        assert!(parser
            .process(b"\x1bPpayload\x1b")
            .expect("DCS prefix")
            .is_empty());
        let dcs = parser.process(b"\\").expect("DCS terminator");
        assert_eq!(
            dcs,
            vec![TerminalEvent::UnsupportedControlString(
                ControlStringKind::Dcs
            )]
        );
    }

    #[test]
    fn osc_bel_terminates_after_an_escape_payload_byte() {
        let mut parser = VtParser::new();
        assert_eq!(
            parser.process(b"\x1b]title\x1b\x07").expect("OSC"),
            vec![TerminalEvent::UnsupportedControlString(
                ControlStringKind::Osc
            )]
        );
        assert!(parser.is_ground());
    }

    #[test]
    fn overlong_escape_is_ignored_until_final_then_recovers() {
        let mut parser = VtParser::new();
        let events = parser.process(b"\x1b( #AZ").expect("bounded input");
        assert_eq!(
            events,
            vec![
                TerminalEvent::Malformed(SequenceError::EscapeTooLong),
                TerminalEvent::PrintByte(b'Z')
            ]
        );
        assert!(parser.is_ground());
    }

    #[test]
    fn control_strings_are_bounded_and_recover_after_terminator() {
        let mut parser = VtParser::new();
        let mut input = Vec::with_capacity(MAX_CONTROL_STRING_BYTES + 4);
        input.extend_from_slice(b"\x1b]x");
        input.resize(MAX_CONTROL_STRING_BYTES + 4, b'x');
        input.push(0x07);
        let chunks = input.chunks(MAX_INPUT_CHUNK_BYTES);
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(parser.process(chunk).expect("bounded chunk"));
        }
        assert_eq!(
            events.last(),
            Some(&TerminalEvent::Malformed(
                SequenceError::ControlStringTooLong
            ))
        );
        assert!(parser.is_ground());
        assert_eq!(
            parser.process(b"Z").expect("parser recovered"),
            vec![TerminalEvent::PrintByte(b'Z')]
        );
    }

    #[test]
    fn overlong_csi_is_ignored_until_final_then_recovers() {
        let mut parser = VtParser::new();
        let mut input = Vec::with_capacity(MAX_CSI_PARAMETER_BYTES + 3);
        input.extend_from_slice(b"\x1b[");
        input.resize(MAX_CSI_PARAMETER_BYTES + 3, b'1');
        input.push(b'm');
        let events = parser.process(&input).expect("input under chunk limit");
        assert_eq!(
            events.first(),
            Some(&TerminalEvent::Malformed(SequenceError::CsiTooLong))
        );
        assert!(parser.is_ground());
        assert_eq!(
            parser.process(b"X").expect("parser recovered"),
            vec![TerminalEvent::PrintByte(b'X')]
        );
    }

    #[test]
    fn can_and_sub_cancel_incomplete_sequences() {
        let mut parser = VtParser::new();
        assert!(parser.process(b"\x1b[31").expect("CSI prefix").is_empty());
        assert_eq!(
            parser.process(b"\x18").expect("CAN"),
            vec![TerminalEvent::Cancelled]
        );
        assert!(parser.is_ground());

        assert!(parser
            .process(b"\x1b]unfinished")
            .expect("OSC prefix")
            .is_empty());
        assert_eq!(
            parser.process(b"\x1a").expect("SUB"),
            vec![TerminalEvent::Cancelled]
        );
        assert!(parser.is_ground());
    }

    #[test]
    fn oversized_chunk_is_rejected_without_changing_state() {
        let mut parser = VtParser::new();
        parser.process(b"\x1b[").expect("CSI prefix");
        let too_large = vec![b'x'; MAX_INPUT_CHUNK_BYTES + 1];
        assert_eq!(
            parser.process(&too_large),
            Err(ParserError::InputChunkTooLarge {
                actual: MAX_INPUT_CHUNK_BYTES + 1,
                maximum: MAX_INPUT_CHUNK_BYTES,
            })
        );
        let events = parser.process(b"31m").expect("state preserved");
        let [TerminalEvent::Csi(sequence)] = events.as_slice() else {
            panic!("expected preserved CSI sequence: {events:?}");
        };
        assert_eq!(sequence.parameter_bytes(), b"31");
        assert_eq!(sequence.final_byte(), b'm');
    }

    #[test]
    fn deterministic_fuzz_input_never_panics_and_is_bounded() {
        let mut parser = VtParser::new();
        let mut state = 0x1234_5678u32;
        for _ in 0..2048 {
            let mut chunk = [0u8; 64];
            for byte in &mut chunk {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            let events = parser.process(&chunk).expect("chunk is bounded");
            assert!(events.len() <= chunk.len());
        }

        parser.reset();
        assert!(parser.is_ground());
        assert_eq!(
            parser.process(b"ok").expect("parser reset"),
            vec![
                TerminalEvent::PrintByte(b'o'),
                TerminalEvent::PrintByte(b'k')
            ]
        );
    }
}
