// The judges of `PROXY_ALLOWED_TARGETS` (decision 3).
//
// The rule (which entry admits which address) has no judge outside this code: it is restated here in another form,
// bit strings compared as text, and every list of a table is judged against it for every address of a table, each kind
// of address and its IPv4-mapped form, and for ranges drawn around their bounds.
//
// What an address written in a URL is, and where a name leads, is judged by the kernel and the system's resolver: each
// spelling drawn of a loopback or unspecified address, and each spelling of `localhost`, is sent through a relay
// without a list to a real listening socket; only those that reach it are kept, and with each list they must reach it
// again when the table admits their address, or be refused with no connection seen, by the relay, the availability
// check and the check of a saved target alike.
//
// What it does not see: a name that resolves to a link-local address (no such name on a test machine; the rule is
// judged on the addresses of the table, and a link-local address written in a URL is judged before any connection);
// IPv6 when the machine has no `::1` (that part says so and stops); redirects, which the relay never follows, judged
// in `the_relays_the_availability_check_and_saving_follow_the_list`.
use super::*;
use crate::engine::{ProxyClient, ProxyError};
use axum::body::Body;
use axum::http::Request;
use std::sync::atomic::{AtomicUsize, Ordering};

const SEED: u64 = 0x7072_6f78_7969_6573;

fn list(text: &str) -> AllowedTargets {
    AllowedTargets::parse(text).unwrap_or_else(|e| panic!("{text:?}: {e}"))
}

// ---------------------------------------------------------------------------------------------------------------
// Reading the list

#[test]
fn the_list_is_read_entry_by_entry() {
    let accepted = [
        ("", "none"),
        (" , ,", "none"),
        ("api.example.com", "api.example.com"),
        ("API.Example.COM.", "api.example.com"),
        ("*.example.com", "*.example.com"),
        ("*.Example.com.", "*.example.com"),
        ("localhost", "localhost"),
        ("b\u{fc}cher.example", "xn--bcher-kva.example"),
        ("10.0.0.5", "10.0.0.5/32"),
        ("127.1", "127.0.0.1/32"),
        ("0x7f.1", "127.0.0.1/32"),
        ("::1", "::1/128"),
        ("[::1]", "::1/128"),
        ("[fe80::]/10", "fe80::/10"),
        ("::ffff:10.0.0.1", "10.0.0.1/32"),
        ("::ffff:10.0.0.0/104", "10.0.0.0/8"),
        ("::ffff:0:0/96", "0.0.0.0/0"),
        ("::/0", "::/0"),
        ("0.0.0.0/0", "0.0.0.0/0"),
        ("10.0.0.0/8", "10.0.0.0/8"),
        ("10.0.0.0/08", "10.0.0.0/8"),
        ("192.168.1.128/25", "192.168.1.128/25"),
        ("fd00::/8", "fd00::/8"),
        (
            " api.example.com , *.svc.cluster.local,10.0.0.0/8 ",
            "api.example.com, *.svc.cluster.local, 10.0.0.0/8",
        ),
    ];
    for (text, described) in accepted {
        assert_eq!(list(text).describe(), described, "{text:?}");
    }
    let refused = [
        ("api.example.com:8080", "is not a host name"),
        ("http://api.example.com", "is not a host name"),
        ("api.example.com/path", "is not a host name"),
        ("/8", "is not a host name"),
        ("[::1", "is not a host name"),
        ("fe80::1%eth0", "is not a host name"),
        (".", "is not a host name"),
        ("*", "holds a * other than"),
        ("*.", "is not a host name"),
        ("a.*.example.com", "holds a * other than"),
        ("*.*.example.com", "holds a * other than"),
        ("api*.example.com", "holds a * other than"),
        ("*.10.0.0.1", "puts a wildcard over an address"),
        ("*.[::1]", "puts a wildcard over an address"),
        ("10.0.0.0/", "has no prefix length"),
        ("10.0.0.0/+8", "has no prefix length"),
        ("10.0.0.0/8a", "has no prefix length"),
        ("10.0.0.0/256", "has no prefix length"),
        ("10.0.0.0/33", "beyond the size of its address"),
        ("::/129", "beyond the size of its address"),
        ("10.0.0.1/8", "has bits set past its prefix length"),
        ("11.0.0.0/7", "has bits set past its prefix length"),
        ("10.0.0.0/6", "has bits set past its prefix length"),
        ("fe80::1/10", "has bits set past its prefix length"),
        ("::ffff:10.0.0.1/104", "has bits set past its prefix length"),
        // Inside the mapped addresses, but with a prefix that cuts their `ffff`: an IPv6 range with bits past it.
        ("::ffff:0:0/95", "has bits set past its prefix length"),
        ("::ffff:0.0.0.0/80", "has bits set past its prefix length"),
        ("127.1/8", "is not a host name"),
    ];
    for (text, why) in refused {
        let error = AllowedTargets::parse(&format!("10.0.0.0/8, {text}")).unwrap_err();
        assert!(
            error.starts_with(&format!("PROXY_ALLOWED_TARGETS: \"{}\" ", text.trim()))
                && error.contains(why),
            "{text:?}: {error}"
        );
    }
}

#[test]
fn an_unset_setting_allows_everything_and_an_empty_one_nothing() {
    use std::env::VarError;
    let only = |value: &'static str| {
        move |name: &str| match name {
            "PROXY_ALLOWED_TARGETS" => Ok(value.to_string()),
            _ => Err(VarError::NotPresent),
        }
    };
    assert_eq!(
        AllowedTargets::from_setting(|_| Err(VarError::NotPresent)),
        Ok(None)
    );
    let empty = AllowedTargets::from_setting(only("")).unwrap().unwrap();
    assert_eq!(empty.describe(), "none");
    assert!(!empty.matches_name("localhost"));
    assert!(!empty.admits("8.8.8.8".parse().unwrap(), false));
    let other = AllowedTargets::from_setting(|name| match name {
        "OTHER" => Ok("x".to_string()),
        _ => Err(VarError::NotPresent),
    });
    assert_eq!(other, Ok(None));
    let set = AllowedTargets::from_setting(only("10.0.0.0/8"));
    assert_eq!(set.unwrap().unwrap().describe(), "10.0.0.0/8");
    assert!(
        AllowedTargets::from_setting(only("x:1"))
            .unwrap_err()
            .contains("\"x:1\"")
    );
    // Not even UTF-8: refused, never read as unset.
    let garbled = AllowedTargets::from_setting(|_| {
        Err(VarError::NotUnicode(std::ffi::OsString::from("10.0.0.0/8")))
    });
    assert_eq!(
        garbled,
        Err("PROXY_ALLOWED_TARGETS is not valid UTF-8".to_string())
    );
}

// ---------------------------------------------------------------------------------------------------------------
// The rule, restated on bit strings

fn bits(address: IpAddr) -> String {
    match address {
        IpAddr::V4(a) => format!("{:032b}", a.to_bits()),
        IpAddr::V6(a) => format!("{:0128b}", a.to_bits()),
    }
}

/// `a` within `network/prefix`, by comparing the first `prefix` characters of their bit strings.
fn oracle_holds(network: IpAddr, prefix: usize, a: IpAddr) -> bool {
    network.is_ipv4() == a.is_ipv4() && bits(network)[..prefix] == bits(a)[..prefix]
}

/// The IPv4 address an IPv4-mapped IPv6 address carries, read from its text.
fn oracle_canonical(a: IpAddr) -> IpAddr {
    match a {
        IpAddr::V6(v6) if bits(a).starts_with(&format!("{}{}", "0".repeat(80), "1".repeat(16))) => {
            let [.., a, b, c, d] = v6.octets();
            IpAddr::V4(Ipv4Addr::new(a, b, c, d))
        }
        other => other,
    }
}

const ORACLE_SPECIAL: [(&str, usize); 6] = [
    ("169.254.0.0", 16),
    ("fe80::", 10),
    ("0.0.0.0", 8),
    ("::", 128),
    ("fd00:ec2::254", 128),
    ("100.100.100.200", 32),
];

/// The table of decision 3, for a list of ranges written as `address/prefix` (or an address alone) and names.
fn oracle_admits(entries: &[&str], address: IpAddr, name_matched: bool) -> bool {
    let address = oracle_canonical(address);
    let special = ORACLE_SPECIAL
        .iter()
        .map(|(n, p)| (n.parse::<IpAddr>().unwrap(), *p))
        .find(|(n, p)| oracle_holds(*n, *p, address));
    if special.is_none() && name_matched {
        return true;
    }
    entries
        .iter()
        .filter_map(|e| oracle_range(e))
        .any(|(network, prefix)| {
            oracle_holds(network, prefix, address)
                && special.is_none_or(|(s, sp)| prefix >= sp && oracle_holds(s, sp, network))
        })
}

fn oracle_range(entry: &str) -> Option<(IpAddr, usize)> {
    let (address, prefix) = entry.split_once('/').unwrap_or((entry, ""));
    let address: IpAddr = address.parse().ok()?;
    let full = if address.is_ipv4() { 32 } else { 128 };
    let prefix = if prefix.is_empty() {
        full
    } else {
        prefix.parse().unwrap()
    };
    match (address, oracle_canonical(address)) {
        (IpAddr::V6(_), IpAddr::V4(v4)) if prefix >= 96 => Some((IpAddr::V4(v4), prefix - 96)),
        _ => Some((address, prefix)),
    }
}

const ADDRESSES: &[&str] = &[
    "8.8.8.8",
    "1.0.0.0",
    "10.1.2.3",
    "10.255.255.255",
    "11.0.0.0",
    "127.0.0.1",
    "127.255.255.254",
    "169.254.169.254",
    "169.254.0.0",
    "169.255.0.1",
    "169.253.255.255",
    "168.254.0.1",
    "0.0.0.0",
    "0.1.2.3",
    "1.255.255.255",
    "192.168.1.1",
    "192.168.1.128",
    "192.168.1.127",
    "255.255.255.255",
    "2001:db8::1",
    "fd00::1",
    "fc00::1",
    "::1",
    "fe80::1",
    "febf:ffff::1",
    "fec0::1",
    "fe7f::1",
    "::",
    "::2",
    "::7f00:1",
    "::ffff:169.254.169.254",
    "::ffff:127.0.0.1",
    "::ffff:10.1.2.3",
    "::ffff:0.0.0.0",
    "::ffff:8.8.8.8",
    "::fffe:a01:203",
    "64:ff9b::a9fe:a9fe",
    "fd00:ec2::254",
    "fd00:ec2::253",
    "fd00:ec2::255",
    "100.100.100.200",
    "100.100.100.201",
    "100.100.100.199",
    "::ffff:100.100.100.200",
];

const LISTS: &[&[&str]] = &[
    &[],
    &["api.example.com", "*.example.com"],
    &["10.0.0.0/8"],
    &["0.0.0.0/0"],
    &["::/0"],
    &["0.0.0.0/0", "::/0"],
    &["128.0.0.0/1"],
    &["169.254.169.254"],
    &["169.254.0.0/16"],
    &["169.254.0.0/17"],
    &["169.0.0.0/8"],
    &["160.0.0.0/4"],
    &["0.0.0.0/8"],
    &["0.0.0.0"],
    &["0.0.0.0/7"],
    &["::"],
    &["::/127"],
    &["::/96"],
    &["fe80::/10"],
    &["fe80::/9"],
    &["fe80::1"],
    &["febf::/16"],
    &["127.0.0.1"],
    &["127.0.0.0/8"],
    &["::1"],
    &["::ffff:127.0.0.1"],
    &["::ffff:0:0/96"],
    &["::ffff:169.254.0.0/112"],
    &["192.168.1.128/25"],
    &["fd00::/8", "10.0.0.0/8", "api.example.com"],
    &["64:ff9b::/96"],
    &["fd00:ec2::254"],
    &["fd00:ec2::/32"],
    &["100.100.100.200"],
    &["100.64.0.0/10"],
    &["100.100.100.200/31"],
];

#[test]
fn every_address_of_the_table_is_admitted_as_the_rule_says() {
    let mut checked = 0;
    let mut admitted = [0; 2];
    for entries in LISTS {
        let targets = list(&entries.join(","));
        for text in ADDRESSES {
            let address: IpAddr = text.parse().unwrap();
            for name_matched in [false, true] {
                let expected = oracle_admits(entries, address, name_matched);
                assert_eq!(
                    targets.admits(address, name_matched),
                    expected,
                    "list {entries:?}, address {text}, name matched {name_matched}"
                );
                checked += 1;
                admitted[usize::from(expected)] += 1;
            }
        }
    }
    // Both verdicts, many times each: a judge whose cases all expect one answer judges nothing.
    assert_eq!(checked, LISTS.len() * ADDRESSES.len() * 2);
    assert!(admitted[0] > 400 && admitted[1] > 400, "{admitted:?}");
}

/// Ranges drawn around their bounds: an address of the network with one bit changed on either side of the prefix, the
/// network itself, its last address, or anything; and networks with or without bits past their prefix.
#[test]
fn ranges_hold_what_their_bit_strings_say() {
    crate::test_seed::draw_from(SEED);
    let mut seen = [[0; 2]; 2];
    for _ in 0..4_000 {
        let v4 = fastrand::bool();
        let width: u32 = if v4 { 32 } else { 128 };
        let prefix = fastrand::u32(0..=width) as u8;
        let raw: u128 = if v4 {
            fastrand::u32(..).into()
        } else {
            fastrand::u128(..)
        };
        let mask = if prefix == 0 {
            0
        } else {
            (u128::MAX << (128 - u32::from(prefix))) >> (128 - width)
        };
        let clean = fastrand::u8(0..4) != 0;
        let network_bits = if clean { raw & mask } else { raw };
        let address_bits = match fastrand::u8(0..5) {
            0 => network_bits,
            1 => network_bits | !mask & (u128::MAX >> (128 - width)),
            2 => network_bits ^ (1u128 << fastrand::u32(0..width)),
            3 => {
                let near = (width - u32::from(prefix)).saturating_sub(1);
                network_bits ^ (1u128 << near.min(width - 1))
            }
            _ => {
                if v4 {
                    fastrand::u32(..).into()
                } else {
                    fastrand::u128(..)
                }
            }
        };
        let ip = |b: u128| -> IpAddr {
            if v4 {
                IpAddr::V4(Ipv4Addr::from_bits(b as u32))
            } else {
                IpAddr::V6(Ipv6Addr::from_bits(b))
            }
        };
        let (network, address) = (ip(network_bits), ip(address_bits));
        let written = format!("{network}/{prefix}");
        let has_host_bits = bits(network)[usize::from(prefix)..].contains('1');
        // Mapped IPv6 networks are rewritten as IPv4: judged by the address table above, not here.
        if !v4 && oracle_canonical(network).is_ipv4() {
            continue;
        }
        match entry(&written) {
            Ok(Entry::Range(range)) => {
                assert!(
                    !has_host_bits,
                    "{written} accepted with bits past its prefix"
                );
                let expected = oracle_holds(network, usize::from(prefix), address);
                assert_eq!(range.holds(address), expected, "{written} holds {address}");
                seen[usize::from(v4)][usize::from(expected)] += 1;
            }
            Ok(other) => panic!("{written}: {other:?}"),
            Err(why) => assert!(has_host_bits, "{written} refused: {why}"),
        }
    }
    for (family, counts) in ["IPv6", "IPv4"].iter().zip(seen) {
        assert!(counts[0] > 200 && counts[1] > 200, "{family}: {counts:?}");
    }
}

#[test]
fn names_match_as_a_url_holds_them() {
    let host = |url: &str| {
        url::Url::parse(url)
            .unwrap()
            .host_str()
            .unwrap()
            .to_string()
    };
    let targets = list("api.example.com, *.svc.cluster.local, b\u{fc}cher.example, localhost");
    let cases = [
        ("http://api.example.com/", true),
        ("http://API.EXAMPLE.COM/", true),
        ("http://api.example.com./", true),
        ("http://example.com/", false),
        ("http://x.api.example.com/", false),
        ("http://apiexample.com/", false),
        ("http://orders.svc.cluster.local:8080/", true),
        ("http://a.b.svc.cluster.local/", true),
        ("http://svc.cluster.local/", false),
        ("http://xsvc.cluster.local/", false),
        ("http://B\u{dc}CHER.example/", true),
        ("http://xn--bcher-kva.example/", true),
        ("http://bucher.example/", false),
        ("http://localhost/", true),
        ("http://LocalHost./", true),
        ("http://l%6fcalhost/", true),
        ("http://localhost.localdomain/", false),
        ("http://127.0.0.1/", false),
    ];
    for (url, matched) in cases {
        assert_eq!(targets.matches_name(&host(url)), matched, "{url}");
    }
    let fullwidth: String = "localhost"
        .chars()
        .map(|c| char::from_u32(c as u32 + 0xFEE0).unwrap())
        .collect();
    assert!(targets.matches_name(&host(&format!("http://{fullwidth}/"))));
    assert!(!list("").matches_name("localhost"));
    assert!(!list("10.0.0.0/8").matches_name("10.0.0.0"));
}

#[test]
fn every_objection_names_its_host_and_the_setting() {
    let cases = [
        (Objection::Unreadable, vec![]),
        (
            Objection::Outside {
                host: "a.test".into(),
            },
            vec!["a.test"],
        ),
        (
            Objection::ResolvesOutside {
                host: "b.test".into(),
                addresses: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
            },
            vec!["b.test", "127.0.0.1, ::1"],
        ),
        (
            Objection::Unresolved {
                host: "c.test".into(),
            },
            vec!["c.test", "does not resolve"],
        ),
    ];
    for (objection, parts) in cases {
        let message = objection.message();
        assert!(message.contains("PROXY_ALLOWED_TARGETS"), "{message}");
        for part in parts {
            assert!(message.contains(part), "{message} lacks {part}");
        }
    }
    assert_eq!(Refused.to_string(), "outside PROXY_ALLOWED_TARGETS");
}

#[tokio::test]
async fn a_target_being_saved_is_judged_like_a_connection() {
    let targets = list("*.example.com, localhost, 10.0.0.0/8");
    let cases = [
        ("not a url", Err(Objection::Unreadable)),
        ("mailto:someone@example.com", Err(Objection::Unreadable)),
        ("http://10.1.2.3:8080/base", Ok(())),
        ("http://[::ffff:10.1.2.3]/", Ok(())),
        (
            "http://169.254.169.254/latest",
            Err(Objection::Outside {
                host: "169.254.169.254".into(),
            }),
        ),
        (
            "http://[fe80::1]/",
            Err(Objection::Outside {
                host: "[fe80::1]".into(),
            }),
        ),
        ("http://localhost:1/", Ok(())),
        ("http://LOCALHOST.:1/", Ok(())),
        // A name an entry matches is accepted while it does not resolve.
        ("http://nowhere.example.com/", Ok(())),
        (
            "http://nowhere.invalid/",
            Err(Objection::Unresolved {
                host: "nowhere.invalid".into(),
            }),
        ),
    ];
    for (target, expected) in cases {
        assert_eq!(targets.check(target).await, expected, "{target}");
    }
    // Resolved, but to nothing a range admits.
    let ranges_only = list("10.0.0.0/8");
    assert_eq!(
        ranges_only.check("http://localhost:7/").await,
        Err(Objection::ResolvesOutside {
            host: "localhost".into(),
            addresses: vec!["127.0.0.1".parse().unwrap()],
        })
    );
    // Without any range, a name no entry matches is refused without a lookup.
    assert_eq!(
        list("*.example.com").check("http://localhost:7/").await,
        Err(Objection::Outside {
            host: "localhost".into(),
        })
    );
    assert_eq!(list("127.0.0.0/8").check("http://localhost/").await, Ok(()));
}

// ---------------------------------------------------------------------------------------------------------------
// Real sockets, the system's resolver: the external judge

/// A local socket that counts the connections it accepts and answers each one with an empty 200, then closes it.
struct Listener {
    port: u16,
    accepted: Arc<AtomicUsize>,
}

impl Listener {
    async fn bind(address: &str) -> Option<Self> {
        let socket = tokio::net::TcpListener::bind(address).await.ok()?;
        let port = socket.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&accepted);
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut stream, _)) = socket.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buffer = [0u8; 4096];
                    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer))
                        .await;
                    let _ = stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                        )
                        .await;
                });
            }
        });
        Some(Self { port, accepted })
    }

    fn count(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

/// What a spelling names, which decides the verdict of each list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Named {
    Loopback4,
    Unspecified4,
    Loopback6,
    Unspecified6,
    Localhost,
}

/// One part of an IPv4 address in decimal, octal or hexadecimal, as the URL standard reads them.
fn number(value: u32) -> String {
    match fastrand::u8(0..4) {
        0 => value.to_string(),
        1 => format!("0{value:o}"),
        2 => format!("0x{value:x}"),
        _ => format!("0X{value:X}"),
    }
}

/// An IPv4 address in one to four parts, the last part holding the bytes left.
fn ipv4_spelling(address: Ipv4Addr) -> String {
    let [a, b, c, d] = address.octets();
    let parts: Vec<u32> = match fastrand::u8(1..=4) {
        1 => vec![address.to_bits()],
        2 => vec![a.into(), u32::from_be_bytes([0, b, c, d])],
        3 => vec![a.into(), b.into(), u32::from(u16::from_be_bytes([c, d]))],
        _ => vec![a.into(), b.into(), c.into(), d.into()],
    };
    let parts: Vec<String> = parts.into_iter().map(number).collect();
    let dot = if fastrand::u8(0..6) == 0 { "." } else { "" };
    format!("{}{dot}", parts.join("."))
}

/// An IPv6 address with each group in upper or lower case, padded or not, and any run of zero groups compressed or not.
fn ipv6_groups(groups: [u16; 8]) -> String {
    let written: Vec<String> = groups
        .iter()
        .map(|g| match fastrand::u8(0..3) {
            0 => format!("{g:x}"),
            1 => format!("{g:04X}"),
            _ => format!("{g:04x}"),
        })
        .collect();
    let zeros: Vec<usize> = (0..8).filter(|i| groups[*i] == 0).collect();
    if zeros.is_empty() || fastrand::u8(0..3) == 0 {
        return written.join(":");
    }
    let start = zeros[fastrand::usize(..zeros.len())];
    let run_end = groups[start..]
        .iter()
        .position(|g| *g != 0)
        .map_or(8, |n| start + n);
    let end = fastrand::usize(start + 1..=run_end);
    format!(
        "{}::{}",
        written[..start].join(":"),
        written[end..].join(":")
    )
}

fn spelling(named: Named) -> String {
    // `::ffff:` then the IPv4 address in dots, or eight groups of hexadecimal.
    let mapped = |v4: Ipv4Addr| -> String {
        let [a, b, c, d] = v4.octets();
        if fastrand::bool() {
            let prefix = ipv6_groups([0, 0, 0, 0, 0, 0xffff, 1, 1]);
            let prefix = prefix.rsplitn(3, ':').nth(2).unwrap();
            format!("[{prefix}:{v4}]")
        } else {
            let groups = [
                0,
                0,
                0,
                0,
                0,
                0xffff,
                u16::from_be_bytes([a, b]),
                u16::from_be_bytes([c, d]),
            ];
            format!("[{}]", ipv6_groups(groups))
        }
    };
    match named {
        Named::Loopback4 if fastrand::u8(0..4) == 0 => mapped(Ipv4Addr::LOCALHOST),
        Named::Loopback4 => ipv4_spelling(Ipv4Addr::LOCALHOST),
        Named::Unspecified4 if fastrand::u8(0..4) == 0 => mapped(Ipv4Addr::UNSPECIFIED),
        Named::Unspecified4 => ipv4_spelling(Ipv4Addr::UNSPECIFIED),
        Named::Loopback6 => format!("[{}]", ipv6_groups([0, 0, 0, 0, 0, 0, 0, 1])),
        Named::Unspecified6 => format!("[{}]", ipv6_groups([0; 8])),
        Named::Localhost => {
            let base: String = "localhost"
                .chars()
                .map(|c| match fastrand::u8(0..6) {
                    0 => c.to_ascii_uppercase().to_string(),
                    1 => format!("%{:02x}", c as u32),
                    2 => char::from_u32(c as u32 + 0xFEE0).unwrap().to_string(),
                    _ => c.to_string(),
                })
                .collect();
            if fastrand::bool() {
                format!("{base}.")
            } else {
                base
            }
        }
    }
}

/// A list of the judge and what it admits.
struct Case {
    text: &'static str,
    admits: fn(Named) -> bool,
}

const CASES: &[Case] = &[
    Case {
        text: "10.0.0.0/8, *.example.com, fd00::/8",
        admits: |_| false,
    },
    Case {
        text: "0.0.0.0/0, ::/0",
        admits: |n| matches!(n, Named::Loopback4 | Named::Loopback6 | Named::Localhost),
    },
    Case {
        text: "127.0.0.1, ::1, 0.0.0.0, ::",
        admits: |_| true,
    },
    Case {
        text: "localhost",
        admits: |n| n == Named::Localhost,
    },
    Case {
        text: "*.localhost, 127.0.0.2, 0.0.0.1",
        admits: |_| false,
    },
];

/// What `attempt` answered, and how many connections the listener accepted meanwhile. A connection completes in the
/// kernel before the listener accepts it: when `expected` connections are due, they are waited for, two seconds at most.
async fn reached<T>(
    listener: &Listener,
    expected: usize,
    attempt: impl std::future::Future<Output = T>,
) -> (T, usize) {
    let before = listener.count();
    let answer = attempt.await;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while listener.count() - before < expected && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    (answer, listener.count() - before)
}

async fn relay(client: &ProxyClient, url: &str) -> Result<(), ProxyError> {
    client
        .forward(url, "/x", Request::new(Body::empty()))
        .await
        .map(drop)
}

/// For each family of spellings drawn, the number kept (those that reach the listener without a list), at least.
const KEPT_AT_LEAST: usize = 25;

async fn judge_spellings(listener: &Listener, families: &[Named]) {
    let open = ProxyClient::new();
    let clients: Vec<(&Case, ProxyClient)> = CASES
        .iter()
        .map(|case| (case, ProxyClient::restricted(list(case.text))))
        .collect();
    let mut kept = vec![0; families.len()];
    let mut expected_total = 0;
    let mut tried = std::collections::BTreeSet::new();
    for round in 0..(families.len() * 60) {
        let index = round % families.len();
        let named = families[index];
        let host = spelling(named);
        if !tried.insert(host.clone()) {
            continue;
        }
        let url = format!("http://{host}:{}", listener.port);
        let (opened, connections) =
            reached(listener, 0, async { relay(&open, &url).await.is_ok() }).await;
        if !opened || connections != 1 {
            if std::env::var("TARGETS_DEBUG").is_ok() {
                eprintln!(
                    "not reached: {url} ({named:?}) opened {opened} connections {connections}"
                );
            }
            continue;
        }
        kept[index] += 1;
        expected_total += 1;
        for (case, client) in &clients {
            let admitted = (case.admits)(named);
            let what = format!("{url} ({named:?}) with {:?}", case.text);
            let due = usize::from(admitted);
            let (relayed, connections) =
                reached(listener, due, async { relay(client, &url).await }).await;
            if admitted {
                assert_eq!((relayed, connections), (Ok(()), 1), "relay of {what}");
            } else {
                assert_eq!(
                    (relayed, connections),
                    (Err(ProxyError::Refused), 0),
                    "relay of {what}"
                );
            }
            let (pinged, connections) =
                reached(listener, due, async { client.ping(&url).await.reachable }).await;
            assert_eq!((pinged, connections), (admitted, due), "ping of {what}");
            expected_total += 2 * due;
            let targets = client.allowed_targets().unwrap();
            assert_eq!(targets.check(&url).await.is_ok(), admitted, "saving {what}");
        }
    }
    for (named, count) in families.iter().zip(kept) {
        assert!(
            count >= KEPT_AT_LEAST,
            "{named:?}: {count} spellings reached the listener"
        );
    }
    // No connection late or unseen: the spellings left out (which never reached it) included.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(listener.count(), expected_total);
}

#[tokio::test]
async fn spellings_of_ipv4_loopback_and_localhost_reach_the_listener_only_when_admitted() {
    crate::test_seed::draw_from(SEED);
    let listener = Listener::bind("127.0.0.1:0").await.unwrap();
    judge_spellings(
        &listener,
        &[Named::Loopback4, Named::Unspecified4, Named::Localhost],
    )
    .await;
}

#[tokio::test]
async fn spellings_of_ipv6_loopback_reach_the_listener_only_when_admitted() {
    crate::test_seed::draw_from(SEED ^ 6);
    let listener = Listener::bind("[::1]:0")
        .await
        .expect("this test needs the IPv6 loopback address ::1");
    judge_spellings(&listener, &[Named::Loopback6, Named::Unspecified6]).await;
}

/// The metadata endpoint written as an address is refused before any connection, by broad ranges too, and the error is
/// the refusal, not a network failure.
#[tokio::test]
async fn a_link_local_address_is_refused_before_connecting() {
    let client = ProxyClient::restricted(list("0.0.0.0/0, ::/0, *.example.com"));
    for url in [
        "http://169.254.169.254/latest/meta-data",
        "http://2852039166/",
        "http://0xa9.0xfe.0xa9.0xfe/",
        "http://[::ffff:169.254.169.254]/",
        "http://[fe80::1]:8080/",
    ] {
        let started = std::time::Instant::now();
        assert_eq!(relay(&client, url).await, Err(ProxyError::Refused), "{url}");
        let ping = client.ping(url).await;
        assert!(
            !ping.reachable
                && ping
                    .error
                    .as_deref()
                    .is_some_and(|e| e.contains("PROXY_ALLOWED_TARGETS")),
            "{url}: {ping:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2), "{url}");
    }
}

/// Without a list, the availability check reaches a target written as an IPv6 address (it looked up `[::1]`, brackets
/// included, and always failed).
#[tokio::test]
async fn the_availability_check_reaches_an_ipv6_target() {
    let listener = Listener::bind("[::1]:0")
        .await
        .expect("this test needs the IPv6 loopback address ::1");
    let url = format!("http://[::1]:{}/", listener.port);
    let (status, connections) = reached(&listener, 1, ProxyClient::new().ping(&url)).await;
    assert!(status.reachable, "{status:?}");
    assert_eq!(connections, 1);
}

// ---------------------------------------------------------------------------------------------------------------
// Through the server: the three relays, the availability check, saving

fn service(name: &str, target: &str, mocked: bool, proxy_rule: bool) -> serde_json::Value {
    let rules: Vec<serde_json::Value> = if proxy_rule {
        vec![serde_json::json!({
            "name": "relay", "method": "GET", "sub_path": null, "action": "proxy", "script": null,
            "conditions": {"all_of": [], "any_of": []},
            "response": {"status": 200, "headers": [], "body": []}
        })]
    } else {
        vec![]
    };
    serde_json::json!({
        "name": name, "listen_path": "", "real_target_url": target, "is_mocked": mocked,
        "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "mock", "rules": rules
    })
}

async fn server_with(list_text: &str, services: Vec<serde_json::Value>) -> String {
    use crate::server::test_support::{auth_disabled, serve, temp_data_dir, test_state};
    let config =
        serde_json::from_value(serde_json::json!({"services": services, "groups": []})).unwrap();
    let data_dir = temp_data_dir("proxy-targets");
    let mut state = test_state(&data_dir, config, auth_disabled()).await;
    state.proxy = ProxyClient::restricted(list(list_text));
    state.observation.toggle.enable(None, "observed");
    serve(crate::server::build_router(state, &data_dir)).await
}

#[tokio::test]
async fn the_relays_the_availability_check_and_saving_follow_the_list() {
    let backend = Listener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://localhost:{}", backend.port);
    let services = || {
        vec![
            service("relay", &target, false, false),
            service("observed", &target, false, false),
            service("ruled", &target, true, true),
        ]
    };
    let http = reqwest::Client::builder().no_proxy().build().unwrap();

    // Outside the list: every relay answers the refusal, the log says 502, the check says why, and nothing connects.
    let refusing = server_with("10.0.0.0/8, *.example.com", services()).await;
    for name in ["relay", "observed", "ruled"] {
        let (response, connections) =
            reached(&backend, 0, http.get(format!("{refusing}/{name}/x")).send()).await;
        let response = response.unwrap();
        assert_eq!(response.status(), 502, "{name}");
        assert_eq!(
            response.text().await.unwrap(),
            crate::engine::REFUSED_TARGET,
            "{name}"
        );
        assert_eq!(connections, 0, "{name}");
    }
    let log: Vec<serde_json::Value> = http
        .get(format!("{refusing}/api/logs"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let statuses: Vec<u64> = log.iter().map(|e| e["status"].as_u64().unwrap()).collect();
    assert_eq!(statuses, [502, 502, 502]);
    let (ping, connections) = reached(
        &backend,
        0,
        http.post(format!("{refusing}/api/services/relay/ping"))
            .send(),
    )
    .await;
    let ping: serde_json::Value = ping.unwrap().json().await.unwrap();
    assert_eq!(ping["reachable"], false);
    assert!(
        ping["error"]
            .as_str()
            .unwrap()
            .contains("resolves to 127.0.0.1"),
        "{ping}"
    );
    assert_eq!(connections, 0);

    // Inside the list: the same three relays and the check reach the backend.
    let allowing = server_with("localhost", services()).await;
    for name in ["relay", "observed", "ruled"] {
        let (response, connections) =
            reached(&backend, 1, http.get(format!("{allowing}/{name}/x")).send()).await;
        assert_eq!(response.unwrap().status(), 200, "{name}");
        assert_eq!(connections, 1, "{name}");
    }
    let (ping, connections) = reached(
        &backend,
        1,
        http.post(format!("{allowing}/api/services/relay/ping"))
            .send(),
    )
    .await;
    assert_eq!(
        ping.unwrap().json::<serde_json::Value>().await.unwrap()["reachable"],
        true
    );
    assert_eq!(connections, 1);

    // Saving: a new target is judged; a target already stored for the service is not judged again.
    let save = |method: reqwest::Method, path: &str, body: serde_json::Value| {
        http.request(method, format!("{refusing}{path}"))
            .json(&body)
            .send()
    };
    let error = |response: reqwest::Response| async move {
        let status = response.status().as_u16();
        let body: serde_json::Value = response.json().await.unwrap();
        (
            status,
            body["error"].as_str().unwrap_or_default().to_string(),
        )
    };
    let post = reqwest::Method::POST;
    let put = reqwest::Method::PUT;
    let refused = [
        ("http://169.254.169.254/latest", "169.254.169.254"),
        ("http://localhost:1/", "resolves to 127.0.0.1"),
        ("http://2130706433:1/", "127.0.0.1"),
        ("http://[::ffff:7f00:1]:1/", "[::ffff:7f00:1]"),
        ("not a url", "cannot be read"),
    ];
    for (new_target, said) in refused {
        let response = save(
            post.clone(),
            "/api/services",
            service("fresh", new_target, false, false),
        );
        let (status, message) = error(response.await.unwrap()).await;
        assert_eq!(status, 400, "{new_target}: {message}");
        assert!(
            message.contains(said) && message.contains("PROXY_ALLOWED_TARGETS"),
            "{new_target}: {message}"
        );
    }
    for (name, new_target, mocked) in [
        ("inside", "http://10.1.2.3:1/", false),
        ("mocked", "", true),
    ] {
        let response = save(
            post.clone(),
            "/api/services",
            service(name, new_target, mocked, false),
        );
        assert_eq!(response.await.unwrap().status(), 201, "{new_target}");
    }
    let mut kept = service("relay", &target, false, false);
    kept["listen_path"] = "/v2/*".into();
    assert_eq!(
        save(put.clone(), "/api/services/relay", kept)
            .await
            .unwrap()
            .status(),
        200
    );
    let moved = service("relay", "http://127.0.0.1:1/", false, false);
    let (status, message) = error(
        save(put.clone(), "/api/services/relay", moved)
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        (status, message.contains("127.0.0.1")),
        (400, true),
        "{message}"
    );

    let config: serde_json::Value = http
        .get(format!("{refusing}/api/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        save(put.clone(), "/api/config", config.clone())
            .await
            .unwrap()
            .status(),
        200
    );
    let mut imported = config.clone();
    imported["services"].as_array_mut().unwrap().push(service(
        "metadata",
        "http://[fe80::1]/",
        false,
        false,
    ));
    let (status, message) = error(save(put.clone(), "/api/config", imported).await.unwrap()).await;
    assert_eq!(
        (status, message.contains("[fe80::1]")),
        (400, true),
        "{message}"
    );
    assert_eq!(backend.count(), 4, "no save connects to its target");
}

/// A backend's redirect goes back to the caller as it is: followed, it would reach an address no check judged (the
/// HTTP client never resolves an address, and `check_address` judges only the first URL).
#[tokio::test]
async fn a_redirect_is_relayed_never_followed() {
    let elsewhere = Listener::bind("127.0.0.1:0").await.unwrap();
    let location = format!("http://127.0.0.1:{}/", elsewhere.port);
    let redirect = axum::Router::new().fallback(move || {
        let location = location.clone();
        async move { (axum::http::StatusCode::FOUND, [("location", location)]) }
    });
    let redirector = crate::server::test_support::serve(redirect).await;
    let target = redirector.replace("127.0.0.1", "localhost");
    let mimicway = server_with("localhost", vec![service("relay", &target, false, false)]).await;
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = http
        .get(format!("{mimicway}/relay/x"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 302);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(elsewhere.count(), 0);
}
