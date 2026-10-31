#![no_main]

use de_terminal_core::parser::{VtParser, MAX_INPUT_CHUNK_BYTES};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    let mut parser = VtParser::new();
    let mut offset = 0usize;

    // Vary chunk boundaries so escape/CSI/control-string state is exercised
    // across arbitrary incremental reads as well as complete byte streams.
    while offset < input.len() {
        let chunk_hint = usize::from(input[offset] % 127) + 1;
        let chunk_length = chunk_hint.min(MAX_INPUT_CHUNK_BYTES);
        let end = offset.saturating_add(chunk_length).min(input.len());
        let _ = parser.process(&input[offset..end]);
        offset = end;
    }

    parser.reset();
    let _ = parser.process(&input[..input.len().min(MAX_INPUT_CHUNK_BYTES)]);
});
