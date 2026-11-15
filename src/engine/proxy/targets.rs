// Where the proxy may connect (`PROXY_ALLOWED_TARGETS`, decision 3): host names, their subdomains, addresses and
// ranges an operator allows, judged on the addresses a connection really uses. Unset, every target is allowed.
//
// The relay's HTTP client resolves names through `Gate`, which hands it only the addresses the list admits, so that the
// address judged is the address connected; the client never resolves a target written as an address, which
// `ProxyClient` judges with `admits` before sending anything. The availability check connects to what `connectable`
// returns, and saving a target asks `check`, which judges it the same way.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use url::Host;

const SETTING: &str = "PROXY_ALLOWED_TARGETS";

/// How long saving waits for a name to resolve before judging it as not resolving.
const SAVE_LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Addresses whose first `prefix` bits are those of `network`. An IPv4 address mapped in IPv6 is held as IPv4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Range {
    network: IpAddr,
    prefix: u8,
}

impl Range {
    fn holds(&self, address: IpAddr) -> bool {
        match (self.network, address) {
            (IpAddr::V4(n), IpAddr::V4(a)) => {
                same_prefix(n.to_bits().into(), a.to_bits().into(), self.prefix, 32)
            }
            (IpAddr::V6(n), IpAddr::V6(a)) => {
                same_prefix(n.to_bits(), a.to_bits(), self.prefix, 128)
            }
            _ => false,
        }
    }

    fn within(&self, outer: &Range) -> bool {
        self.prefix >= outer.prefix && outer.holds(self.network)
    }
}

/// Whether `a` and `b`, numbers of `width` bits, agree on their first `prefix` bits.
fn same_prefix(a: u128, b: u128, prefix: u8, width: u32) -> bool {
    (a ^ b)
        .checked_shr(width - u32::from(prefix))
        .is_none_or(|rest| rest == 0)
}

/// Link-local and unspecified addresses, and the cloud metadata endpoints outside link-local (AWS over IPv6, Alibaba
/// Cloud), admitted only by an entry inside their own range: `0.0.0.0/0` or `fd00::/8` must not open a metadata
/// endpoint, and Linux connects `0.0.0.0` to the machine itself.
const SPECIAL: [Range; 6] = [
    Range {
        network: IpAddr::V4(Ipv4Addr::new(169, 254, 0, 0)),
        prefix: 16,
    },
    Range {
        network: IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0)),
        prefix: 10,
    },
    Range {
        network: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        prefix: 8,
    },
    Range {
        network: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
        prefix: 128,
    },
    Range {
        network: IpAddr::V6(Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254)),
        prefix: 128,
    },
    Range {
        network: IpAddr::V4(Ipv4Addr::new(100, 100, 100, 200)),
        prefix: 32,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    /// A host name, lower case, without a final dot.
    Name(String),
    /// Every name that ends with this suffix, which starts with a dot (`.example.com`).
    Subdomains(String),
    Range(Range),
}

impl std::fmt::Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Entry::Name(name) => f.write_str(name),
            Entry::Subdomains(suffix) => write!(f, "*{suffix}"),
            Entry::Range(range) => write!(f, "{}/{}", range.network, range.prefix),
        }
    }
}

/// The targets a deployment allows, read from `PROXY_ALLOWED_TARGETS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowedTargets {
    entries: Vec<Entry>,
}

/// Why a target was not saved: the sentence shown to the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Objection {
    Unreadable,
    Outside {
        host: String,
    },
    ResolvesOutside {
        host: String,
        addresses: Vec<IpAddr>,
    },
    Unresolved {
        host: String,
    },
}

impl Objection {
    pub(crate) fn message(&self) -> String {
        use crate::i18n::tr;
        match self {
            Objection::Unreadable => tr(
                "The target URL cannot be read, so it cannot be checked against the targets this server may relay to (PROXY_ALLOWED_TARGETS).",
                &[],
            ),
            Objection::Outside { host } => tr(
                "The target host {0} is outside the targets this server may relay to (PROXY_ALLOWED_TARGETS, set by its administrator).",
                &[host],
            ),
            Objection::ResolvesOutside { host, addresses } => {
                let addresses: Vec<String> = addresses.iter().map(IpAddr::to_string).collect();
                tr(
                    "The target host {0} resolves to {1}, outside the targets this server may relay to (PROXY_ALLOWED_TARGETS, set by its administrator).",
                    &[host, &addresses.join(", ")],
                )
            }
            Objection::Unresolved { host } => tr(
                "The target host {0} does not resolve now, and only a name written in PROXY_ALLOWED_TARGETS is accepted without its addresses: ask the administrator to add it, or save again once it resolves.",
                &[host],
            ),
        }
    }
}

/// Why no address of a host may be connected.
#[derive(Debug)]
pub(crate) enum Blocked {
    /// Nothing the list admits (the addresses found, if a lookup ran).
    Refused(Vec<IpAddr>),
    /// The name did not resolve.
    Unresolved(std::io::Error),
}

/// The error the relay's HTTP client gets from `Gate` for a name that resolves to nothing the list admits; the relay
/// finds it among the causes of the client's error (`refused_in`).
#[derive(Debug)]
pub(crate) struct Refused;

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "outside {SETTING}")
    }
}

impl std::error::Error for Refused {}

/// Whether `Refused` is `error` or one of its causes.
pub(crate) fn refused_in(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(current) = cause {
        if current.is::<Refused>() {
            return true;
        }
        cause = current.source();
    }
    false
}

impl AllowedTargets {
    /// The list `lookup` (`std::env::var`, in `main.rs`) gives for `PROXY_ALLOWED_TARGETS`; `None` when it is unset. A
    /// value that cannot be read, not even as UTF-8, stops the server instead of allowing everything.
    pub fn from_setting(
        lookup: impl Fn(&str) -> Result<String, std::env::VarError>,
    ) -> Result<Option<Self>, String> {
        match lookup(SETTING) {
            Ok(value) => Self::parse(&value).map(Some),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(format!("{SETTING} is not valid UTF-8")),
        }
    }

    /// Entries separated by commas, blanks around them and empty ones ignored: a value without any entry allows no
    /// target at all.
    pub fn parse(value: &str) -> Result<Self, String> {
        let entries = value
            .split(',')
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| entry(text).map_err(|why| format!("{SETTING}: \"{text}\" {why}")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { entries })
    }

    /// The entries as they are matched, for the server's log.
    pub fn describe(&self) -> String {
        if self.entries.is_empty() {
            return "none".into();
        }
        let entries: Vec<String> = self.entries.iter().map(Entry::to_string).collect();
        entries.join(", ")
    }

    /// Whether a name or subdomains entry matches `name`, a host name as a URL holds it.
    fn matches_name(&self, name: &str) -> bool {
        let name = name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase();
        self.entries.iter().any(|entry| match entry {
            Entry::Name(listed) => *listed == name,
            Entry::Subdomains(suffix) => name.ends_with(suffix.as_str()),
            Entry::Range(_) => false,
        })
    }

    /// Whether a connection to `address` is allowed, for a host that a name entry matches (`name_matched`) or for a
    /// host that no name entry matches, or that is written as an address.
    pub(crate) fn admits(&self, address: IpAddr, name_matched: bool) -> bool {
        let address = address.to_canonical();
        let special = SPECIAL.iter().find(|special| special.holds(address));
        if special.is_none() && name_matched {
            return true;
        }
        self.ranges().any(|range| {
            range.holds(address) && special.is_none_or(|special| range.within(special))
        })
    }

    fn ranges(&self) -> impl Iterator<Item = &Range> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Range(range) => Some(range),
            _ => None,
        })
    }

    /// The addresses of `host` that a connection to `port` may use, in the order the resolver gave them.
    pub(crate) async fn connectable(
        &self,
        host: Host<&str>,
        port: u16,
    ) -> Result<Vec<SocketAddr>, Blocked> {
        let name = match host {
            Host::Ipv4(address) => return self.literal(IpAddr::V4(address), port),
            Host::Ipv6(address) => return self.literal(IpAddr::V6(address), port),
            Host::Domain(name) => name,
        };
        let matched = self.matches_name(name);
        // Only a range can admit the addresses of a name no entry matches: without one, nothing is looked up.
        if !matched && self.ranges().next().is_none() {
            return Err(Blocked::Refused(Vec::new()));
        }
        let found: Vec<SocketAddr> = tokio::net::lookup_host((name, port))
            .await
            .map_err(Blocked::Unresolved)?
            .collect();
        let usable: Vec<SocketAddr> = found
            .iter()
            .copied()
            .filter(|address| self.admits(address.ip(), matched))
            .collect();
        if usable.is_empty() {
            return Err(Blocked::Refused(found.iter().map(SocketAddr::ip).collect()));
        }
        Ok(usable)
    }

    fn literal(&self, address: IpAddr, port: u16) -> Result<Vec<SocketAddr>, Blocked> {
        if self.admits(address, false) {
            Ok(vec![SocketAddr::new(address, port)])
        } else {
            Err(Blocked::Refused(Vec::new()))
        }
    }

    /// The judgement of a target being saved: what a connection would decide, except that a name a name entry matches
    /// is accepted while it does not resolve, its addresses being judged at each connection.
    pub(crate) async fn check(&self, target: &str) -> Result<(), Objection> {
        let url = url::Url::parse(target).map_err(|_| Objection::Unreadable)?;
        let host = url.host().ok_or(Objection::Unreadable)?;
        let shown = host.to_string();
        let lookup = tokio::time::timeout(SAVE_LOOKUP_TIMEOUT, self.connectable(host.clone(), 0));
        match lookup.await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(Blocked::Refused(addresses))) if !addresses.is_empty() => {
                Err(Objection::ResolvesOutside {
                    host: shown,
                    addresses,
                })
            }
            Ok(Err(Blocked::Refused(_))) => Err(Objection::Outside { host: shown }),
            _ => match host {
                Host::Domain(name) if self.matches_name(name) => Ok(()),
                _ => Err(Objection::Unresolved { host: shown }),
            },
        }
    }
}

/// An entry of the list, or why it is not one.
fn entry(text: &str) -> Result<Entry, &'static str> {
    if let Some((address, prefix)) = text.split_once('/')
        && let Some(network) = bare_address(address)
    {
        let is_number = !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit());
        let prefix: u8 = prefix
            .parse()
            .ok()
            .filter(|_| is_number)
            .ok_or("has no prefix length after its /")?;
        return range(network, prefix);
    }
    if let Some(address) = bare_address(text) {
        return single(address);
    }
    let (wildcard, rest) = match text.strip_prefix("*.") {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if rest.contains('*') {
        return Err("holds a * other than a leading *.");
    }
    match Host::parse(rest) {
        Ok(Host::Domain(name)) => {
            let name = name.strip_suffix('.').unwrap_or(&name).to_string();
            if name.is_empty() {
                Err("is not a host name")
            } else if wildcard {
                Ok(Entry::Subdomains(format!(".{name}")))
            } else {
                Ok(Entry::Name(name))
            }
        }
        Ok(Host::Ipv4(address)) if !wildcard => single(IpAddr::V4(address)),
        Ok(_) => Err("puts a wildcard over an address"),
        Err(_) => Err("is not a host name, an address or a range (no scheme, port or path)"),
    }
}

/// An address written alone, IPv6 with or without brackets.
fn bare_address(text: &str) -> Option<IpAddr> {
    let text = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .unwrap_or(text);
    text.parse().ok()
}

fn single(address: IpAddr) -> Result<Entry, &'static str> {
    range(address, width(address))
}

fn width(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    }
}

/// A range from what was written: an IPv6 range inside the mapped IPv4 addresses (`::ffff:0:0/96`) becomes the IPv4
/// range it holds, since mapped addresses are judged as IPv4.
fn range(network: IpAddr, prefix: u8) -> Result<Entry, &'static str> {
    if prefix > width(network) {
        return Err("has a prefix length beyond the size of its address");
    }
    let range = match network {
        IpAddr::V6(v6) if prefix >= 96 => match v6.to_ipv4_mapped() {
            Some(v4) => Range {
                network: IpAddr::V4(v4),
                prefix: prefix - 96,
            },
            None => Range { network, prefix },
        },
        _ => Range { network, prefix },
    };
    let bits = match range.network {
        IpAddr::V4(n) => u128::from(n.to_bits()),
        IpAddr::V6(n) => n.to_bits(),
    };
    if !clear_after(bits, range.prefix, u32::from(width(range.network))) {
        return Err("has bits set past its prefix length (write the first address of the range)");
    }
    Ok(Entry::Range(range))
}

/// Whether the bits of `value` past its first `prefix`, out of `width`, are all zero.
fn clear_after(value: u128, prefix: u8, width: u32) -> bool {
    let kept = u32::from(prefix);
    value
        .checked_shl(128 - width + kept)
        .is_none_or(|rest| rest == 0)
}

/// The resolver of the relay's HTTP client when the list is set: the system's resolver, then only the addresses the
/// list admits for that name, or `Refused`.
pub(crate) struct Gate(pub(crate) Arc<AllowedTargets>);

impl reqwest::dns::Resolve for Gate {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let targets = Arc::clone(&self.0);
        let name = name.as_str().to_owned();
        Box::pin(async move {
            match targets.connectable(Host::Domain(&name), 0).await {
                Ok(addresses) => Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs),
                Err(Blocked::Refused(_)) => Err(Box::new(Refused) as _),
                Err(Blocked::Unresolved(e)) => Err(Box::new(e) as _),
            }
        })
    }
}

#[cfg(test)]
mod tests;
