// Mimicway's server, as a library: the binary (main.rs) reads the environment and starts it, and the fuzz targets
// (fuzz/) call its parsers. Nothing is published from it; the binary is the product.
//
// No unsafe Rust in the shipped binary. Tests use it only to set environment variables (unsafe since edition 2024),
// which is why the attribute is limited to non-test builds.
#![cfg_attr(not(test), forbid(unsafe_code))]

//   auth/      Keycloak token validation, group permissions
//   models/    configuration schema (Service, Rule, Group...)
//   engine/    matching, HTTP proxy, templates, Rhai scripts
//   store/     YAML persistence, backups
//   server/    management API (/api/*), service interception, browser guard
//   i18n       server messages in the language of the request
//   messaging/ Kafka, compiled only with the "messaging-kafka" feature
//   tcp/       raw TCP mocks, compiled only with the "tcp-mock" feature
pub mod auth;
pub mod engine;
pub mod i18n;
#[cfg(feature = "messaging-kafka")]
pub mod messaging;
pub mod models;
pub mod server;
pub mod store;
#[cfg(feature = "tcp-mock")]
pub mod tcp;
