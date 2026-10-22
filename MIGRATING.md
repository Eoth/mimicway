# Moving from lightMock to Mimicway

lightMock is now called **Mimicway**. It is the same program: the management API, the URLs of your mocked services, the environment variables, the configuration format and the data directory do not change. An existing `DATA_PATH` (with `mock-config.yaml`, `tcp-config.yaml` and `backups/`) is picked up as it is, and an exported configuration imports as it always did.

Only names change. Most of them need nothing from you; the table says which do.

| Before | Now | What to do |
|---|---|---|
| Binary `light-mock` | `mimicway` | Update scripts and service units that start it. |
| Image built as `lightmock` | `mimicway`, published as `ghcr.io/eoth/mimicway` | Use the new name; keep your volume (see [Docker](#docker)). |
| `RUST_LOG=light_mock=debug` | `RUST_LOG=mimicway=debug` | Nothing urgent: the former name is still read, with a warning in the log. Update it when convenient. |
| Default Kafka consumer group `lightmock` | `mimicway` | Set `KAFKA_CONSUMER_GROUP=lightmock` to keep consuming from your current offsets. |
| Language, theme and session saved by the browser | Same values, new keys | Nothing: they are moved on the first visit. |
| Kubernetes resources `lightmock`, volume `lightmock-data` | `mimicway`, `mimicway-data`, namespace `mimicway` | Keep your volume (see [Kubernetes](#kubernetes)). |
| Repository `github.com/Eoth/light-api-mock` | `github.com/Eoth/mimicway` | Nothing: old links and `git remote` URLs redirect. Update them when convenient. |
| Exported file `lightmock-config-<date>.json` | `mimicway-config-<date>.json` | Nothing: imports accept any file name. |
| Guide pages `docs/<French name>.md` | `docs/en/<English name>.md` and `docs/fr/<English name>.md` | Update bookmarks and links to the guide (see [Documentation](#documentation)). |

## Binary

Replace `light-mock` (or `light-mock.exe`) with `mimicway` (`mimicway.exe`) wherever you start it. Keep the same environment, in particular `DATA_PATH`, and it serves the same mocks.

## Docker

Keep the named volume that holds your data; only the image name changes:

```bash
docker run --rm -p 127.0.0.1:7342:7342 -v lightmock-data:/data ghcr.io/eoth/mimicway
```

With Compose, change the `image:` (or the `build:` tag) and leave the `volumes:` entry as it is.

## Kubernetes

The reference manifests now create a `mimicway` namespace and an empty `mimicway-data` volume. To keep the volume your configuration and backups live on today, apply them through a small overlay of your own instead, for instance `k8s/my-upgrade/kustomization.yaml`:

```yaml
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
# The namespace your lightMock runs in today.
namespace: entreprise-tools
resources:
  - ../ingress            # or ../gloo-edge
patches:
  # Mount the volume that holds your configuration and backups today, instead of a new empty one.
  - target:
      kind: Deployment
      name: mimicway
    patch: |-
      - op: replace
        path: /spec/template/spec/volumes/0/persistentVolumeClaim/claimName
        value: lightmock-data
  - patch: |-
      $patch: delete
      apiVersion: v1
      kind: PersistentVolumeClaim
      metadata:
        name: mimicway-data
```

Then:

```bash
NS=entreprise-tools

# 1. Stop lightMock: on SIGTERM it writes its pending changes, then releases the volume.
kubectl -n $NS scale deployment lightmock --replicas=0

# 2. Start Mimicway on the same volume.
kubectl apply -k k8s/my-upgrade
kubectl -n $NS rollout status deployment mimicway

# 3. Remove what lightMock used, except its volume.
kubectl -n $NS delete deployment/lightmock service/lightmock configmap/lightmock-config
kubectl -n $NS delete ingress/lightmock --ignore-not-found
```

With Gloo Edge, also delete the `lightmock` Upstream, RouteTable and VirtualService, and point the overlay at `../gloo-edge` (whose resources name the namespace in their specification: patch it there too, or keep the `mimicway` namespace and carry the configuration over by export and import, below).

To start clean in the new `mimicway` namespace instead, apply `k8s/ingress` as documented, then restore your configuration: export it from the old instance (Import/Export in the UI, or `GET /api/config`) and import it into the new one.

## Kafka

The consumer group used when `KAFKA_CONSUMER_GROUP` is not set is now `mimicway`. A new group starts from the latest messages: nothing is consumed twice, but messages sent while the instance was stopped are not consumed. To carry on exactly where lightMock stopped, set `KAFKA_CONSUMER_GROUP=lightmock`.

## Documentation

The user guide was a single French tree under French file names. It now exists in English and in French, with the same English file names in both languages, and each page links to its other language. Former addresses do not redirect; this is where each page went:

| Former page | English | French |
|---|---|---|
| `docs/index.md` | [docs/en/index.md](docs/en/index.md) | [docs/fr/index.md](docs/fr/index.md) |
| `docs/services.md` | [docs/en/services.md](docs/en/services.md) | [docs/fr/services.md](docs/fr/services.md) |
| `docs/groupes.md` | [docs/en/groups.md](docs/en/groups.md) | [docs/fr/groups.md](docs/fr/groups.md) |
| `docs/ping-de-disponibilite.md` | [docs/en/availability-check.md](docs/en/availability-check.md) | [docs/fr/availability-check.md](docs/fr/availability-check.md) |
| `docs/regles-de-matching.md` | [docs/en/matching-rules.md](docs/en/matching-rules.md) | [docs/fr/matching-rules.md](docs/fr/matching-rules.md) |
| `docs/reponses-et-templates.md` | [docs/en/responses-and-templates.md](docs/en/responses-and-templates.md) | [docs/fr/responses-and-templates.md](docs/fr/responses-and-templates.md) |
| `docs/scripts-rhai.md` | [docs/en/rhai-scripts.md](docs/en/rhai-scripts.md) | [docs/fr/rhai-scripts.md](docs/fr/rhai-scripts.md) |
| `docs/testeur-de-regle-et-conflits.md` | [docs/en/rule-tester-and-conflicts.md](docs/en/rule-tester-and-conflicts.md) | [docs/fr/rule-tester-and-conflicts.md](docs/fr/rule-tester-and-conflicts.md) |
| `docs/journal-des-requetes.md` | [docs/en/request-log.md](docs/en/request-log.md) | [docs/fr/request-log.md](docs/fr/request-log.md) |
| `docs/observation-de-trafic.md` | [docs/en/traffic-observation.md](docs/en/traffic-observation.md) | [docs/fr/traffic-observation.md](docs/fr/traffic-observation.md) |
| `docs/sauvegardes-et-restauration.md` | [docs/en/backups-and-restore.md](docs/en/backups-and-restore.md) | [docs/fr/backups-and-restore.md](docs/fr/backups-and-restore.md) |
| `docs/administration.md` | [docs/en/administration.md](docs/en/administration.md) | [docs/fr/administration.md](docs/fr/administration.md) |
| `docs/authentification.md` | [docs/en/authentication.md](docs/en/authentication.md) | [docs/fr/authentication.md](docs/fr/authentication.md) |
| `docs/messaging-kafka.md` | [docs/en/kafka-messaging.md](docs/en/kafka-messaging.md) | [docs/fr/kafka-messaging.md](docs/fr/kafka-messaging.md) |

The security model, which was only in English, is at [docs/en/security.md](docs/en/security.md) and [docs/fr/security.md](docs/fr/security.md). The README has a French version, [README.fr.md](README.fr.md).
