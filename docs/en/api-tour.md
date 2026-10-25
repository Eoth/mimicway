[Français](../fr/api-tour.md)

# The API in five steps

Everything the interface does goes through the [management API](../../README.md#management-api), so a mock can also be written by a script or a CI job. This page shows it with `curl`, in one pass: a mocked service, a rule that reads the request, a rule forwarded to a backend, the request log. The backend is a second service of the same instance: nothing leaves the machine. Paste the blocks in order into one bash terminal, from the root of a checkout where `cargo build --release` has run. Under each command, the line that starts with `#` is its answer, `…` standing for fake data or long values; CI runs these commands as written and fails when an answer changes.

<!-- tour:start -->

## 1. Start an empty instance

It listens on port 7343, on this machine only, and keeps its data in a temporary directory, so an instance already on 7342 and its `./data` are left alone; from a release archive, run `./mimicway`. `c` is `curl` that prints the status code after the body.

```bash
PORT=7343 RUST_LOG=warn DATA_PATH="$(mktemp -d)" ./target/release/mimicway &
until curl -sf localhost:7343/api/health >/dev/null; do sleep 0.2; done
c() { curl -sS -w ' → %{http_code}\n' -H 'Content-Type: application/json' "$@"; }
```

## 2. Mock a service

`users` answers `GET /users/{id}` with the id it was called with and a fake first name. `svc` is the service with a `RULES` placeholder: each call below sends the whole service, its rules in place of `RULES`.

```bash
user='{"name":"user","method":"GET","action":"mock","conditions":{},
  "response":{"body":[{"type":"Template","template":"{\"id\":\"{{path.id}}\",\"name\":\"{{fake.FirstName}}\"}"}]}}'
svc='{"name":"users","listen_path":"/{id}","real_target_url":"http://localhost:7343/backend","is_mocked":true,"wsdl_mode":"auto","rules":[RULES]}'
c localhost:7343/api/services -d "${svc/RULES/$user}"
# {"name":"users",…} → 201
c localhost:7343/users/42
# {"id":"42","name":"…"} → 200
```

## 3. Add a rule with a condition

Rules are tried in order: `vip` answers the requests whose query has `plan=vip`, `user` all the others. `PUT` replaces the whole service.

```bash
vip='{"name":"vip","method":"GET","action":"mock","conditions":{"all_of":[{"source":{"type":"QueryParam","key":"plan"},
  "operator":{"type":"Eq","value":"vip"}}]},"response":{"body":[{"type":"Literal","value":"VIP offer"}]}}'
c -X PUT localhost:7343/api/services/users -d "${svc/RULES/$vip,$user}"
# {"name":"users",…} → 200
c 'localhost:7343/users/42?plan=vip'
# VIP offer → 200
```

## 4. Forward one rule

`backend` stands for the real API that `users` points to (its `real_target_url`). With `"action":"proxy"`, the `vip` rule sends the requests it matches there, while `user` goes on mocking the others.

```bash
c localhost:7343/api/services -d '{"name":"backend","listen_path":"","real_target_url":"","is_mocked":true,"wsdl_mode":"auto",
  "rules":[{"name":"all","method":"GET","action":"mock","conditions":{},"response":{"body":[{"type":"Literal","value":"from the backend"}]}}]}'
# {"name":"backend",…} → 201
c -X PUT localhost:7343/api/services/users -d "${svc/RULES/${vip/mock/proxy},$user}"
# {"name":"users",…"action":"proxy",…} → 200
c 'localhost:7343/users/42?plan=vip'
# from the backend → 200
```

## 5. Read the request log, then clean up

The last request went through `users` in proxy mode, to `/backend/42`. Stopping the instance leaves nothing behind but its temporary directory.

```bash
c 'localhost:7343/api/logs?limit=1'
# [{…"path":"/users/42","mode":"proxy",…"target_url":"http://localhost:7343/backend/42",…}] → 200
c -X DELETE localhost:7343/api/services/users
# → 204
kill %1
```

<!-- tour:end -->
