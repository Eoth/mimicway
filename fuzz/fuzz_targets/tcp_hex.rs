#![no_main]
// The hexadecimal text of raw TCP mocks: encoding and decoding give the bytes back. mimicway::fuzzing::tcp_hex says what is checked.
libfuzzer_sys::fuzz_target!(|data: &[u8]| mimicway::fuzzing::tcp_hex(data));
