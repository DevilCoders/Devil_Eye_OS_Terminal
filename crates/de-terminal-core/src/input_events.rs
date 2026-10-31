//! Session-scoped paste, mouse, and focus input mapping.
//!
//! An InputEventMapper instance belongs to exactly one terminal session. The
//! session layer updates its modes from that session's terminal DEC private-mode
//! changes and routes returned bytes only to that session's bounded input
//! queue. This module performs no I/O.

use std::error::Error;
use std::fmt;

use crate::input::{InputResult, Modifiers};
use crate::parser::CsiSequence;
use crate::TerminalSize;

const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";
const MAX_DEC_PRIVATE_PARAMETERS: usize = 32;
const LEGACY_MOUSE_MAX_COORDINATE: u16 = 223;

/// Maximum paste size used by the default policy (1 MiB).
pub const DEFAULT_MAX_PASTE_BYTES: usize = 1024 * 1024;

/// Policy for multi-line paste. The UI must obtain user confirmation before
/// selecting Allow when the default RequireConfirmation policy is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultilinePastePolicy {
    RequireConfirmation,
    Allow,
}

/// Bounds and safety policy for text pasted into a terminal session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PastePolicy {
    max_bytes: usize,
    multiline: MultilinePastePolicy,
}

impl PastePolicy {
    pub const fn new(max_bytes: usize, multiline: MultilinePastePolicy) -> Self {
        Self {
            max_bytes,
            multiline,
        }
    }

    pub const fn max_bytes(self) -> usize {
        self.max_bytes
    }

    pub const fn multiline(self) -> MultilinePastePolicy {
        self.multiline
    }
}

impl Default for PastePolicy {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_PASTE_BYTES,
            MultilinePastePolicy::RequireConfirmation,
        )
    }
}

/// Why a paste request was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteError {
    TooLarge { actual: usize, maximum: usize },
    MultilineConfirmationRequired,
    DisallowedControlCharacter { codepoint: u32, byte_offset: usize },
    SizeOverflow,
    AllocationFailed,
}

impl fmt::Display for PasteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "paste of {actual} bytes exceeds limit of {maximum}"
                )
            }
            Self::MultilineConfirmationRequired => {
                formatter.write_str("multi-line paste requires explicit confirmation")
            }
            Self::DisallowedControlCharacter {
                codepoint,
                byte_offset,
            } => write!(
                formatter,
                "paste contains disallowed control U+{codepoint:04X} at byte offset {byte_offset}"
            ),
            Self::SizeOverflow => formatter.write_str("paste output size overflowed"),
            Self::AllocationFailed => formatter.write_str("could not allocate paste buffer"),
        }
    }
}

impl Error for PasteError {}

/// Xterm mouse tracking modes. Higher modes include the events of lower modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseTrackingMode {
    #[default]
    Disabled,
    /// DECSET 9: button presses only.
    X10,
    /// DECSET 1000: button press and release.
    Normal,
    /// DECSET 1002: button press/release and motion while a button is held.
    ButtonMotion,
    /// DECSET 1003: all pointer motion.
    AnyMotion,
}

/// Mouse wire encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseEncoding {
    /// Legacy X10 byte encoding (coordinates are limited to 223 cells).
    #[default]
    Legacy,
    /// SGR extended encoding (1-based decimal coordinates).
    Sgr,
}

/// Effective terminal input modes relevant to paste, mouse, and focus events.
///
/// The flags are independent because terminals may enable several DEC private
/// modes simultaneously. mouse_tracking() resolves their effective priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputEventModes {
    x10_mouse: bool,
    normal_mouse: bool,
    button_motion_mouse: bool,
    any_motion_mouse: bool,
    sgr_mouse: bool,
    bracketed_paste: bool,
    focus_reporting: bool,
}

impl InputEventModes {
    pub const fn mouse_tracking(self) -> MouseTrackingMode {
        if self.any_motion_mouse {
            MouseTrackingMode::AnyMotion
        } else if self.button_motion_mouse {
            MouseTrackingMode::ButtonMotion
        } else if self.normal_mouse {
            MouseTrackingMode::Normal
        } else if self.x10_mouse {
            MouseTrackingMode::X10
        } else {
            MouseTrackingMode::Disabled
        }
    }

    pub const fn mouse_encoding(self) -> MouseEncoding {
        if self.sgr_mouse {
            MouseEncoding::Sgr
        } else {
            MouseEncoding::Legacy
        }
    }

    pub const fn bracketed_paste(self) -> bool {
        self.bracketed_paste
    }

    pub const fn focus_reporting(self) -> bool {
        self.focus_reporting
    }

    /// Applies one DEC private mode. Returns true for modes handled here.
    pub fn set_dec_private_mode(&mut self, mode: u16, enabled: bool) -> bool {
        let target = match mode {
            9 => &mut self.x10_mouse,
            1000 => &mut self.normal_mouse,
            1002 => &mut self.button_motion_mouse,
            1003 => &mut self.any_motion_mouse,
            1004 => &mut self.focus_reporting,
            1006 => &mut self.sgr_mouse,
            2004 => &mut self.bracketed_paste,
            _ => return false,
        };
        *target = enabled;
        true
    }

    /// Applies DECSET/DECRST sequences such as CSI ? 1000 ; 1006 h.
    ///
    /// Unsupported private modes are ignored. A malformed parameter list is
    /// rejected atomically: no mode is changed.
    pub fn apply_csi_sequence(&mut self, sequence: &CsiSequence) -> Result<bool, InputModeError> {
        if !sequence.intermediates().is_empty() || !matches!(sequence.final_byte(), b'h' | b'l') {
            return Ok(false);
        }

        let raw = sequence.parameter_bytes();
        let Some(parameters) = raw.strip_prefix(b"?") else {
            return Ok(false);
        };
        if parameters.is_empty() {
            return Err(InputModeError::EmptyParameterList);
        }

        let mut values = [0u16; MAX_DEC_PRIVATE_PARAMETERS];
        let mut count = 0usize;
        for parameter in parameters.split(|byte| *byte == b';') {
            if parameter.is_empty() {
                return Err(InputModeError::InvalidParameter);
            }
            if count == values.len() {
                return Err(InputModeError::TooManyParameters);
            }

            let mut value = 0u16;
            for byte in parameter {
                if !byte.is_ascii_digit() {
                    return Err(InputModeError::InvalidParameter);
                }
                value = value
                    .checked_mul(10)
                    .and_then(|current| current.checked_add(u16::from(*byte - b'0')))
                    .ok_or(InputModeError::ParameterOutOfRange)?;
            }
            values[count] = value;
            count += 1;
        }

        let enabled = sequence.final_byte() == b'h';
        let mut handled = false;
        for mode in &values[..count] {
            handled |= self.set_dec_private_mode(*mode, enabled);
        }
        Ok(handled)
    }
}

/// Why a DEC private-mode update could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputModeError {
    EmptyParameterList,
    InvalidParameter,
    ParameterOutOfRange,
    TooManyParameters,
}

impl fmt::Display for InputModeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyParameterList => formatter.write_str("DEC private-mode list is empty"),
            Self::InvalidParameter => formatter.write_str("DEC private-mode parameter is invalid"),
            Self::ParameterOutOfRange => {
                formatter.write_str("DEC private-mode parameter exceeds u16")
            }
            Self::TooManyParameters => formatter.write_str("too many DEC private-mode parameters"),
        }
    }
}

impl Error for InputModeError {}

/// Zero-based terminal cell coordinate supplied by the GUI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MousePosition {
    row: u16,
    column: u16,
}

impl MousePosition {
    pub const fn new(row: u16, column: u16) -> Self {
        Self { row, column }
    }

    pub const fn row(self) -> u16 {
        self.row
    }

    pub const fn column(self) -> u16 {
        self.column
    }

    const fn is_within(self, size: TerminalSize) -> bool {
        self.row < size.rows() && self.column < size.columns()
    }
}

/// Physical mouse button represented by the xterm protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// Wheel direction represented by the xterm mouse protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Mouse event before terminal protocol encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    Press(MouseButton),
    Release(MouseButton),
    /// held_button is the button currently held, if any.
    Move {
        held_button: Option<MouseButton>,
    },
    Wheel(WheelDirection),
}

/// GUI mouse event in terminal-cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub position: MousePosition,
    pub modifiers: Modifiers,
}

impl MouseEvent {
    pub const fn new(kind: MouseEventKind, position: MousePosition, modifiers: Modifiers) -> Self {
        Self {
            kind,
            position,
            modifiers,
        }
    }

    pub const fn press(button: MouseButton, position: MousePosition, modifiers: Modifiers) -> Self {
        Self::new(MouseEventKind::Press(button), position, modifiers)
    }

    pub const fn release(
        button: MouseButton,
        position: MousePosition,
        modifiers: Modifiers,
    ) -> Self {
        Self::new(MouseEventKind::Release(button), position, modifiers)
    }

    pub const fn move_event(
        held_button: Option<MouseButton>,
        position: MousePosition,
        modifiers: Modifiers,
    ) -> Self {
        Self::new(MouseEventKind::Move { held_button }, position, modifiers)
    }

    pub const fn wheel(
        direction: WheelDirection,
        position: MousePosition,
        modifiers: Modifiers,
    ) -> Self {
        Self::new(MouseEventKind::Wheel(direction), position, modifiers)
    }
}

/// Why a mouse event could not be encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMappingError {
    PositionOutOfBounds,
    LegacyCoordinateOutOfRange,
    AllocationFailed,
}

impl fmt::Display for MouseMappingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PositionOutOfBounds => {
                formatter.write_str("mouse position is outside the terminal grid")
            }
            Self::LegacyCoordinateOutOfRange => {
                formatter.write_str("legacy mouse encoding supports coordinates up to 223")
            }
            Self::AllocationFailed => formatter.write_str("could not allocate mouse event buffer"),
        }
    }
}

impl Error for MouseMappingError {}

/// Focus transition delivered by the GUI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusEvent {
    Gained,
    Lost,
}

/// Session-scoped mapper for paste, mouse, and focus events.
///
/// Keep one instance per terminal session. Its mode state must be updated from
/// that session's terminal mode events; never share it between tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InputEventMapper {
    modes: InputEventModes,
}

impl InputEventMapper {
    pub const fn new(modes: InputEventModes) -> Self {
        Self { modes }
    }

    pub const fn modes(&self) -> InputEventModes {
        self.modes
    }

    pub fn set_modes(&mut self, modes: InputEventModes) {
        self.modes = modes;
    }

    pub fn apply_terminal_mode_sequence(
        &mut self,
        sequence: &CsiSequence,
    ) -> Result<bool, InputModeError> {
        self.modes.apply_csi_sequence(sequence)
    }

    /// Maps pasted text. By default, multi-line input requires UI confirmation.
    ///
    /// NUL, ESC, DEL, and other control characters are rejected; CR, LF, and
    /// TAB are retained verbatim. The caller chooses an Allow policy only
    /// after any required confirmation has been obtained.
    pub fn map_paste(&self, text: &str, policy: PastePolicy) -> Result<InputResult, PasteError> {
        if text.is_empty() {
            return Ok(InputResult::Ignored);
        }
        if text.len() > policy.max_bytes {
            return Err(PasteError::TooLarge {
                actual: text.len(),
                maximum: policy.max_bytes,
            });
        }

        for (byte_offset, character) in text.char_indices() {
            if character.is_control() && !matches!(character, '\r' | '\n' | '\t') {
                return Err(PasteError::DisallowedControlCharacter {
                    codepoint: u32::from(character),
                    byte_offset,
                });
            }
        }

        let is_multiline = text.contains('\n') || text.contains('\r');
        if is_multiline && policy.multiline == MultilinePastePolicy::RequireConfirmation {
            return Err(PasteError::MultilineConfirmationRequired);
        }

        let wrapper_length = if self.modes.bracketed_paste { 12 } else { 0 };
        let capacity = text
            .len()
            .checked_add(wrapper_length)
            .ok_or(PasteError::SizeOverflow)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| PasteError::AllocationFailed)?;

        if self.modes.bracketed_paste {
            bytes.extend_from_slice(BRACKETED_PASTE_START);
        }
        bytes.extend_from_slice(text.as_bytes());
        if self.modes.bracketed_paste {
            bytes.extend_from_slice(BRACKETED_PASTE_END);
        }
        Ok(InputResult::Bytes(bytes))
    }

    /// Maps a pointer event according to this session's active DEC mouse modes.
    pub fn map_mouse(
        &self,
        event: MouseEvent,
        size: TerminalSize,
    ) -> Result<InputResult, MouseMappingError> {
        let tracking = self.modes.mouse_tracking();
        if tracking == MouseTrackingMode::Disabled {
            return Ok(InputResult::Ignored);
        }
        if !event.position.is_within(size) {
            return Err(MouseMappingError::PositionOutOfBounds);
        }
        if event.modifiers.super_key() {
            return Ok(InputResult::Ignored);
        }

        let (base_code, is_motion, is_release) = match event.kind {
            MouseEventKind::Press(button) => (button_code(button), false, false),
            MouseEventKind::Release(button) => {
                if tracking == MouseTrackingMode::X10 {
                    return Ok(InputResult::Ignored);
                }
                (button_code(button), false, true)
            }
            MouseEventKind::Move { held_button } => {
                let button = match (tracking, held_button) {
                    (MouseTrackingMode::AnyMotion, Some(button))
                    | (MouseTrackingMode::ButtonMotion, Some(button)) => button_code(button),
                    (MouseTrackingMode::AnyMotion, None) => 3,
                    _ => return Ok(InputResult::Ignored),
                };
                (button, true, false)
            }
            MouseEventKind::Wheel(direction) => (wheel_code(direction), false, false),
        };

        let modifier_bits = (u8::from(event.modifiers.shift()) * 4)
            | (u8::from(event.modifiers.alt()) * 8)
            | (u8::from(event.modifiers.control()) * 16);
        let motion_bit = if is_motion { 32 } else { 0 };
        let code = base_code + modifier_bits + motion_bit;
        let x = u32::from(event.position.column) + 1;
        let y = u32::from(event.position.row) + 1;

        match self.modes.mouse_encoding() {
            MouseEncoding::Sgr => {
                let final_byte = if is_release { 'm' } else { 'M' };
                Ok(InputResult::Bytes(
                    format!("\x1b[<{code};{x};{y}{final_byte}").into_bytes(),
                ))
            }
            MouseEncoding::Legacy => {
                if x > u32::from(LEGACY_MOUSE_MAX_COORDINATE)
                    || y > u32::from(LEGACY_MOUSE_MAX_COORDINATE)
                {
                    return Err(MouseMappingError::LegacyCoordinateOutOfRange);
                }
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(6)
                    .map_err(|_| MouseMappingError::AllocationFailed)?;
                bytes.extend_from_slice(b"\x1b[M");
                let legacy_code = if is_release { 3 + modifier_bits } else { code };
                bytes.push(legacy_code + 32);
                bytes.push(x as u8 + 32);
                bytes.push(y as u8 + 32);
                Ok(InputResult::Bytes(bytes))
            }
        }
    }

    /// Emits xterm focus-in/focus-out only while DECSET 1004 is active.
    pub fn map_focus(&self, event: FocusEvent) -> InputResult {
        if !self.modes.focus_reporting {
            return InputResult::Ignored;
        }
        match event {
            FocusEvent::Gained => InputResult::Bytes(b"\x1b[I".to_vec()),
            FocusEvent::Lost => InputResult::Bytes(b"\x1b[O".to_vec()),
        }
    }
}

const fn button_code(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

const fn wheel_code(direction: WheelDirection) -> u8 {
    match direction {
        WheelDirection::Up => 64,
        WheelDirection::Down => 65,
        WheelDirection::Left => 66,
        WheelDirection::Right => 67,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FocusEvent, InputEventMapper, InputEventModes, InputModeError, MouseButton, MouseEncoding,
        MouseEvent, MouseMappingError, MousePosition, MouseTrackingMode, MultilinePastePolicy,
        PasteError, PastePolicy, WheelDirection,
    };
    use crate::input::{InputResult, Modifiers};
    use crate::parser::{TerminalEvent, VtParser};
    use crate::TerminalSize;

    fn size(columns: u16, rows: u16) -> TerminalSize {
        TerminalSize::new(columns, rows).expect("valid terminal dimensions")
    }

    fn csi(input: &[u8]) -> crate::parser::CsiSequence {
        let mut parser = VtParser::new();
        let events = parser.process(input).expect("bounded CSI input");
        let [TerminalEvent::Csi(sequence)] = events.as_slice() else {
            panic!("expected one CSI event, got {events:?}");
        };
        *sequence
    }

    #[test]
    fn applies_dec_private_modes_and_resets_them_independently() {
        let mut mapper = InputEventMapper::default();
        assert!(mapper
            .apply_terminal_mode_sequence(&csi(b"\x1b[?1000;1006;1004;2004h"))
            .expect("valid DECSET"));
        assert_eq!(mapper.modes().mouse_tracking(), MouseTrackingMode::Normal);
        assert_eq!(mapper.modes().mouse_encoding(), MouseEncoding::Sgr);
        assert!(mapper.modes().focus_reporting());
        assert!(mapper.modes().bracketed_paste());

        mapper
            .apply_terminal_mode_sequence(&csi(b"\x1b[?1000;1006l"))
            .expect("valid DECRST");
        assert_eq!(mapper.modes().mouse_tracking(), MouseTrackingMode::Disabled);
        assert_eq!(mapper.modes().mouse_encoding(), MouseEncoding::Legacy);
        assert!(mapper.modes().focus_reporting());
        assert!(mapper.modes().bracketed_paste());
    }

    #[test]
    fn malformed_dec_private_modes_do_not_partially_update_state() {
        let mut mapper = InputEventMapper::default();
        assert_eq!(
            mapper.apply_terminal_mode_sequence(&csi(b"\x1b[?1000;;2004h")),
            Err(InputModeError::InvalidParameter)
        );
        assert_eq!(mapper.modes(), InputEventModes::default());
        assert_eq!(
            mapper.apply_terminal_mode_sequence(&csi(b"\x1b[?999999999999h")),
            Err(InputModeError::ParameterOutOfRange)
        );
    }

    #[test]
    fn tracking_mode_precedence_is_any_then_button_then_normal_then_x10() {
        let mut modes = InputEventModes::default();
        modes.set_dec_private_mode(9, true);
        modes.set_dec_private_mode(1000, true);
        assert_eq!(modes.mouse_tracking(), MouseTrackingMode::Normal);
        modes.set_dec_private_mode(1002, true);
        assert_eq!(modes.mouse_tracking(), MouseTrackingMode::ButtonMotion);
        modes.set_dec_private_mode(1003, true);
        assert_eq!(modes.mouse_tracking(), MouseTrackingMode::AnyMotion);
        modes.set_dec_private_mode(1003, false);
        assert_eq!(modes.mouse_tracking(), MouseTrackingMode::ButtonMotion);
    }

    #[test]
    fn paste_is_bounded_rejects_controls_and_requires_multiline_confirmation() {
        let mapper = InputEventMapper::default();
        let policy = PastePolicy::default();
        assert_eq!(
            mapper.map_paste("one\ntwo", policy),
            Err(PasteError::MultilineConfirmationRequired)
        );
        assert_eq!(
            mapper.map_paste("12345", PastePolicy::new(4, MultilinePastePolicy::Allow)),
            Err(PasteError::TooLarge {
                actual: 5,
                maximum: 4
            })
        );
        assert_eq!(
            mapper.map_paste("bad\x1bsequence", policy),
            Err(PasteError::DisallowedControlCharacter {
                codepoint: 0x1b,
                byte_offset: 3
            })
        );
        assert_eq!(
            mapper.map_paste("nul\0byte", policy),
            Err(PasteError::DisallowedControlCharacter {
                codepoint: 0,
                byte_offset: 3
            })
        );
    }

    #[test]
    fn bracketed_paste_wraps_confirmed_multiline_text_without_mutating_it() {
        let mut modes = InputEventModes::default();
        modes.set_dec_private_mode(2004, true);
        let mapper = InputEventMapper::new(modes);
        assert_eq!(
            mapper.map_paste(
                "first\r\nsecond\tline",
                PastePolicy::new(128, MultilinePastePolicy::Allow)
            ),
            Ok(InputResult::Bytes(
                b"\x1b[200~first\r\nsecond\tline\x1b[201~".to_vec()
            ))
        );
    }

    #[test]
    fn paste_without_bracketed_mode_is_plain_utf8() {
        let mapper = InputEventMapper::default();
        assert_eq!(
            mapper.map_paste("Привет 🦊", PastePolicy::default()),
            Ok(InputResult::Bytes("Привет 🦊".as_bytes().to_vec()))
        );
    }

    #[test]
    fn x10_and_normal_mouse_modes_filter_events_correctly() {
        let mut modes = InputEventModes::default();
        modes.set_dec_private_mode(9, true);
        let mapper = InputEventMapper::new(modes);
        let position = MousePosition::new(1, 2);
        assert_eq!(
            mapper.map_mouse(
                MouseEvent::press(MouseButton::Left, position, Modifiers::NONE),
                size(80, 24)
            ),
            Ok(InputResult::Bytes(b"\x1b[M #\"".to_vec()))
        );
        assert_eq!(
            mapper.map_mouse(
                MouseEvent::release(MouseButton::Left, position, Modifiers::NONE),
                size(80, 24)
            ),
            Ok(InputResult::Ignored)
        );

        modes.set_dec_private_mode(1000, true);
        let normal = InputEventMapper::new(modes);
        assert_eq!(
            normal.map_mouse(
                MouseEvent::release(MouseButton::Left, position, Modifiers::NONE),
                size(80, 24)
            ),
            Ok(InputResult::Bytes(b"\x1b[M##\"".to_vec()))
        );
    }

    #[test]
    fn button_motion_and_any_motion_use_expected_codes() {
        let position = MousePosition::new(0, 0);
        let mut modes = InputEventModes::default();
        modes.set_dec_private_mode(1002, true);
        let button = InputEventMapper::new(modes);
        assert_eq!(
            button.map_mouse(
                MouseEvent::move_event(Some(MouseButton::Right), position, Modifiers::NONE),
                size(80, 24)
            ),
            Ok(InputResult::Bytes(b"\x1b[MB!!".to_vec()))
        );
        assert_eq!(
            button.map_mouse(
                MouseEvent::move_event(None, position, Modifiers::NONE),
                size(80, 24)
            ),
            Ok(InputResult::Ignored)
        );

        modes.set_dec_private_mode(1003, true);
        let any = InputEventMapper::new(modes);
        assert_eq!(
            any.map_mouse(
                MouseEvent::move_event(None, position, Modifiers::NONE),
                size(80, 24)
            ),
            Ok(InputResult::Bytes(b"\x1b[MC!!".to_vec()))
        );
    }

    #[test]
    fn sgr_mouse_encodes_press_release_wheel_and_modifiers() {
        let mut modes = InputEventModes::default();
        modes.set_dec_private_mode(1000, true);
        modes.set_dec_private_mode(1006, true);
        let mapper = InputEventMapper::new(modes);
        let position = MousePosition::new(4, 9);
        assert_eq!(
            mapper.map_mouse(
                MouseEvent::press(
                    MouseButton::Middle,
                    position,
                    Modifiers::new(true, true, false, false)
                ),
                size(80, 24)
            ),
            Ok(InputResult::Bytes(b"\x1b[<21;10;5M".to_vec()))
        );
        assert_eq!(
            mapper.map_mouse(
                MouseEvent::release(MouseButton::Right, position, Modifiers::NONE),
                size(80, 24)
            ),
            Ok(InputResult::Bytes(b"\x1b[<2;10;5m".to_vec()))
        );
        assert_eq!(
            mapper.map_mouse(
                MouseEvent::wheel(WheelDirection::Up, position, Modifiers::NONE),
                size(80, 24)
            ),
            Ok(InputResult::Bytes(b"\x1b[<64;10;5M".to_vec()))
        );
    }

    #[test]
    fn mouse_rejects_out_of_bounds_and_unrepresentable_legacy_coordinates() {
        let mut modes = InputEventModes::default();
        modes.set_dec_private_mode(1000, true);
        let mapper = InputEventMapper::new(modes);
        assert_eq!(
            mapper.map_mouse(
                MouseEvent::press(
                    MouseButton::Left,
                    MousePosition::new(24, 0),
                    Modifiers::NONE
                ),
                size(80, 24)
            ),
            Err(MouseMappingError::PositionOutOfBounds)
        );

        assert_eq!(
            mapper.map_mouse(
                MouseEvent::press(
                    MouseButton::Left,
                    MousePosition::new(0, 223),
                    Modifiers::NONE
                ),
                size(300, 24)
            ),
            Err(MouseMappingError::LegacyCoordinateOutOfRange)
        );
    }

    #[test]
    fn focus_reporting_is_opt_in() {
        let mut modes = InputEventModes::default();
        let mapper = InputEventMapper::new(modes);
        assert_eq!(mapper.map_focus(FocusEvent::Gained), InputResult::Ignored);
        modes.set_dec_private_mode(1004, true);
        let mapper = InputEventMapper::new(modes);
        assert_eq!(
            mapper.map_focus(FocusEvent::Gained),
            InputResult::Bytes(b"\x1b[I".to_vec())
        );
        assert_eq!(
            mapper.map_focus(FocusEvent::Lost),
            InputResult::Bytes(b"\x1b[O".to_vec())
        );
    }

    #[test]
    fn separate_mapper_instances_keep_session_modes_isolated() {
        let mut first_modes = InputEventModes::default();
        first_modes.set_dec_private_mode(2004, true);
        let first = InputEventMapper::new(first_modes);
        let second = InputEventMapper::default();
        let policy = PastePolicy::new(32, MultilinePastePolicy::Allow);
        assert_eq!(
            first.map_paste("x", policy),
            Ok(InputResult::Bytes(b"\x1b[200~x\x1b[201~".to_vec()))
        );
        assert_eq!(
            second.map_paste("x", policy),
            Ok(InputResult::Bytes(b"x".to_vec()))
        );
    }
}
