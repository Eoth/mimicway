// Tests of the test ports (test_ports.rs): distinct across threads, and never one the kernel hands out by itself.
use crate::test_ports::{FIRST, LAST, reserve_port};
use std::collections::HashSet;

#[test]
fn ports_reserved_from_several_threads_are_all_different() {
    let threads: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(|| (0..50).map(|_| reserve_port()).collect::<Vec<_>>()))
        .collect();
    let ports: Vec<u16> = threads
        .into_iter()
        .flat_map(|t| t.join().unwrap())
        .collect();
    let distinct: HashSet<_> = ports.iter().collect();
    assert_eq!(distinct.len(), ports.len());
}

// The kernel is the judge: while the reserved ports sit closed, it hands out thousands of ports of its own, to
// listeners on port 0 and to outgoing connections, and none of them may be a reserved one.
#[test]
fn the_kernel_never_hands_out_a_reserved_port() {
    let reserved: HashSet<u16> = (0..50).map(|_| reserve_port()).collect();
    let listeners: Vec<_> = (0..2_000)
        .map(|_| std::net::TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    // One connection per listener: none is accepted, so a single listener would fill its backlog and hang.
    let connections: Vec<_> = listeners[..500]
        .iter()
        .map(|l| std::net::TcpStream::connect(l.local_addr().unwrap()).unwrap())
        .collect();
    let given = listeners
        .iter()
        .map(|l| l.local_addr().unwrap().port())
        .chain(connections.iter().map(|c| c.local_addr().unwrap().port()));
    let clashes: Vec<u16> = given.filter(|p| reserved.contains(p)).collect();
    assert!(clashes.is_empty(), "the kernel handed out {clashes:?}");
}

#[cfg(target_os = "linux")]
#[test]
fn the_range_is_below_the_ephemeral_range_of_this_machine() {
    let range = std::fs::read_to_string("/proc/sys/net/ipv4/ip_local_port_range").unwrap();
    let low: u16 = range.split_whitespace().next().unwrap().parse().unwrap();
    assert!(
        LAST < low,
        "test ports {FIRST}..={LAST} overlap the ephemeral range from {low}"
    );
    let port = reserve_port();
    assert!((FIRST..=LAST).contains(&port), "{port}");
}
