# Kubernetes manifests

A [Kustomize](https://kustomize.io) base with everything Mimicway needs, and two ways to route traffic to it. Mimicway does not depend on any routing technology: use the overlay that matches your cluster, or keep the base and add your own routing.

| Directory | Content |
|---|---|
| `base/` | ConfigMap (environment, authentication on), PersistentVolumeClaim (64 MiB for the configuration and its backups), Deployment (one pod, probes, strict security context), Service (port 80 to 7342), NetworkPolicy (no outbound traffic but name resolution). No namespace, no routing. |
| `ingress/` | The base in the `mimicway` namespace, exposed by a standard `Ingress` on `mimicway.example.com`, over TLS. |
| `gloo-edge/` | The base in the `mimicway` namespace, exposed through Gloo Edge (`Upstream`, `RouteTable`, `VirtualService`), over TLS. |
| `local/` | The base in the `mimicway` namespace, without routing and without authentication, for a cluster of your own (kind, minikube, k3d), reached with `kubectl port-forward`. Never on a cluster shared with others: anything that reaches the Service could change every mock. |

The CI renders every overlay and refuses one that would route traffic to Mimicway without authentication or without TLS, let it connect beyond name resolution, or give the pod a service account token (`scripts/check-k8s-defaults.mjs`).

## Deploy

```bash
# Build and push the image, or use a published one.
docker build -t <registry>/mimicway:<version> .
docker push <registry>/mimicway:<version>

# Point the overlay at it, give it the certificate of its host, then apply.
cd k8s/ingress
kustomize edit set image mimicway=<registry>/mimicway:<version>
kubectl create namespace mimicway
kubectl -n mimicway create secret tls mimicway-tls --cert=tls.crt --key=tls.key
kubectl apply -k .

kubectl -n mimicway get pods -l app.kubernetes.io/name=mimicway
kubectl -n mimicway logs -l app.kubernetes.io/name=mimicway
```

Edit the host name in `ingress/ingress.yaml` (or `gloo-edge/virtualservice.yaml`), and the settings in `base/configmap.yaml` (see the configuration table of the [README](../README.md#configuration)). Authentication is on: set `KEYCLOAK_URL`, `KEYCLOAK_REALM` and `KEYCLOAK_CLIENT_ID` there (and `SUPER_ADMINS`), or the pod stops at startup with a message naming what is missing, and allow the pod to reach Keycloak (see [Network policy](#network-policy)). The `mimicway-tls` secret may also come from cert-manager or any tool that writes a `kubernetes.io/tls` secret.

On a cluster of your own, the `local` overlay needs neither Keycloak nor certificate:

```bash
kubectl create namespace mimicway
kubectl apply -k k8s/local
kubectl -n mimicway port-forward svc/mimicway 7342:80   # then open http://localhost:7342
``` To use another namespace, change it in the overlay's `kustomization.yaml`; for Gloo Edge, also in the three Gloo resources, which refer to it in their specification.

## What any routing must provide

Whatever exposes Mimicway (these overlays, a Helm chart of your own, a platform that generates routes from labels…) only has to meet three conditions:

1. **Mimicway is served from the root of a host.** Its UI loads `/assets/…`, `/runtime-config.json` and `/api/…` from the root, so it cannot be moved under a path prefix such as `/mimicway` (planned, see the [roadmap](../ROADMAP.md)).
2. **The pod's port (`PORT`, 7342) is reachable** from the routing layer, through the Service.
3. **`/api/health` answers the probes**; it never requires authentication.

The UI and the API are always served by the same pod. If your platform exposes them on two different hosts, set `API_BASE_URL` on the UI's host and `CORS_ALLOWED_ORIGINS` on the API's, as described in [Split UI and API](../README.md#split-ui-and-api); both routes then point to the same Service.

## Why one pod, and why `Recreate`

The configuration is one file on one volume, written by one process. A second replica, or a rolling update that starts the new pod before the old one stops, would let two processes write the same file and lose changes. The Deployment therefore runs a single replica with the `Recreate` strategy: on an update, the old pod receives `SIGTERM`, writes its pending changes and stops, then the new one starts. Expect a few seconds of downtime per update.

## Pod security

- `runAsNonRoot`, user 1000, no privilege escalation, every Linux capability dropped; `fsGroup: 1000` makes the data volume writable whatever owner the storage class gives it.
- Read-only root file system: only `/data` (the volume) is writable.
- Memory limit 256 MiB: idle, Mimicway uses a few MiB, but request bodies up to 10 MiB are buffered to evaluate rules.
- No service account token (`automountServiceAccountToken: false`): Mimicway never calls the Kubernetes API. The runtime's default seccomp profile filters its system calls.
- Mimicway serves plain HTTP: both routing overlays terminate TLS in front of it, with the `mimicway-tls` secret.

## Network policy

Editors choose where the proxy connects, so the base's `mimicway-egress` policy refuses every outbound connection of the pod but name resolution (port 53). Allow Keycloak and the backends you proxy with a policy of your own, applied next to it: network policies add up. For instance, Keycloak in the `keycloak` namespace and one backend outside the cluster:

```yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: mimicway-egress-allowed
  namespace: mimicway
spec:
  podSelector:
    matchLabels:
      app.kubernetes.io/name: mimicway
  policyTypes:
    - Egress
  egress:
    - to:
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: keycloak
      ports:
        - port: 8443
    - to:
        - ipBlock:
            cidr: 203.0.113.10/32
      ports:
        - port: 443
```

Network policies need a network plugin that enforces them (Calico, Cilium, and most managed clusters); without one, they are accepted and ignored. Inbound traffic is left to the routing in front of the Service.

## Persistence

`mock-config.yaml` lives on the volume mounted at `/data`, next to `backups/`. Writes are atomic (temporary file, then rename), so a crash never leaves a half-written configuration.
