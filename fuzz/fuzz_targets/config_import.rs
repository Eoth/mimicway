#![no_main]
// A configuration imported as JSON, checked, then written to YAML and read back unchanged. mimicway::fuzzing::config_import says what is checked.
libfuzzer_sys::fuzz_target!(|data: &[u8]| mimicway::fuzzing::config_import(data));
