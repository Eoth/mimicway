#![no_main]
// A response template rendered with a request's values. mimicway::fuzzing::template says what is checked.
libfuzzer_sys::fuzz_target!(|data: &[u8]| mimicway::fuzzing::template(data));
