#![no_main]

use de_ffi::{ExecutionContext, Shell, MAX_SOURCE_BYTES};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &[u8]| {
    if input.len() > MAX_SOURCE_BYTES {
        return;
    }

    // A fresh C-owned shell per input prevents one fuzz case's built-in state
    // from influencing another. No external executor is installed, so fuzzed
    // input cannot launch host processes.
    let Ok(mut shell) = Shell::new() else {
        return;
    };
    let _ = shell.execute(input, &ExecutionContext::new(), None);
});
