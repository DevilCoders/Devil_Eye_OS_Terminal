//! Backend-independent keyboard, text-commit, and shortcut mapping.
//!
//! Printable text is supplied through text-commit events (after keyboard layout
//! and IME processing). Key events are used for control combinations,
//! navigation keys, and application shortcuts.

use std::collections::BTreeMap;

/// Physical/logical key identity supplied by the GUI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyCode {
    Character(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    Insert,
    Delete,
    PageUp,
    PageDown,
    Function(u8),
}

/// Modifier state at the time of a key or text event.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Modifiers {
    shift: bool,
    control: bool,
    alt: bool,
    super_key: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        control: false,
        alt: false,
        super_key: false,
    };

    pub const fn new(shift: bool, control: bool, alt: bool, super_key: bool) -> Self {
        Self {
            shift,
            control,
            alt,
            super_key,
        }
    }

    pub const fn shift(self) -> bool {
        self.shift
    }

    pub const fn control(self) -> bool {
        self.control
    }

    pub const fn alt(self) -> bool {
        self.alt
    }

    pub const fn super_key(self) -> bool {
        self.super_key
    }

    /// Xterm modifier parameter: 1 + Shift + 2*Alt + 4*Control + 8*Super.
    const fn xterm_parameter(self) -> u8 {
        1 + (self.shift as u8)
            + 2 * (self.alt as u8)
            + 4 * (self.control as u8)
            + 8 * (self.super_key as u8)
    }

    const fn is_empty(self) -> bool {
        !self.shift && !self.control && !self.alt && !self.super_key
    }
}

/// Key transition. Releases are ignored by the terminal byte mapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

/// A GUI key event before terminal protocol encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: Modifiers,
    pub state: KeyState,
}

impl KeyEvent {
    pub const fn pressed(code: KeyCode, modifiers: Modifiers) -> Self {
        Self {
            code,
            modifiers,
            state: KeyState::Pressed,
        }
    }

    pub const fn released(code: KeyCode, modifiers: Modifiers) -> Self {
        Self {
            code,
            modifiers,
            state: KeyState::Released,
        }
    }
}

/// Configurable application-level shortcut action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    Copy,
    Paste,
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    SplitHorizontal,
    SplitVertical,
}

/// A key plus exact modifier chord used to resolve an application shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub modifiers: Modifiers,
}

impl KeyChord {
    pub const fn new(code: KeyCode, modifiers: Modifiers) -> Self {
        Self { code, modifiers }
    }
}

/// User-configurable shortcut bindings. Binding an existing chord replaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutMap {
    bindings: BTreeMap<KeyChord, ShortcutAction>,
}

impl Default for ShortcutMap {
    fn default() -> Self {
        let mut map = Self {
            bindings: BTreeMap::new(),
        };
        let ctrl_shift = Modifiers::new(true, true, false, false);
        map.bind(
            KeyChord::new(KeyCode::Character('c'), ctrl_shift),
            ShortcutAction::Copy,
        );
        map.bind(
            KeyChord::new(KeyCode::Character('v'), ctrl_shift),
            ShortcutAction::Paste,
        );
        map.bind(
            KeyChord::new(KeyCode::Character('t'), ctrl_shift),
            ShortcutAction::NewTab,
        );
        map.bind(
            KeyChord::new(KeyCode::Character('w'), ctrl_shift),
            ShortcutAction::CloseTab,
        );
        map.bind(
            KeyChord::new(KeyCode::Tab, Modifiers::new(false, true, false, false)),
            ShortcutAction::NextTab,
        );
        map.bind(
            KeyChord::new(KeyCode::Tab, ctrl_shift),
            ShortcutAction::PreviousTab,
        );
        map
    }
}

impl ShortcutMap {
    pub fn bind(&mut self, chord: KeyChord, action: ShortcutAction) -> Option<ShortcutAction> {
        self.bindings.insert(chord, action)
    }

    pub fn unbind(&mut self, chord: KeyChord) -> Option<ShortcutAction> {
        self.bindings.remove(&chord)
    }

    pub fn get(&self, chord: KeyChord) -> Option<ShortcutAction> {
        self.bindings.get(&chord).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (KeyChord, ShortcutAction)> + '_ {
        self.bindings.iter().map(|(chord, action)| (*chord, *action))
    }
}

/// Terminal modes that affect keyboard protocol encoding.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardModes {
    application_cursor_keys: bool,
    backspace_sends_bs: bool,
}

impl KeyboardModes {
    pub const fn new(application_cursor_keys: bool, backspace_sends_bs: bool) -> Self {
        Self {
            application_cursor_keys,
            backspace_sends_bs,
        }
    }

    pub const fn application_cursor_keys(self) -> bool {
        self.application_cursor_keys
    }

    pub const fn backspace_sends_bs(self) -> bool {
        self.backspace_sends_bs
    }
}

/// Result of mapping a GUI event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputResult {
    /// Bytes to append to the owning session's bounded PTY input queue.
    Bytes(Vec<u8>),
    /// An application action; do not forward it to the child process.
    Shortcut(ShortcutAction),
    /// Key release, uncommitted IME composition, or unsupported key.
    Ignored,
}

/// Stateless protocol mapper plus mutable terminal mode and shortcut settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputMapper {
    modes: KeyboardModes,
    shortcuts: ShortcutMap,
}

impl Default for InputMapper {
    fn default() -> Self {
        Self::new(KeyboardModes::default(), ShortcutMap::default())
    }
}

impl InputMapper {
    pub const fn new(modes: KeyboardModes, shortcuts: ShortcutMap) -> Self {
        Self { modes, shortcuts }
    }

    pub const fn modes(&self) -> KeyboardModes {
        self.modes
    }

    pub fn set_modes(&mut self, modes: KeyboardModes) {
        self.modes = modes;
    }

    pub fn shortcuts(&self) -> &ShortcutMap {
        &self.shortcuts
    }

    pub fn shortcuts_mut(&mut self) -> &mut ShortcutMap {
        &mut self.shortcuts
    }

    /// Maps a key transition. Shortcut resolution precedes terminal encoding.
    pub fn map_key(&self, event: KeyEvent) -> InputResult {
        if event.state != KeyState::Pressed {
            return InputResult::Ignored;
        }

        let chord = KeyChord::new(event.code, event.modifiers);
        if let Some(action) = self.shortcuts.get(chord) {
            return InputResult::Shortcut(action);
        }

        if event.modifiers.super_key() {
            return InputResult::Ignored;
        }

        if let KeyCode::Character(character) = event.code {
            if event.modifiers.control() {
                return map_control_character(character, event.modifiers.alt());
            }
            // Printable key-down is not text. The GUI must deliver the
            // committed text separately after layout/IME processing.
            return InputResult::Ignored;
        }

        map_special_key(event.code, event.modifiers, self.modes)
    }

    /// Maps committed text after keyboard layout and IME composition.
    pub fn map_text_commit(&self, text: &str, modifiers: Modifiers) -> InputResult {
        if text.is_empty() || modifiers.control() || modifiers.super_key() {
            return InputResult::Ignored;
        }

        let mut bytes = Vec::with_capacity(text.len() + usize::from(modifiers.alt()));
        if modifiers.alt() {
            bytes.push(0x1b);
        }
        bytes.extend_from_slice(text.as_bytes());
        InputResult::Bytes(bytes)
    }

    /// Intermediate IME composition must never leak into the PTY stream.
    pub const fn map_composition_update(&self, _preedit: &str) -> InputResult {
        InputResult::Ignored
    }
}

fn map_control_character(character: char, alt: bool) -> InputResult {
    let upper = character.to_ascii_uppercase();
    let control_byte = match upper {
        '@'..='_' => (upper as u8) & 0x1f,
        '?' => 0x7f,
        _ => return InputResult::Ignored,
    };
    let mut bytes = Vec::with_capacity(if alt { 2 } else { 1 });
    if alt {
        bytes.push(0x1b);
    }
    bytes.push(control_byte);
    InputResult::Bytes(bytes)
}

fn map_special_key(code: KeyCode, modifiers: Modifiers, modes: KeyboardModes) -> InputResult {
    let modified = !modifiers.is_empty();
    let modifier = modifiers.xterm_parameter();
    let mut bytes = Vec::with_capacity(8);

    let sequence = match code {
        KeyCode::Enter => {
            bytes.push(b'\r');
            None
        }
        KeyCode::Tab if modifiers.shift() => Some(if modified {
            format!("\x1b[1;{modifier}Z")
        } else {
            "\x1b[Z".to_owned()
        }),
        KeyCode::Tab => {
            bytes.push(b'\t');
            None
        }
        KeyCode::Backspace => {
            bytes.push(if modes.backspace_sends_bs() { 0x08 } else { 0x7f });
            None
        }
        KeyCode::Escape => {
            bytes.push(0x1b);
            None
        }
        KeyCode::ArrowUp => Some(cursor_sequence(b'A', modifiers, modes)),
        KeyCode::ArrowDown => Some(cursor_sequence(b'B', modifiers, modes)),
        KeyCode::ArrowRight => Some(cursor_sequence(b'C', modifiers, modes)),
        KeyCode::ArrowLeft => Some(cursor_sequence(b'D', modifiers, modes)),
        KeyCode::Home => Some(cursor_sequence(b'H', modifiers, modes)),
        KeyCode::End => Some(cursor_sequence(b'F', modifiers, modes)),
        KeyCode::Insert => Some(tilde_sequence(2, modifier, modified)),
        KeyCode::Delete => Some(tilde_sequence(3, modifier, modified)),
        KeyCode::PageUp => Some(tilde_sequence(5, modifier, modified)),
        KeyCode::PageDown => Some(tilde_sequence(6, modifier, modified)),
        KeyCode::Function(number) => function_sequence(number, modifier, modified),
        KeyCode::Character(_) => None,
    };

    if let Some(sequence) = sequence {
        bytes.extend_from_slice(sequence.as_bytes());
    }
    if bytes.is_empty() {
        InputResult::Ignored
    } else {
        if modifiers.alt() && !matches!(code, KeyCode::Tab if modifiers.shift()) {
            bytes.insert(0, 0x1b);
        }
        InputResult::Bytes(bytes)
    }
}

fn cursor_sequence(final_byte: u8, modifiers: Modifiers, modes: KeyboardModes) -> String {
    if modifiers.is_empty() {
        if modes.application_cursor_keys() {
            format!("\x1bO{}", char::from(final_byte))
        } else {
            format!("\x1b[{}", char::from(final_byte))
        }
    } else {
        format!("\x1b[1;{}{}", modifiers.xterm_parameter(), char::from(final_byte))
    }
}

fn tilde_sequence(number: u8, modifier: u8, modified: bool) -> String {
    if modified {
        format!("\x1b[{number};{modifier}~")
    } else {
        format!("\x1b[{number}~")
    }
}

fn function_sequence(number: u8, modifier: u8, modified: bool) -> Option<String> {
    let (base, final_byte) = match number {
        1 => (1, b'P'),
        2 => (1, b'Q'),
        3 => (1, b'R'),
        4 => (1, b'S'),
        5 => (15, b'~'),
        6 => (17, b'~'),
        7 => (18, b'~'),
        8 => (19, b'~'),
        9 => (20, b'~'),
        10 => (21, b'~'),
        11 => (23, b'~'),
        12 => (24, b'~'),
        _ => return None,
    };

    if final_byte == b'~' {
        Some(tilde_sequence(base, modifier, modified))
    } else if modified {
        Some(format!("\x1b[1;{modifier}{}", char::from(final_byte)))
    } else {
        Some(format!("\x1bO{}", char::from(final_byte)))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InputMapper, InputResult, KeyChord, KeyCode, KeyEvent, KeyboardModes, Modifiers,
        ShortcutAction, ShortcutMap,
    };

    #[test]
    fn maps_navigation_keys_and_application_cursor_mode() {
        let mapper = InputMapper::default();
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::ArrowUp, Modifiers::NONE)),
            InputResult::Bytes(b"\x1b[A".to_vec())
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::Home, Modifiers::NONE)),
            InputResult::Bytes(b"\x1b[H".to_vec())
        );

        let application = InputMapper::new(
            KeyboardModes::new(true, false),
            ShortcutMap::default(),
        );
        assert_eq!(
            application.map_key(KeyEvent::pressed(KeyCode::ArrowUp, Modifiers::NONE)),
            InputResult::Bytes(b"\x1bOA".to_vec())
        );
    }

    #[test]
    fn maps_modified_navigation_and_function_keys() {
        let mapper = InputMapper::default();
        let ctrl_alt = Modifiers::new(false, true, true, false);
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::ArrowUp, ctrl_alt)),
            InputResult::Bytes(b"\x1b[1;7A".to_vec())
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::Function(5), Modifiers::NONE)),
            InputResult::Bytes(b"\x1b[15~".to_vec())
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(
                KeyCode::Function(1),
                Modifiers::new(true, false, false, false)
            )),
            InputResult::Bytes(b"\x1b[1;2P".to_vec())
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::Function(13), Modifiers::NONE)),
            InputResult::Ignored
        );
    }

    #[test]
    fn maps_backspace_tab_and_control_characters() {
        let mapper = InputMapper::default();
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::Backspace, Modifiers::NONE)),
            InputResult::Bytes(vec![0x7f])
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(
                KeyCode::Backspace,
                Modifiers::new(false, false, false, false)
            )),
            InputResult::Bytes(vec![0x7f])
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(
                KeyCode::Tab,
                Modifiers::new(true, false, false, false)
            )),
            InputResult::Bytes(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(
                KeyCode::Character('c'),
                Modifiers::new(false, true, false, false)
            )),
            InputResult::Bytes(vec![0x03])
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(
                KeyCode::Character(' '),
                Modifiers::new(false, true, false, false)
            )),
            InputResult::Bytes(vec![0x00])
        );
    }

    #[test]
    fn committed_unicode_text_is_utf8_and_preedit_is_not_forwarded() {
        let mapper = InputMapper::default();
        assert_eq!(
            mapper.map_text_commit("Привет 🦊", Modifiers::NONE),
            InputResult::Bytes("Привет 🦊".as_bytes().to_vec())
        );
        assert_eq!(
            mapper.map_text_commit("x", Modifiers::new(false, false, true, false)),
            InputResult::Bytes(b"\x1bx".to_vec())
        );
        assert_eq!(
            mapper.map_composition_update("uncommitted composition"),
            InputResult::Ignored
        );
        assert_eq!(
            mapper.map_text_commit("x", Modifiers::new(false, true, false, false)),
            InputResult::Ignored
        );
    }

    #[test]
    fn default_shortcuts_are_consumed_before_terminal_encoding() {
        let mapper = InputMapper::default();
        let ctrl_shift = Modifiers::new(true, true, false, false);
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::Character('c'), ctrl_shift)),
            InputResult::Shortcut(ShortcutAction::Copy)
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::Character('t'), ctrl_shift)),
            InputResult::Shortcut(ShortcutAction::NewTab)
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(
                KeyCode::Tab,
                Modifiers::new(false, true, false, false)
            )),
            InputResult::Shortcut(ShortcutAction::NextTab)
        );
    }

    #[test]
    fn custom_shortcuts_can_replace_and_remove_bindings() {
        let mut shortcuts = ShortcutMap::default();
        let chord = KeyChord::new(
            KeyCode::Character('t'),
            Modifiers::new(true, true, false, false),
        );
        assert_eq!(
            shortcuts.bind(chord, ShortcutAction::SplitVertical),
            Some(ShortcutAction::NewTab)
        );
        let mapper = InputMapper::new(KeyboardModes::default(), shortcuts.clone());
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(chord.code, chord.modifiers)),
            InputResult::Shortcut(ShortcutAction::SplitVertical)
        );
        assert_eq!(
            shortcuts.unbind(chord),
            Some(ShortcutAction::SplitVertical)
        );
    }

    #[test]
    fn ignores_key_releases_and_unhandled_super_combinations() {
        let mapper = InputMapper::default();
        assert_eq!(
            mapper.map_key(KeyEvent::released(KeyCode::ArrowUp, Modifiers::NONE)),
            InputResult::Ignored
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(
                KeyCode::Character('x'),
                Modifiers::new(false, false, false, true)
            )),
            InputResult::Ignored
        );
    }

    #[test]
    fn backspace_mode_changes_the_terminal_byte() {
        let mapper = InputMapper::new(
            KeyboardModes::new(false, true),
            ShortcutMap::default(),
        );
        assert_eq!(
            mapper.map_key(KeyEvent::pressed(KeyCode::Backspace, Modifiers::NONE)),
            InputResult::Bytes(vec![0x08])
        );
    }
}
