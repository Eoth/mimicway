#![no_main]
// Observed exchanges through the rule suggestions. mimicway::fuzzing::suggestions says what is checked.
libfuzzer_sys::fuzz_target!(|data: &[u8]| mimicway::fuzzing::suggestions(data));
