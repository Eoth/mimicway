// Fails when a Kubernetes overlay of k8s/ would deploy Mimicway open to the network. Each overlay is rendered with
// `kubectl kustomize`, then:
// - an overlay that routes traffic to it (Ingress, Gloo Edge VirtualService) must turn authentication on and serve
//   that route over TLS: tokens and passwords cross it, and without authentication whoever reaches it changes every
//   mock and uses the proxy;
// - every overlay keeps the pod's egress to name resolution (a NetworkPolicy whose rules all name port 53), so that the
//   proxy reaches only what a deployment allows next to it;
// - every overlay keeps the pod without a service account token and under the runtime's default seccomp profile.
// Every directory of k8s/ with a kustomization.yaml but base/ is an overlay: a new one is checked without being listed.
// Run with: node scripts/check-k8s-defaults.mjs (kubectl on the PATH); its tests: node --test scripts/*.test.mjs
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * Reads the YAML that kustomize writes: block mappings and sequences, plain or quoted scalars, empty `[]` and `{}`.
 * Anything else (flow collections, block scalars, anchors) throws, so that a check never passes on a misread file.
 */
export function parseDocuments(text) {
  return text
    .split(/^---$/m)
    .map((part) =>
      part
        .split('\n')
        .filter((line) => line.trim() !== '' && !line.trimStart().startsWith('#'))
        .map((line) => ({ indent: line.length - line.trimStart().length, text: line.trim() })),
    )
    .filter((lines) => lines.length > 0)
    .map((lines) => {
      const cursor = { lines, at: 0 };
      const value = parseBlock(cursor, lines[0].indent);
      if (cursor.at < lines.length) throw new Error(`unexpected line: ${lines[cursor.at].text}`);
      return value;
    });
}

function parseBlock(cursor, indent) {
  const line = cursor.lines[cursor.at];
  return line.text.startsWith('- ') || line.text === '-' ? parseSequence(cursor, indent) : parseMapping(cursor, indent);
}

function parseMapping(cursor, indent) {
  const map = {};
  while (cursor.at < cursor.lines.length) {
    const line = cursor.lines[cursor.at];
    if (line.indent !== indent || line.text.startsWith('- ')) break;
    const match = /^("[^"]*"|'[^']*'|[^:]+):(?: (.*))?$/.exec(line.text);
    if (!match) throw new Error(`not a key: ${line.text}`);
    const key = scalar(match[1]);
    cursor.at += 1;
    if (match[2] !== undefined && match[2] !== '') {
      map[key] = scalar(match[2]);
      continue;
    }
    const next = cursor.lines[cursor.at];
    if (next && next.indent > indent) map[key] = parseBlock(cursor, next.indent);
    else if (next && next.indent === indent && next.text.startsWith('- ')) map[key] = parseSequence(cursor, indent);
    else map[key] = null;
  }
  return map;
}

function parseSequence(cursor, indent) {
  const items = [];
  while (cursor.at < cursor.lines.length) {
    const line = cursor.lines[cursor.at];
    if (line.indent !== indent || !line.text.startsWith('- ')) break;
    const rest = line.text.slice(2);
    if (/^("[^"]*"|'[^']*'|[^:"']+):(?: |$)/.test(rest)) {
      // "- key: value" opens a mapping whose keys are aligned on the text after the dash.
      cursor.lines[cursor.at] = { indent: indent + 2, text: rest };
      items.push(parseMapping(cursor, indent + 2));
    } else {
      items.push(scalar(rest));
      cursor.at += 1;
    }
  }
  return items;
}

function scalar(raw) {
  const text = raw.trim();
  if (text === '[]') return [];
  if (text === '{}') return {};
  if (/^".*"$/.test(text)) return JSON.parse(text);
  if (/^'.*'$/.test(text)) return text.slice(1, -1).replaceAll("''", "'");
  if (/^[[{|>&*!]/.test(text)) throw new Error(`unsupported YAML: ${text}`);
  return text;
}

const ROUTING_KINDS = ['Ingress', 'VirtualService'];

/** The problems of one rendered overlay, `name` being its directory. */
export function checkOverlay(name, documents) {
  const problems = [];
  const ofKind = (kind) => documents.filter((doc) => doc?.kind === kind);
  const config = ofKind('ConfigMap').find((doc) => doc.metadata?.name === 'mimicway-config');
  const routes = documents.filter((doc) => ROUTING_KINDS.includes(doc?.kind));

  if (routes.length > 0 && config?.data?.AUTH_ENABLED !== 'true') {
    problems.push(`${name}: routes traffic to Mimicway without AUTH_ENABLED: "true"`);
  }
  for (const route of routes) {
    const tls =
      route.kind === 'Ingress' ? (route.spec?.tls ?? []).length > 0 : Boolean(route.spec?.sslConfig?.secretRef?.name);
    if (!tls) problems.push(`${name}: ${route.kind} ${route.metadata?.name} is served without TLS`);
  }

  const egress = ofKind('NetworkPolicy').filter(
    (doc) =>
      doc.spec?.podSelector?.matchLabels?.['app.kubernetes.io/name'] === 'mimicway' &&
      (doc.spec?.policyTypes ?? []).includes('Egress'),
  );
  if (egress.length === 0) problems.push(`${name}: no NetworkPolicy restricts the egress of the pod`);
  for (const policy of egress) {
    for (const rule of policy.spec?.egress ?? []) {
      const ports = rule.ports ?? [];
      if (ports.length === 0 || ports.some((port) => String(port.port) !== '53')) {
        problems.push(`${name}: NetworkPolicy ${policy.metadata?.name} allows egress beyond name resolution`);
      }
    }
  }

  const deployment = ofKind('Deployment').find((doc) => doc.metadata?.name === 'mimicway');
  const pod = deployment?.spec?.template?.spec;
  if (!pod) {
    problems.push(`${name}: no Deployment named mimicway`);
  } else {
    if (pod.automountServiceAccountToken !== 'false') {
      problems.push(`${name}: the pod mounts a service account token`);
    }
    if (pod.securityContext?.seccompProfile?.type !== 'RuntimeDefault') {
      problems.push(`${name}: the pod runs without the RuntimeDefault seccomp profile`);
    }
  }
  return problems;
}

/** The overlays of `k8sDir`: its directories with a kustomization.yaml, but base/. */
export function overlays(k8sDir) {
  return readdirSync(k8sDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && entry.name !== 'base')
    .filter((entry) => existsSync(path.join(k8sDir, entry.name, 'kustomization.yaml')))
    .map((entry) => entry.name)
    .sort();
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const k8sDir = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'k8s');
  const problems = [];
  const names = overlays(k8sDir);
  for (const name of names) {
    const rendered = execFileSync('kubectl', ['kustomize', path.join(k8sDir, name)], { encoding: 'utf8' });
    problems.push(...checkOverlay(name, parseDocuments(rendered)));
  }
  if (problems.length > 0) {
    console.error(problems.join('\n'));
    process.exit(1);
  }
  console.log(`The ${names.length} overlays (${names.join(', ')}) keep Mimicway closed by default.`);
}
