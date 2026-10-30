# Phasme — the design system of the Mimicway interface

Status: in force. Applies to every screen of the web interface: `src/tokens.css` holds its values, `src/app.css` its shared classes, and `src/tests/design-system.test.js` its rules.

## Why a design system of its own

Mimicway is a tool developers keep open for hours, next to the application it stands in for. What matters on its screens is the traffic: services, rules, requests, responses. Everything else should be read at a glance and then recede. A generic look (soft cards, a palette from a component kit, rounded everything) would compete with that, and would make Mimicway look like every other generated tool.

So the identity comes from what the product does. A mock imitates a real service closely enough for an application to take it for the real one, the way a stick insect imitates a twig. **Phasme** is French for the stick insect. Its colors are those of lichen and moss on bark; its one motif is the line an imitation is drawn with.

## Principles

- **The traffic is the subject.** Surfaces are low-chroma neutrals with the green cast of lichen; ink goes to names, paths, methods and statuses.
- **An imitation is drawn dashed, the real thing solid.** A mocked service, a mock rule, the mock choice of a rule: dashed, in moss. A service relayed to its target, a proxy rule: solid, in bark. The line says it as well as the color, so the mode reads in grey and for people who do not tell the two colors apart.
- **Flat.** Surfaces are separated by hairlines and by tone. Only what floats over the page casts a shadow: dialogs and the script autocompletion.
- **Dense, still readable.** Body text is 14 px, secondary text 13 px; forms and tables are tight enough to show a whole rule or a page of log without scrolling, and figures are tabular so that columns hold still.
- **Facts, not a tone.** Interface text states what is known and what follows from it: `No request has been captured for this service yet.`, `Read-only replay: no request is sent again`. No exclamation marks, no emoji, no reassurance.
- **Calm.** Transitions last at most 150 ms; `prefers-reduced-motion` removes them.
- **Quiet by default.** No web font and no request to anyone: the interface uses the system's own fonts, as the content security policy requires.

## Motifs

| Motif | Where | What it says |
|---|---|---|
| Mode stem | left edge of a service card and of a rule in the list | dashed moss: answered by a mock; solid bark: relayed to the real target |
| Mode badge | `MOCK` and `PROXY` badges, the rule list | the same, as a badge: dashed border for a mock, solid for a proxy |
| Mode choice | the action of a rule (Mock or Proxy) | the chosen card takes the line and tint of its mode |
| Mark | before the name in the header | a short dashed stem: the line Phasme draws imitations with |

A motif is never decorative: an element that carries one says something true about the service or the rule.

## Tokens

All values live in `src/tokens.css`, in two levels.

1. **Primitives** (`--lichen-*`, `--moss-*`, `--bark-*`, `--berry-*`, `--pollen-*`, `--lake-*`, `--shade-*`): raw colors named after what they are. `tokens.css` is the only file where a literal color may appear, and the only one that reads a primitive.
2. **Semantic tokens**: named after what they do. Color roles (`--color-*`) are redefined for each theme, on `:root, [data-theme="light"]` and `[data-theme="dark"]`; scales (`--space-*`, `--text-*`, `--line-*`, `--radius-*`, `--z-*`, `--duration-*`, `--size-*`) are the same in both. Components and `app.css` read semantic tokens only.

### Color roles

| Role | Used for | Light | Dark |
|---|---|---|---|
| `--color-bg` | the page | lichen 100 | lichen 950 |
| `--color-surface` | panels, cards, fields | lichen 0 | lichen 900 |
| `--color-sunken` | code, wells, the rule tester | lichen 150 | lichen 975 |
| `--color-hover` | a row or button under the pointer | lichen 150 | lichen 850 |
| `--color-border` | hairlines between surfaces (decorative) | lichen 300 | lichen 800 |
| `--color-control` | the boundary of a field or a button | lichen 500, 3.63:1 | lichen 600, 3.61:1 |
| `--color-text`, `--color-text-muted` | text, secondary text | 14.18:1, 6.60:1 | 14.64:1, 7.02:1 |
| `--color-primary` (+ `-hover`, `--color-on-primary`) | the main action, links | moss 600, 7.33:1 | moss 300, 8.34:1 |
| `--color-selected` | the background of a chosen item | moss 50 | moss 900 |
| `--color-focus` | the focus ring | moss 700, 9.25:1 | moss 200, 11.12:1 |
| `--color-mock` (+ `-bg`, `-text`) | what is imitated | moss | moss |
| `--color-proxy` (+ `-bg`, `-text`) | the real target | bark 600, 7.55:1 | bark 400, 7.22:1 |
| `--color-success`, `-warning`, `-danger`, `-info` (+ `-bg`, `-text`, `--color-on-*`) | states | moss, pollen, berry, lake | the same families, lighter |
| `--color-code-keyword`, `-function`, `-string`, `-number`, `-comment` | the pieces of a script in its editor | lake 700, moss 600, bark 700, berry 700, lichen 700 | lake 300, moss 300, bark 300, berry 300, lichen 400 |
| `--color-overlay`, `--shadow-popover`, `--shadow-overlay` | what floats | shade | shade |

Ratios are against the surface or background the role is set on. The test measures every declared pair in both themes and fails under WCAG 2.2 AA: 4.5:1 for text, 3:1 for the boundaries of controls, the focus ring and the mode lines.

### Scales

- **Space**: multiples of 4 px, `--space-N` being N × 4 px (`--space-1` to `--space-12`). The half steps `--space-0-5` (2 px) and `--space-1-5` (6 px) are for the inside of dense controls only: badges, chips, table cells.
- **Type**: a major second (ratio 1.125) from the 16 px root, rounded to the pixel: `--text-xs` 11, `--text-s` 13, `--text-m` 14, `--text-l` 16, `--text-xl` 18, `--text-2xl` 20, `--text-3xl` 23. The small end is close-set on purpose: body (14), secondary text (13) and uppercase badges (11) differ yet stay readable. Weights: regular, medium, strong (600, labels and titles), heavy (700, uppercase badges only). Line heights: `--leading-tight` for titles, `--leading-body` for text.
- **Fonts**: `--font-ui`, the system's interface font; `--font-code`, one monospace stack for every path, header, script, template and log value, without ligatures (`=>` stays two characters).
- **Lines**: `--line-thin` (1 px) separates, `--line-thick` (2 px) marks a choice and draws the focus ring, `--line-stem` (3 px) is the mode stem.
- **Corners**: `--radius-s` (2 px) for badges and code, `--radius-m` (3 px) for controls and panels. Only the switch is a pill (`--radius-pill`): its shape is its meaning. Index discs are `--radius-round`.
- **Layers**: `--z-popover`, `--z-modal`, `--z-skip-link`, from the page up.
- **Motion**: `--duration-quick` (120 ms, colors) and `--duration-move` (150 ms, the chevron of a group and the knob of a switch); both are 0 under `prefers-reduced-motion`.
- **Sizes**: `--size-control` and `--size-control-s` for square icon buttons, `--size-content` for the width of the page. A size that belongs to one component (a column of the log, the track of the switch) stays in that component.

## Shared classes

`src/app.css` holds every class that more than one component uses, once. A component styles only what is its own; the test fails when it styles a class of `app.css`, even in a descendant selector.

| Class | For |
|---|---|
| `.btn` with `.btn-primary`, `.btn-secondary`, `.btn-outline`, `.btn-danger`, `.btn-danger-outline`, `.btn-sm`; `.btn-xs` | text buttons, one primary per screen or form |
| `.btn-icon` with `.btn-icon-s`, `.btn-icon-xs`, `.btn-delete`, `.btn-edit`; `.btn-close` | square buttons holding a glyph, named by `aria-label`; the smallest is 24 px, the WCAG 2.2 target minimum |
| `.form-field`, `.form-row`, `.field-hint`, `.form-error`, `.form-actions` | fields with their label, hint and error |
| `.section`, `.section-help`, `.sub-section` | the fieldsets of a form and their parts |
| `.callout` with `.callout-warning`, `.callout-danger`; `.callout-title`, `.callout-list`, `.callout-actions` | what to weigh before going on, in the flow of a form |
| `.badge`, `.badge-pill` with `.badge-mock`, `.badge-proxy`, `.badge-success`, `.badge-info`, `.badge-error`, `.badge-unknown`, `.badge-testing`, `.badge-reachable`, `.badge-unreachable`, `.badge-expired`; `.method-badge` | a mode, a state, an HTTP method: a shape class, then a color class |
| `.modal-overlay`, `.modal-content`, `.modal-header`, `.modal-footer` | dialogs |
| `.data-breadcrumb`, `.collapsed-indicator`, `.preview-code` | the JSON and XML response builders |

## Focus

One ring everywhere, set once in `app.css`: a solid `--line-thick` outline in `--color-focus`, `--line-thick` away from the control. It is never removed. Where a container clips its overflow, the ring moves inside (`outline-offset` negative), as on the header of a service group. Dashes are kept for imitations, so a focus ring is never dashed.

## Guards

`src/tests/design-system.test.js` runs with `npm test` and fails on:

- a literal color (hexadecimal, color function or color name, a `var()` fallback included) in a component or a style sheet other than `tokens.css`, inline styles included;
- a `var(--…)` that nothing defines;
- a primitive read outside `tokens.css`;
- a font family, a font size, a line height, a spacing, a radius, a z-index or a shadow that does not come from a token;
- a role defined in one theme and not the other;
- a class of `app.css` styled again by a component;
- a declared pair of colors under its WCAG AA threshold, in either theme.

Each rule is first shown failing on a sample, so that a guard that stopped catching anything would fail too.

## Preview

`npm run design:preview` (in `frontend/`) opens `design-preview.html`, served by Vite and never built into the binary. It shows every color role with its value, every scale, the shared classes and the components that carry a motif (status badge, switch, service card, rule list, notifications), in the light and the dark theme side by side. The roles and steps are read from `tokens.css`, so a new token shows without editing the page. It is how a visual change is looked at before it is committed.

## Adding a component

1. Use the shared classes of `app.css` first (see the table above).
2. Style only what is specific to the component, in its `<style>` block, with semantic tokens: a color role, a step of a scale.
3. If no role fits, add one to both theme blocks of `tokens.css`, built from primitives, and declare its pairs in the contrast test. Never a literal color in a component, never a primitive.
4. If a second component needs the same style, move it to `app.css` rather than copying it.
5. Look at it in the preview and in the application, in both themes, at 1280 px and at 360 px, with the keyboard.

## Changing the look

- **Another palette**: change the primitives; the test reports the contrasts that no longer hold.
- **Another theme**: one block of color roles; no component changes.
- **Another design system**: replace `tokens.css`, the shared classes of `app.css` and this document; a component changes only if a role it reads disappears.
