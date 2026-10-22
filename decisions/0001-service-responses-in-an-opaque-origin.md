# 1. Service responses run in an opaque origin; tokens stay in memory

Accepted, 2026-10-22.

## Context

One HTTP port, and so one browser origin, serves the interface, the management API and the traffic of every mocked or proxied service (`src/server/mod.rs`: one router, the interface as its fallback). The interface and the API carry a content security policy, but the responses of the services were left exactly as configured or as the backend sent them (`src/server/browser_guard.rs`), and the interface kept the user's access token and refresh token in `localStorage`, where any script of that origin can read them.

Who writes a service response:

- any editor of the service: with authentication on, any user of the identity provider can create a group, becomes its admin, and creates services in it;
- a proxied backend, untrusted by design ([security model](../docs/en/security.md));
- the caller, without any right: templates insert request values as they are (`{{query.x}}`), so a rule answering `text/html` with a request value reflects whatever a link puts in it.

A response that a browser renders as a document (`text/html`, XHTML, SVG) therefore ran its scripts as the interface: it read the tokens of whoever opened the link, a super-admin as well as anyone, and called the management API with their rights; its calls were same-origin, so the browser guard, which refuses cross-site writes, saw nothing to refuse. Without authentication, the same script could rewrite or reset the configuration of the developer who opened it.

## Options

1. **Serve the services on another port or host than the interface.** The strongest separation, enforced by the browser for any content. But every service URL changes (or the interface's), along with the configuration of every application under test, the routes of every cluster and the documentation; the interface must also learn and show the second address. A large, breaking change.
2. **A `Content-Security-Policy: sandbox` header on every service response.** The browser renders such a document in an opaque origin of its own: its scripts may run, but they read none of the interface's storage, and their calls to the API leave from origin `null`, marked cross-site, which the browser guard already refuses for writes and CORS keeps unreadable. No URL changes. Clients that are not a browser rendering the response as a page (applications under test, `fetch`, server-to-server calls) ignore the header.
3. **Keep the tokens out of `localStorage`.** Alone it does not stop a script of the same origin, which can still call the API while the user's page is open, or open the interface in a window of the same origin and read it. It shortens what a stolen token is worth and leaves nothing on disk.

## Decision

Options 2 and 3 together.

- Every service response, mocked or proxied, carries `Content-Security-Policy: sandbox allow-downloads allow-forms allow-modals allow-popups allow-scripts`: everything a page may need except `allow-same-origin`, which would give it back the interface's origin, and `allow-popups-to-escape-sandbox`, which would give it to its popups. The header is added next to any policy the rule or the backend sets, never in place of it: browsers apply every policy they receive.
- The interface keeps the access token in memory only. It no longer keeps the refresh token, which it never used: nothing renewed a session, which ended when the access token expired. On load, it deletes the tokens that earlier versions left in `localStorage`. A reload of the page now asks to sign in again.

Option 1 stays possible as an option for deployments that need it, recorded in the [roadmap](../ROADMAP.md) (T12).

## Consequences

- A proxied web application that relies on its own origin (cookies, storage, same-origin calls from its pages) no longer works when browsed through Mimicway. Mimicway mocks the services that applications call; it is not a reverse proxy for browsing. Option 1 would serve that case.
- A browser without support for the `sandbox` directive (none of the current ones) would still run such a page as the interface, without finding a token in storage.
- The interface's own policy keeps `script-src 'self'`, which includes service paths; it loads no script from them, and it inserts no HTML it did not build, so nothing can make it do so.
- `POST /api/auth/login` still returns the refresh token to API clients, unchanged; the interface drops it.

## What would make this decision wrong

- A browser that renders a document from a response carrying `Content-Security-Policy: sandbox` in the origin of its URL: the end-to-end test that opens such a page and reads its origin would then fail.
- Users who need proxied web applications to keep their origin: option 1 then becomes the next step.
