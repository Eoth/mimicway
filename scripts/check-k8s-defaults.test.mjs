// Tests of check-k8s-defaults.mjs: the reader of kustomize's output, a closed overlay that passes, and each way of
// opening one that fails. Run with: node --test scripts/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { checkOverlay, overlays, parseDocuments } from './check-k8s-defaults.mjs';

// What kubectl kustomize writes for an overlay with an Ingress, in its own layout (lists not indented under their key).
const CONFIG = `apiVersion: v1
data:
  AUTH_ENABLED: "true"
  PORT: "7342"
kind: ConfigMap
metadata:
  name: mimicway-config
`;
const DEPLOYMENT = `apiVersion: apps/v1
kind: Deployment
metadata:
  name: mimicway
spec:
  template:
    spec:
      automountServiceAccountToken: false
      containers:
      - image: mimicway
        name: mimicway
      securityContext:
        fsGroup: 1000
        seccompProfile:
          type: RuntimeDefault
`;
const POLICY = `apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: mimicway-egress
spec:
  egress:
  - ports:
    - port: 53
      protocol: UDP
    - port: 53
      protocol: TCP
  podSelector:
    matchLabels:
      app.kubernetes.io/name: mimicway
  policyTypes:
  - Egress
`;
const INGRESS = `apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: mimicway
spec:
  rules:
  - host: mimicway.example.com
  tls:
  - hosts:
    - mimicway.example.com
    secretName: mimicway-tls
`;
const VIRTUAL_SERVICE = `apiVersion: gateway.solo.io/v1
kind: VirtualService
metadata:
  name: mimicway
spec:
  sslConfig:
    secretRef:
      name: mimicway-tls
      namespace: mimicway
  virtualHost:
    domains:
    - mimicway.example.com
`;

function check(...parts) {
  return checkOverlay('overlay', parseDocuments(parts.join('---\n')));
}

test('the reader follows kustomize: nested maps, lists at their key, quoted and plain scalars', () => {
  const [config, deployment, policy] = parseDocuments([CONFIG, DEPLOYMENT, POLICY].join('---\n'));
  assert.deepEqual(config.data, { AUTH_ENABLED: 'true', PORT: '7342' });
  assert.equal(deployment.spec.template.spec.containers[0].name, 'mimicway');
  assert.equal(deployment.spec.template.spec.securityContext.seccompProfile.type, 'RuntimeDefault');
  assert.deepEqual(policy.spec.egress, [
    {
      ports: [
        { port: '53', protocol: 'UDP' },
        { port: '53', protocol: 'TCP' },
      ],
    },
  ]);
  assert.deepEqual(policy.spec.policyTypes, ['Egress']);
});

test('the reader refuses what it cannot read rather than guessing', () => {
  for (const text of ['spec:\n  hosts: [a, b]\n', 'data:\n  script: |\n    run\n', 'a: &anchor x\n']) {
    assert.throws(() => parseDocuments(text), /unsupported YAML/, text);
  }
});

test('an overlay with authentication, TLS on its routes, DNS-only egress and a hardened pod passes', () => {
  assert.deepEqual(check(CONFIG, DEPLOYMENT, POLICY, INGRESS), []);
  assert.deepEqual(check(CONFIG, DEPLOYMENT, POLICY, VIRTUAL_SERVICE), []);
});

test('an overlay without routing may turn authentication off', () => {
  assert.deepEqual(check(CONFIG.replace('"true"', '"false"'), DEPLOYMENT, POLICY), []);
});

test('a routed overlay without authentication fails', () => {
  assert.deepEqual(check(CONFIG.replace('"true"', '"false"'), DEPLOYMENT, POLICY, INGRESS), [
    'overlay: routes traffic to Mimicway without AUTH_ENABLED: "true"',
  ]);
  assert.deepEqual(check(CONFIG.replace('  AUTH_ENABLED: "true"\n', ''), DEPLOYMENT, POLICY, VIRTUAL_SERVICE), [
    'overlay: routes traffic to Mimicway without AUTH_ENABLED: "true"',
  ]);
});

test('a route without TLS fails', () => {
  const plainIngress = INGRESS.slice(0, INGRESS.indexOf('  tls:'));
  const plainVirtualService = VIRTUAL_SERVICE.replace(/ {2}sslConfig:\n(?: {4}.*\n)+/, '');
  assert.deepEqual(check(CONFIG, DEPLOYMENT, POLICY, plainIngress), [
    'overlay: Ingress mimicway is served without TLS',
  ]);
  assert.deepEqual(check(CONFIG, DEPLOYMENT, POLICY, plainVirtualService), [
    'overlay: VirtualService mimicway is served without TLS',
  ]);
});

test('egress beyond name resolution, or no egress policy at all, fails', () => {
  const open = POLICY.replace('    - port: 53\n      protocol: TCP\n', '    - port: 443\n      protocol: TCP\n');
  const everywhere = POLICY.replace(/ {2}egress:\n(?: {2}[- ].*\n)+/, '  egress:\n  - {}\n');
  assert.deepEqual(check(CONFIG, DEPLOYMENT, open), [
    'overlay: NetworkPolicy mimicway-egress allows egress beyond name resolution',
  ]);
  assert.deepEqual(check(CONFIG, DEPLOYMENT, everywhere), [
    'overlay: NetworkPolicy mimicway-egress allows egress beyond name resolution',
  ]);
  assert.deepEqual(check(CONFIG, DEPLOYMENT), ['overlay: no NetworkPolicy restricts the egress of the pod']);
  assert.deepEqual(check(CONFIG, DEPLOYMENT, POLICY.replace('  - Egress\n', '  - Ingress\n')), [
    'overlay: no NetworkPolicy restricts the egress of the pod',
  ]);
});

test('a pod with a service account token or without the default seccomp profile fails', () => {
  assert.deepEqual(check(CONFIG, DEPLOYMENT.replace('      automountServiceAccountToken: false\n', ''), POLICY), [
    'overlay: the pod mounts a service account token',
  ]);
  assert.deepEqual(check(CONFIG, DEPLOYMENT.replace('RuntimeDefault', 'Unconfined'), POLICY), [
    'overlay: the pod runs without the RuntimeDefault seccomp profile',
  ]);
  assert.deepEqual(check(CONFIG, POLICY), ['overlay: no Deployment named mimicway']);
});

test('every directory with a kustomization but the base is an overlay to check', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'k8s-overlays-'));
  try {
    for (const name of ['base', 'ingress', 'new-one', 'notes']) mkdirSync(path.join(dir, name));
    for (const name of ['base', 'ingress', 'new-one']) writeFileSync(path.join(dir, name, 'kustomization.yaml'), '');
    assert.deepEqual(overlays(dir), ['ingress', 'new-one']);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
