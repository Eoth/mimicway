// Ports for tests that hand a port number to a server they do not bind themselves (a TCP mock, the real binary).
//
// A port read from a listener bound to port 0 and then closed goes back to the kernel's ephemeral range, where the
// next bind to port 0 or the next outgoing connection of any test can receive it, the next call of the same helper
// included: two of eight such ports were equal in about one run in 500, and a TCP mock that takes the port of another
// one is refused. These ports come from below every ephemeral range (32768 on Linux, 49152 on macOS, Windows and in
// the IANA range), which the kernel never hands out on its own; each is given once per process, and only after a bind
// on it succeeded, which skips a port another program holds.
//
// Not covered: a program that binds the same port explicitly between that check and the server's own bind.
//
// Shared by the library's tests and by tests/ (`#[path]`), so it depends on std only; its own tests are in
// test_ports_tests.rs, which only the library declares.
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) const FIRST: u16 = 20_000;
pub(crate) const LAST: u16 = 29_999;

static NEXT: AtomicU32 = AtomicU32::new(0);
static START: OnceLock<u32> = OnceLock::new();

/// A local port that no other caller of this process receives, free when it was checked, outside the kernel's
/// ephemeral range.
pub(crate) fn reserve_port() -> u16 {
    let span = u32::from(LAST - FIRST) + 1;
    // Two test processes at the same time (a local run beside the CI) start from different places of the range.
    let start = *START.get_or_init(|| std::process::id() % span);
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(n < span, "all {span} test ports were given out");
        let port = FIRST + ((start + n) % span) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}
