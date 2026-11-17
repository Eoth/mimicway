[Français](../fr/traffic-observation.md)

# Traffic observation and rule suggestions

For a service in **pure proxy** mode (not mocked at all yet), Mimicway can watch the traffic really exchanged with the real backend and **suggest mock rules** from what it saw, instead of having you write them all by hand. Nothing starts by itself: you turn it on explicitly, service by service.

![The "Rule suggestions from real traffic" panel, observation off](screenshots/observation-panel-off.png)

## Turning observation on

On the page of a service **in proxy mode** (`is_mocked` off), the "Rule suggestions from real traffic" panel offers **"Observe this service"**. Once clicked, Mimicway captures (within bounds, see "Limits" below) the request and the response of each call relayed to the real backend, as long as observation stays on.

![The panel with observation on and a "Stop observing" button](screenshots/observation-panel-on.png)

This changes **nothing** in how the proxy behaves (the request is still relayed as it is): it only adds a capture on the side, which you can turn off at any time.

## The trap Mimicway avoids

Two calls to the same endpoint (same method, same path) can legitimately get different answers: a parameter, a header or an identifier that changes also changes the real backend's answer. A rule generated naively from the **first** call seen would silently break every other case.

Mimicway therefore waits until it has seen **several calls** to a route (see the next section) before suggesting anything, then:

- When every observed response is the same, it suggests a rule **without conditions**.
- When the responses vary and a path parameter, a query parameter, a field of the body or a header **predicts** which response comes back (each of its values always gets the same response), it suggests **one rule per response**, with its condition. A response reached by several values, such as a directory giving the same address for two names, gets one rule that accepts any of them ("any of" conditions).
- When the responses vary **and no field explains it reliably**, it suggests no rule. A message reports the variance, rather than a rule that would silently break some calls.

When several fields would explain the responses, Mimicway keeps the one with the **fewest different values**: the name that decides wins over an identifier that changes at every call. Repetition is the evidence: a field explains the responses only when **each of its values was seen at least twice**, always with the same response; a value seen once fits any sample, as an identifier unique to each call (a message identifier in a SOAP header) would. One exception keeps the suggestions of earlier versions: a path parameter, a query parameter or a field at the top of a JSON object may explain the responses with values seen once when each response has its own value.

A call answered with a **server error** (a `5xx` status) that no field explains is set aside: Mimicway suggests the rules for the other calls, if there are still enough of them and more of them than of errors, and the suggestion says how many calls were left out and with which status. Five identical answers and one `500` give one rule for the five; three answers and fifty `503` give none, since errors are what that route mostly does.

![Suggestions: two conditional rules derived from observed traffic](screenshots/observation-suggestions.png)

## Paths grouped into routes

Calls that differ only by an identifier, such as `/orders/1` and `/orders/2`, are one route for Mimicway: it groups them under a pattern, `/orders/{id}`, and suggests rules for the pattern, which match every such path. A part of the path becomes a parameter in two cases:

- It looks like an identifier: a number or a date, a UUID, a hexadecimal hash, a long code mixing letters and digits. One call is enough, since a rule written for a single identifier rarely outlives the session it was observed in.
- At least two different words at the same place are answered with documents of the same structure (same status and media type, same JSON keys or same XML elements), as a service directory answers for each service it knows: `/directory/restOrders` and `/directory/restStock` become `/directory/{name}`. Words answered with documents of different structures, such as `/orders` and `/customers`, stay two routes, and so do words answered in plain text.

The minimum number of calls applies to the route, not to each path: three calls to three different orders are enough. Each suggestion names its pattern and a few of the paths it groups.

The pattern's parameters are the first fields Mimicway tries to explain the responses with. When each order gets its own answer, it suggests one rule per order, with a condition on `{id}`. When the responses differ **only** by the value that tells them apart, because the backend repeats it (a directory answering `http://backend/ws/restOrders` for `restOrders`), it suggests a **single** rule whose response repeats that value through an expression such as `{{path.name}}`, so that it also answers the values it never saw. The text around the expression is the observed response as it was; the expression is the only template in it.

![A single suggested rule for every name of a directory, its response repeating the name](screenshots/observation-route-suggestion.png)

A literal path next to a pattern, such as `/users/me` next to `/users/{id}`, is suggested first: rules apply in their order, and the more specific one must come first.

## SOAP operations and fields of the body

Every operation of a SOAP service is a `POST` to the same address: what tells two calls apart is the element the `Body` of the envelope holds. When every call of a route names **one** operation, Mimicway suggests rules **per operation**: each suggestion names its operation, and each of its rules carries the condition `Envelope/Body/<operation>` exists, in the syntax of XPath conditions (namespace prefixes are ignored), so that it never answers another operation. A route where one call names no operation, or several, is not split. Observation keeps the calls of each operation apart, as it does for each path: one busy operation never pushes the calls of the others out.

Inside an operation, and for any body, the fields of the body are tried like the other fields of the request:

- in an XML body, the text of each element that holds no other element, read at the first element of its path, as an XPath condition reads it (`Envelope/Body/getStatus/ref`); a CDATA section is read as text;
- in a JSON body, each string, number or boolean at any depth, as a JSON pointer (`/order/shipping/kind`).

A rule suggested from such a field matches the calls it was drawn from: the field is read as the condition written from it reads the request. Reading a body is bounded: the first 64 fields of a body, 16 levels deep at most, a path of 256 bytes and a value of 256 bytes at most; anything beyond gives no field. A request body cut at the size limit of observation (`TRAFFIC_OBSERVATION_MAX_BODY_SIZE`) gives no field and no operation, and the suggestion says how many bodies were cut: raise that setting when a field you expect is never used. A request body in another encoding than UTF-8 (a SOAP envelope declared `ISO-8859-1`, say) gives no field and no operation either, and the suggestion counts those bodies: the conditions of a rule read the bytes the mock receives, and an XPath condition reads only UTF-8.

## Responses that repeat the request

Many operations answer with what they were asked: the reference of the request, its dates, each line of a list. One rule per value seen would answer a request never seen with the values of another one. When the responses of a route (or of an operation) differ **only** where they repeat values of their own request, Mimicway suggests a **single** rule without condition whose response reads each value from the request:

- **Values**: a path or query parameter, a header, a field of an XML or JSON body. The response keeps the observed text and holds an expression where each value stands, for instance `{{xpath.Envelope/Body/GetStatus/ref}}` or `{{body./order/ref}}`, followed by the pipes that escape the value for the format of the response (`replace("&", "&amp;")` and the like for XML, quotes and backslashes for JSON), so that a value with `&` or a quote keeps the response valid. Several fields can be repeated at once. A field whose value was the same in every call stays as written: nothing shows that the response repeats it.
- **Lists**: when the request holds a repeated element (`<slot>` elements, a JSON array of objects) and the response a block per item, each block being the same text around values of the item of the same rank, the suggested rule comes with a **script** that reads the items (`parse_xml_items`, `parse_json`) and writes one block per item, each value escaped for the format of the response; the response reads the result with `{{script.items}}`. A request with no item gets the text around the list alone. "Use this suggestion" fills the form with the script, switched on, and the advanced template.

The panel names, in plain words, the fields the response repeats and the list it writes one block per item of.

The guarantee: such a rule is suggested only if, rendered by Mimicway itself (templates and script) for each observed request, it gives back the observed response **byte for byte**; otherwise you get the suggestions described above, one rule per value. Text observed in a response never becomes a template nor code: it stays in literal fragments of the response, or in string literals of the script, escaped.

Limits: values of 256 bytes at most, 64 per response; one list per response, two calls with items at least, and items of 32 fields at most. The script runs within the budget of any script (10,000 operations), which holds a list of about a hundred items of two fields: a larger list observed gives no such rule, and at run time a script out of budget answers the text around the list without its blocks. A response whose values are not written as their format escapes them (in a CDATA section, say) is generalized only for the values read as they are.

## Using a suggestion

**"Refresh the suggestions"** reads the traffic observed since observation was turned on and computes the suggestions again (nothing refreshes in the background). **"Use this suggestion"** does **not** create the rule: it fills the usual rule form (method, sub-path, conditions, response), so that you review, adjust and save it like any other rule.

![The rule form filled from a suggestion](screenshots/observation-suggestion-prefilled-form.png)

## Following the services a response points to

Many backends answer with the addresses of others: a service directory giving the address of each service it knows, links between resources, a redirect, the address in a WSDL. The application then calls those addresses **directly**, past Mimicway, where they can be neither observed nor mocked.

After a refresh, the panel lists under **"Backends named in the responses"** each backend the observed responses point to, by origin (`http://host:port`), with the routes that named it and an example address. Mimicway looks in the bodies and in the `Location`, `Content-Location` and `Link` headers, and leaves out XML namespace and schema addresses (`xmlns`, `targetNamespace`, `xsi:schemaLocation`) and Mimicway's own address.

![Backends named by a directory, each with a button to put a proxy service in front of it](screenshots/observation-linked-backends.png)

1. For a backend that no service reaches yet, **"Create a proxy service in front of it"** opens the usual service form, filled with a name taken from the host and the origin as target, in the observed service's group. Nothing is created until you save it, through the usual checks.
2. Back on the observed service, **refresh the suggestions**. In the suggested rules, every address of a backend that a service of Mimicway reaches (its target is the start of the address, and its listen path accepts the rest) now points to that service in Mimicway; each rule says which services it sends the application to.
3. Once the directory is mocked with that rule, the application it serves calls Mimicway, whose new proxy service can be observed in turn, and so on down the chain.

The address of Mimicway is written in the body as `{{header.x-forwarded-proto | default("http")}}://{{header.host}}`, followed by the service's path and the rest of the observed address as it was: the rule answers with the address the application used to reach Mimicway, whatever its host name and port. Headers are never templates, so a `Location` or `Link` header gets the address Mimicway was reached at while observing.

Only the **suggested rules** are rewritten. The traffic Mimicway relays is never changed: while it is a proxy, the directory still answers the real addresses.

## Requirements and limits

- Only for a service in **pure proxy** mode (`is_mocked` off): "Observe this service" is refused otherwise, since observation only makes sense on traffic relayed to a real backend.
- A call is captured only when the sizes of its request and response are known in advance (a `Content-Length` header, or no body). A response streamed in chunks (`Transfer-Encoding: chunked`, no announced size) is still relayed normally but not observed.
- A path of more than 32 segments is not suggested.
- The number of observations kept is bounded, per path (per operation for a SOAP address) and overall (see the `TRAFFIC_OBSERVATION_*` settings in the README): beyond that, the oldest are replaced by the newest, so memory never grows without limit.
- Credentials are never captured: `Authorization`, cookies, API-key headers and any header listed in `REDACT_HEADERS` are kept as `[redacted]`, and suggested rules never copy them.
- Observation stops by itself when its service is deleted, renamed or moved to another group, and on every full reset of the configuration. What it observed is then forgotten, as when you stop observing: observing again starts from nothing, and a service created later under the same name never inherits another one's traffic.
- Like the [request log](request-log.md), nothing is written to disk: a restart of Mimicway starts with no observation.
