// Mimicway's server, as a library: the binary (main.rs) reads the environment and starts it, and the fuzz targets
// (fuzz/) call its parsers. Nothing is published from it; the binary is the product.
//
// No unsafe Rust, tests included: a test that changed the environment (unsafe since edition 2024) would change it for
// the tests running beside it, which is why settings are read through a lookup (settings.rs).
#![forbid(unsafe_code)]

//   auth/      Keycloak token validation, group permissions
//   models/    configuration schema (Service, Rule, Group...)
//   engine/    matching, HTTP proxy, templates, Rhai scripts
//   store/     YAML persistence, backups
//   server/    management API (/api/*), service interception, browser guard
//   settings   settings read from environment variables
//   i18n       server messages in the language of the request
//   messaging/ Kafka, compiled only with the "messaging-kafka" feature
//   tcp/       raw TCP mocks, compiled only with the "tcp-mock" feature
//   fuzzing    entry points of the fuzz targets (fuzz/), compiled by cargo-fuzz only
pub mod auth;
pub mod engine;
#[cfg(fuzzing)]
pub mod fuzzing;
pub mod i18n;
#[cfg(feature = "messaging-kafka")]
pub mod messaging;
pub mod models;
pub mod server;
pub(crate) mod settings;
pub mod store;
#[cfg(feature = "tcp-mock")]
pub mod tcp;
