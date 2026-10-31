# Keyboard and Text Input Mapping Contract

**Scope:** DE-010-01. This module defines the backend-independent conversion from GUI key/text events to terminal input bytes or application shortcut actions. It does not depend on a windowing toolkit.

## Event separation

The GUI adapter must distinguish:

- **Key press/release:** physical/logical key identity and modifier state. Used for navigation/control keys and shortcuts.
- **Text commit:** committed text after keyboard layout, dead-key processing, and IME composition. Used for printable text.
- **Composition update:** uncommitted IME preedit text. It is never sent to the PTY.

A printable key-down event is ignored by the mapper; the corresponding text is sent exactly once through the text-commit path. This avoids duplicate characters and preserves composed Unicode input. Key releases are ignored by the basic terminal protocol mapper.

## Mapping order

1. Resolve the exact configured application shortcut.
2. For control-character shortcuts, try the normalized ASCII key chord (so Shift+Ctrl+C matches the default Copy binding whether the GUI reports `c` or `C`).
3. Ignore unhandled Super/OS-level combinations.
4. Map control-character combinations and terminal special keys.
5. Send committed text separately as UTF-8 bytes.

A shortcut result is an application action and must not be written to the child PTY. A byte result is appended to the owning session's bounded input queue; partial writes and backpressure are handled by the runtime contract.

## Default shortcuts

| Chord | Action |
| --- | --- |
| Ctrl+Shift+C | Copy |
| Ctrl+Shift+V | Paste |
| Ctrl+Shift+T | New tab |
| Ctrl+Shift+W | Close tab |
| Ctrl+Tab | Next tab |
| Ctrl+Shift+Tab | Previous tab |

Bindings are stored in a replaceable map. A caller can bind a new chord, replace an existing action, or unbind it. Shortcut actions are declarative; the application/session layer performs the actual UI operation.

## Terminal key encoding

- Enter → CR.
- Tab → HT; Shift+Tab → CSI Z.
- Backspace → DEL by default, or BS when the backspace mode is enabled.
- Escape → ESC.
- Arrow/Home/End keys → CSI sequences, or SS3 cursor sequences when application-cursor mode is active.
- Insert/Delete/PageUp/PageDown → standard CSI tilde sequences.
- F1–F4 → SS3 sequences; F5–F12 → CSI tilde sequences.
- Shift/Alt/Control/Super modifiers on supported special keys use the xterm modifier parameter `1 + Shift + 2×Alt + 4×Control + 8×Super`.
- Alt on a key without modifier-parameter encoding prefixes ESC.
- Ctrl+ASCII maps to the corresponding C0 control byte; Ctrl+? maps to DEL.
- Committed text is UTF-8. Alt+text prefixes ESC. Control/Super-modified text commits are ignored to prevent duplicate or ambiguous input.

Unsupported function-key numbers and unsupported key combinations return `Ignored`; they do not emit guessed escape sequences.

## IME and Unicode behavior

The mapper does not attempt to translate keyboard scancodes into Unicode. The GUI toolkit owns keyboard layout and IME composition. Intermediate preedit updates are ignored. Once the IME commits text, the complete string is encoded as UTF-8 and sent once, preserving multi-codepoint grapheme sequences.

## Scope boundary and protocol limitations

This is the initial xterm-compatible key mapping contract. It does not yet implement:

- Kitty keyboard protocol or modifyOtherKeys negotiation.
- Numeric keypad application mode.
- Paste/bracketed-paste policy.
- Mouse reporting or focus reporting.
- Configurable terminal-specific key protocols beyond application-cursor and backspace modes.

Paste, mouse, and focus handling are separate in DE-010-02 (#54).

## Failure and ownership behavior

- Mapping is deterministic and does not perform I/O.
- The mapper owns only mode and shortcut configuration; it does not own a PTY or session.
- The caller owns returned byte buffers and must enqueue them with a per-session size limit.
- Shortcut actions are returned to the application and never implicitly executed by the mapper.
- Unsupported or uncommitted events are ignored; malformed GUI events must not cause a panic or unbounded allocation.

## Verification

Unit tests cover:

- Normal/application cursor mode and Home/End navigation.
- Modified navigation and function keys.
- Backspace, Tab, Shift+Tab, Ctrl+character, and Ctrl+Space.
- UTF-8 text commits, Alt prefixing, and ignored IME preedit.
- Default shortcut resolution, uppercase key normalization, replacement, and unbinding.
- Key releases, unsupported function keys, and unhandled Super combinations.
