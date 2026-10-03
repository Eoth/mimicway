#![no_main]
// A request against one condition per source: the rule tester and production must agree. mimicway::fuzzing::request_conditions says what is checked.
libfuzzer_sys::fuzz_target!(|data: &[u8]| mimicway::fuzzing::request_conditions(data));
