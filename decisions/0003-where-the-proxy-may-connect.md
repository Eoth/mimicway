# 3. Where the proxy may connect: a list checked at every connection, after DNS

Accepted, 2026-11-15.

## Context

An editor chooses the target of a proxy (`real_target_url`), and Mimicway connects to whatever host it names: internal services, the cloud metadata endpoint (`169.254.169.254`), Mimicway's own port. Until now only the network could stop it ([security model](../docs/en/security.md)). Three ways connect to a target: the relays (a service in proxy mode, the same service observed, a rule whose action is `proxy`, a WSDL request let through), all in `ProxyClient` (`src/engine/proxy.rs`), and the availability check, a TCP connection.

A check of the target's text is not enough: a name can resolve to any address, and change between a check and the connection (DNS rebinding); an address has many spellings that the URL standard reads as the same one (`2130706433`, `0177.0.0.1`, `0x7f.1`, `127.1`, `[::ffff:127.0.0.1]` all name `127.0.0.1`).

## Options

1. **Check the target's text when it is saved.** Simple, but blind to what a name resolves to, and to every target saved before the list.
2. **Resolve the name, check the addresses, then connect to the name.** The connection resolves again and may get another address: the check and the connection disagree.
3. **Judge the addresses inside the connection.** The relay's HTTP client resolves names through a resolver of ours, which returns only the addresses the list admits; a target written as an address is judged before the request is sent (the client never resolves those); the availability check connects to the addresses the same resolver returns. Saving runs the same judgement, so that an editor learns at once.

## Decision

Option 3, set by `PROXY_ALLOWED_TARGETS` (`src/engine/proxy/targets.rs`). Unset, nothing changes: every target is allowed and the relay honours the operator's `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` as before.

### The rule, as a table

Entries are separated by commas: a host name (`api.example.com`), its subdomains (`*.example.com`, any depth, not `example.com` itself), an address (`10.0.0.5`, `::1`, `[::1]`) or a range (`10.0.0.0/8`, `fd00::/8`). Names compare in lower case, one final dot ignored, international names in their ASCII form, as a URL holds them. Ports are not restricted.

**Special addresses** are link-local (`169.254.0.0/16`, `fe80::/10`), unspecified (`0.0.0.0/8`, `::`; Linux connects `0.0.0.0` to the machine itself), and the two cloud metadata endpoints outside link-local (`fd00:ec2::254`, AWS over IPv6; `100.100.100.200`, Alibaba Cloud). One is admitted only by an address or range entry that lies inside its special range: `169.254.169.254` or `169.254.0.0/16` admit the metadata endpoint, `0.0.0.0/0` does not, nor does `fd00::/8` admit `fd00:ec2::254`.

An **address is admitted** when an address or range entry holds it (inside its special range, for a special address). An IPv4 address mapped in IPv6 (`::ffff:a.b.c.d`) is judged as `a.b.c.d`, as a target and as an entry, since a dual-stack socket connects it to that IPv4 address.

| Target host | How the list holds it | When saved | At each connection (relay, availability check) |
|---|---|---|---|
| any | list unset | accepted, no lookup | as before the list |
| any | list set but empty | refused | refused |
| address, in any spelling the URL standard reads | admitted | accepted | connected to it |
| address | not admitted (a name entry never admits an address: `localhost` does not admit `127.0.0.1`) | refused | refused, no connection attempted |
| name that a name or subdomains entry matches | resolves to at least one address that is not special, or is special and admitted | accepted | connected to those addresses only |
| name that an entry matches | resolves only to special addresses not admitted | refused | refused |
| name that an entry matches | does not resolve within 5 s | accepted (its addresses are judged at each connection) | DNS failure, as before (502) |
| name that no entry matches | resolves to at least one admitted address | accepted | connected to the admitted addresses only |
| name that no entry matches | resolves to no admitted address | refused | refused |
| name that no entry matches | does not resolve within 5 s | refused (nothing proves it inside the list) | DNS failure, as before (502) |
| none (a purely mocked service) | any | accepted | no connection |
| a URL that cannot be read | list set | refused | refused by the HTTP client, as before (502) |

A refused relay answers 502 with a sentence that names `PROXY_ALLOWED_TARGETS` and is written in the request log like any relay; a refused availability check reports the target unreachable with the same reason.

**When saving checks.** After the editor's right to save the service, never before (the lookup would tell a user without it where a name resolves): creating a service, updating a service whose target changes, importing a configuration (each target that is not already the stored target of the same service). Restoring a backup, switching between mock and proxy, or editing rules does not check again: those targets are judged at each connection anyway, and an edit of a rule is not refused for a target saved before the list.

**Invalid list.** The server refuses to start, naming the entry: a value that is not UTF-8, a port (`host:8080`), a scheme or a path, a `*` other than a leading `*.`, a wildcard over an address, a range with bits set past its prefix (`10.0.0.1/8`) or a prefix over 32 (IPv4) or 128 (IPv6). A misspelt security setting never falls back to "everything allowed".

**Around the check.** With the list set, the relay ignores the system's proxy variables: it connects directly, so the addresses judged are the ones connected. Redirects stay unfollowed, so a backend cannot send the relay elsewhere.

## Consequences

- With the list set, saving a new target makes one DNS lookup of its name (5 s at most), a flow the [security model](../docs/en/security.md) lists.
- A deployment behind an HTTP proxy that sets the list loses that proxy for the relay: the relay must then reach its targets directly.
- Not seen: a cloud metadata endpoint outside link-local other than the two above (one appearing later needs its line in `SPECIAL`); addresses that embed an IPv4 address other than the mapped form (NAT64 `64:ff9b::/96`, 6to4, Teredo) are judged as IPv6 addresses; a NAT64 gateway translates only to global addresses (RFC 6052), and the others need a relay the operator set up. A name entry trusts every non-special address its name resolves to, the machine's own loopback included: whoever controls the DNS of a listed name chooses where the relay goes, within the non-special addresses.

## What would make this decision wrong

- An operator who needs the relay behind an HTTP proxy and the list together: the proxy's own address would then be judged, and the target's name sent to it unresolved, judged by name only.
- A connection seen at an address the list does not admit, by the tests that resolve real names and listen on real sockets (`src/engine/proxy/targets/tests.rs`).
