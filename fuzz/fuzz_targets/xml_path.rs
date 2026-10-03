#![no_main]
// An XML or SOAP body read through a slash-separated path. mimicway::fuzzing::xml_path says what is checked.
libfuzzer_sys::fuzz_target!(|data: &[u8]| mimicway::fuzzing::xml_path(data));
