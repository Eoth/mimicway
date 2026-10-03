#![no_main]
// A rule's sub-path pattern against the rest of a request's path. mimicway::fuzzing::sub_path says what is checked.
libfuzzer_sys::fuzz_target!(|data: &[u8]| mimicway::fuzzing::sub_path(data));
