// Raw TCP mocks for simple binary protocols: one message in, fixed bytes out (a proprietary heartbeat, a fixed
// handshake, a small stateless binary RPC). Compiled only with the "tcp-mock" feature, like `messaging/` for Kafka:
// without it, this module does not exist in the binary. Its schema and behavior live here, apart from
// `models::MockConfig` (see `config.rs` for why).
//
// No relay mode: a TCP relay without matching adds nothing a mock is for, and only puts Mimicway between a client
// and a server it could reach directly. So `TcpService` has no target, `TcpRule` has no proxy action, and a
// connection that matches no rule is simply closed: a TCP mock never connects anywhere.
//
// What it cannot do, by construction rather than for lack of time:
//   - LDAP/AD (bind, search, unbind): a real client sends several BER messages on one connection, each waiting for
//     its answer, while a connection here carries one message. Even with a loop, each BIND holds a message id and
//     a BER length that change on every call, so byte prefixes and regexes match only frozen single calls. It would
//     take a loop per connection and some awareness of BER framing (where a message ends).
//   - SMTP: the same need for several exchanges per connection, with a simpler text framing.
//   - Any protocol with state across messages.
//
// Reconfigured live through the API (`/api/tcp/services`, server/api.rs): `TcpRuntime::try_update()` checks and
// writes the new configuration under one lock, then restarts every TCP listener (no per-service diff: there are a
// handful of them). The HTTP server is never affected.
pub mod config;
pub mod hex;
pub mod listener;
pub mod matcher;
pub mod runtime;
pub mod validation;

pub use listener::{TcpServiceStatus, spawn_tcp_services};
pub use runtime::TcpRuntime;

/// The address tests listen on.
#[cfg(test)]
pub const LOOPBACK: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
