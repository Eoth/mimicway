[Français](../fr/index.md)

# Mimicway user guide

Mimicway mocks or relays ("proxies") HTTP calls to a real service: test an application without depending on a real backend, or replay precise scenarios (errors, slowness, particular data) on demand. Everything is driven from a web interface, without restarts.

This page is a **quick overview**: a line or two per feature, with a link to the page that explains how to use it. If you are new to Mimicway, read this list once to know what exists, then come back to the detailed pages when needed.

> These pages are for the people who use and test with Mimicway (QA, developers, business analysts). Installation and configuration are in the [README](../../README.md), the architecture in [ARCHITECTURE.md](../../ARCHITECTURE.md); the security model is in [security.md](security.md).

![The Mimicway home screen with the list of services](screenshots/home-service-list.png)

## Services and routing

| Feature | In short |
|---|---|
| [Services and routing](services.md) | Each mocked service has its own URL (`/my-service/...`), with the address of the real backend, whether the mock is on, and the API type (REST or SOAP). |
| [Service groups](groups.md) | Gather related services (all the services of a team, for instance) under one URL prefix, with rights per group. |
| [Availability check](availability-check.md) | A button that checks whether the real backend can be reached over the network, without ever sending it a real request. |

## Rules and mocked responses

| Feature | In short |
|---|---|
| [Matching rules](matching-rules.md) | A service can hold several rules: each sets an HTTP method, a sub-path and conditions (on parameters, headers, body…) that decide which response to send. |
| [Responses and templates](responses-and-templates.md) | Build the response (JSON or XML) in a visual editor, with fake data (names, addresses…) and simulated failures and slowness (chaos mode). |
| [Rhai scripts](rhai-scripts.md) | For advanced cases: a short script computes values (dates, random or stable numbers, UUIDs…) that the response reuses. |
| [Rule tester and conflict detection](rule-tester-and-conflicts.md) | Check that a rule works against a real request already received, and get a warning when a new rule may conflict with an existing one. |

## Monitoring and diagnosis

| Feature | In short |
|---|---|
| [Request log](request-log.md) | The latest requests Mimicway received, in the interface: see why a rule matched, or did not. |
| [Traffic observation and rule suggestions](traffic-observation.md) | For a service in pure proxy mode: observe real traffic (on demand) and get mock rules suggested from the calls actually seen. |

## Backups and administration

| Feature | In short |
|---|---|
| [Backups and restore](backups-and-restore.md) | The configuration is backed up before each change; an earlier state is restored in one click. |
| [Administration (import, export, reset, dark mode, language)](administration.md) | Export or import the whole configuration as a file, reset everything, switch theme and language. |

## Security and integrations

| Feature | In short |
|---|---|
| [Authentication](authentication.md) | Optional: Mimicway can require a login (Keycloak), with different rights per user and group. |
| [Kafka messaging](kafka-messaging.md) | Optional: beyond HTTP, Mimicway can also answer Kafka messages (needs a build that includes it). |
| [Security model](security.md) | What Mimicway exposes, what it connects to, what it trusts, and how to harden a deployment. |
