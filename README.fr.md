[English](README.md)

# Mimicway

[![CI](https://github.com/Eoth/mimicway/actions/workflows/ci.yml/badge.svg)](https://github.com/Eoth/mimicway/actions/workflows/ci.yml) [![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/Eoth/mimicway/badge)](https://scorecard.dev/viewer/?uri=github.com/Eoth/mimicway)

Simulez ou relayez n'importe quelle API HTTP, une règle à la fois, depuis une interface web. Un seul binaire autonome : ni base de données, ni agent, ni compte cloud.

Mimicway se place entre l'application que vous testez et les services qu'elle appelle. Chaque service déclaré reçoit son propre espace d'URL sur Mimicway ; chaque requête qui l'atteint reçoit soit la réponse d'une règle que vous avez écrite (réponse fixe, templatée ou scriptée, avec latence et erreurs en option), soit est transmise au vrai backend. Tout se modifie à chaud depuis l'interface ou l'API REST, sans rien redémarrer.

- **Mock et proxy côte à côte** : un service entier, ou une seule règle, peut basculer entre une réponse simulée et le vrai backend. Simulez seulement les appels dont vous avez besoin ; laissez passer le reste.
- **Des règles qui lisent la requête** : conditions sur les paramètres de chemin, la query, les en-têtes, le corps JSON, le corps XML (XPath, SOAP) et les champs de formulaire, combinées en ET/OU.
- **Des réponses qui ont l'air vraies** : templates avec valeurs de la requête et données factices, constructeur visuel JSON/XML, scripts Rhai optionnels pour des valeurs calculées ou déterministes, mode Chaos pour la latence et les erreurs.
- **Du trafic réel aux mocks** : observez ce qu'un service relayé renvoie vraiment et acceptez les règles suggérées au lieu de les écrire à la main.
- **Conçu pour être audité** : aucune télémétrie, quatre flux sortants documentés, local uniquement par défaut, aucun code `unsafe`, dépendances vérifiées en CI. Voir [La confiance en bref](#la-confiance-en-bref).

> Mimicway s'appelait lightMock jusqu'à la version 0.1. La mise à jour conserve vos données, vos URL et vos réglages ; [MIGRATING.md](MIGRATING.md) (en anglais) liste les quelques noms à mettre à jour.

L'interface est disponible en anglais et en français ; ajouter une langue tient en un fichier (voir [Traductions](#traductions)).

![La liste des services dans l'interface de Mimicway](docs/fr/screenshots/home-service-list.png)

## La confiance en bref

Mimicway est pensé pour les réseaux d'entreprise où chaque flux sortant doit être justifié. En résumé :

| Question | Réponse |
|---|---|
| Contacte-t-il un serveur de l'éditeur ? | Non. Il n'y a ni télémétrie, ni vérification de mise à jour, ni serveur d'éditeur. |
| À quoi se connecte-t-il ? | Seulement à ce que vous configurez : le backend d'un service relayé, le test TCP de ce backend quand vous cliquez sur « Tester », votre Keycloak si l'authentification est activée, vos brokers Kafka si cette fonctionnalité est compilée et activée. |
| Qui peut l'atteindre ? | Par défaut, il n'écoute que sur `127.0.0.1`. Les conteneurs choisissent d'écouter sur toutes les interfaces avec `BIND_ADDRESS=0.0.0.0`. |
| Une page web peut-elle le piloter ? | Non. L'API de gestion refuse les écritures intersites, les origines CORS étrangères et, en mode local, les requêtes adressées à un nom qui n'est pas la boucle locale (DNS rebinding). |
| Garde-t-il des secrets issus du trafic ? | Non. `Authorization`, les cookies et les en-têtes de clé d'API sont remplacés par `[redacted]` dans les journaux, l'observation et les suggestions ; les URL perdent leurs identifiants. |
| Un script peut-il s'échapper ? | Les scripts Rhai n'ont accès ni aux fichiers, ni au réseau, ni à `eval`, et sont bornés en opérations, en taille de chaîne et en profondeur. |
| Du code non sûr ? | `#![forbid(unsafe_code)]` hors tests. |
| Chaîne d'approvisionnement ? | `Cargo.lock` et `package-lock.json` versionnés, actions de CI et images de base épinglées par empreinte ; `cargo-deny` (vulnérabilités, licences, sources), `npm audit`, Trivy et gitleaks tournent en CI. Les versions publiées livrent des SBOM, des attestations de provenance et une image signée. |

Le détail, avec le code qui appuie chaque affirmation : [modèle de sécurité](docs/fr/security.md). Un parcours guidé du code pour un relecteur, avec des commandes pour vérifier les affirmations vous-même : [REVIEWING.md](REVIEWING.md) (en anglais). Signaler une vulnérabilité : [SECURITY.md](SECURITY.md) (en anglais).

## Démarrage rapide

### Avec Docker

```bash
docker run --rm -p 7342:7342 -v mimicway-data:/data ghcr.io/eoth/mimicway
```

L'image est publiée pour amd64 et arm64 à chaque version, signée et accompagnée de sa provenance de compilation ([vérifier une version](SECURITY.md#verifying-a-release)). Pour la construire vous-même : `docker build -t mimicway .`.

### Binaire

Chaque [version](https://github.com/Eoth/mimicway/releases) fournit un binaire en un seul fichier pour Linux (x86_64 et arm64, statique), macOS (Intel et Apple Silicon) et Windows, avec l'interface incluse. Décompressez-le et lancez `./mimicway`.

Ouvrez <http://localhost:7342>.

### Depuis les sources

Prérequis : Rust 1.85+ (édition 2024), Node.js 20+ avec npm. Des scripts d'amorçage les installent : [Windows](scripts/bootstrap-windows.ps1), [Linux/macOS](scripts/bootstrap-linux.sh).

```bash
cd frontend && npm ci && npm run build && cd ..
cargo build --release
./target/release/mimicway          # mimicway.exe sous Windows
```

Construire l'interface d'abord l'intègre au binaire : `target/release/mimicway` se suffit alors à lui-même, et garde ses données dans `./data` par défaut (voir [Configuration](#configuration)).

### Votre premier mock

Créez un service nommé `demo` avec une règle, puis appelez-le :

```bash
curl -X POST http://localhost:7342/api/services \
  -H "Content-Type: application/json" \
  -d '{
    "name": "demo",
    "listen_path": "/v1/users/{id}",
    "real_target_url": "",
    "is_mocked": true,
    "rewrite_directory_urls": false,
    "group_name": null,
    "wsdl_mode": "auto",
    "rules": [{
      "name": "hello",
      "method": "GET",
      "sub_path": null,
      "action": "mock",
      "conditions": { "all_of": [], "any_of": [] },
      "response": {
        "status": 200,
        "headers": [{ "name": "Content-Type", "value": "application/json" }],
        "body": [{ "type": "Template", "template": "{\"id\":\"{{path.id}}\",\"name\":\"{{fake.FirstName}}\"}" }]
      }
    }]
  }'

curl http://localhost:7342/demo/v1/users/42
# {"id":"42","name":"..."}
```

Le même service se construit en une minute dans l'interface ; sur une instance vide, le bouton **Charger un exemple** de l'écran d'accueil crée un service `users-api` à essayer (`GET /users-api/users/42`). Pour une vue plus complète, importez [examples/devops-toolchain.json](examples/devops-toolchain.json) (mocks de GitLab, Jenkins, Wiki.js et Keycloak) avec le bouton Import de l'interface, ou avec `curl -X PUT http://localhost:7342/api/config -H "Content-Type: application/json" --data @examples/devops-toolchain.json`.

## Concepts

**Service.** Un point d'entrée nommé. Il est exposé sous `/{name}/{listen_path}`, ou `/{group_code}/{name}/{listen_path}` quand il appartient à un groupe. Un `listen_path` vide capte tout ce qui est sous `/{name}/`. Quand une requête est transmise, le préfixe `/{group_code}/{name}` est retiré et le reste est ajouté à `real_target_url`.

**Règle.** Un service contient une liste ordonnée de règles. Chaque règle a sa propre méthode HTTP, un `sub_path` optionnel, des conditions et une action : répondre par un mock, ou transmettre au backend. La première règle qui correspond l'emporte ; quand aucune ne correspond, Mimicway répond 404 et le journal des requêtes l'enregistre.

**Interrupteur mock / proxy.** Avec `is_mocked` désactivé, un service transmet chaque requête à `real_target_url` et ses règles attendent. Activé, les règles répondent ; `action: proxy` sur une règle ne transmet que les requêtes que cette règle matche (mock partiel). Un service sans cible est purement mocké.

| Service | listen_path | Règle | URL à appeler |
|---|---|---|---|
| `insee` | `/v4/sirene/{siret}` | `GET` | `GET /insee/v4/sirene/44306184100047` |
| `accounts` | `/login` | `POST` | `POST /accounts/login` |
| `users` | *(vide)* | `GET`, sub_path `/{id}` | `GET /users/42` |

**Groupe.** Des services peuvent être réunis dans un groupe : affichés ensemble dans l'interface, préfixés dans les URL par le code de 5 caractères du groupe et, quand l'authentification est activée, gérés par les administrateurs et les membres du groupe.

## Fonctionnalités

| Domaine | Ce que vous obtenez | Guide |
|---|---|---|
| Services et routage | Espace d'URL par service, REST ou SOAP (les requêtes WSDL peuvent contourner le mock), réécriture des URL de répertoire | [Services](docs/fr/services.md) |
| Groupes | Préfixe d'URL commun, administrateurs et membres par groupe | [Groupes](docs/fr/groups.md) |
| Règles de correspondance | Méthode, sous-chemin, conditions sur le chemin, la query, les en-têtes, le JSON, le XML/XPath, le formulaire ; ET/OU | [Règles de correspondance](docs/fr/matching-rules.md) |
| Réponses | Templates, constructeur visuel JSON/XML, données factices, mode Chaos (latence, taux d'erreur) | [Réponses et templates](docs/fr/responses-and-templates.md) |
| Scripts | Jusqu'à trois blocs Rhai en bac à sable par règle, valeurs déterministes par graine, dates, aides JSON/XML | [Scripts Rhai](docs/fr/rhai-scripts.md) |
| Testeur de règle et conflits | Rejouer un brouillon de règle contre une requête capturée avec un verdict par condition ; avertissement quand une nouvelle règle en chevauche une autre | [Testeur de règle](docs/fr/rule-tester-and-conflicts.md) |
| Journal des requêtes | Les 200 dernières requêtes : mode, règle correspondante (ou aucune), statut, et la requête capturée pour le testeur de règle | [Journal des requêtes](docs/fr/request-log.md) |
| Observation de trafic | Observer un service relayé et transformer ce qu'il renvoie vraiment en règles | [Observation de trafic](docs/fr/traffic-observation.md) |
| Test de disponibilité | Un test de connexion TCP vers le backend, jamais une requête HTTP | [Test de disponibilité](docs/fr/availability-check.md) |
| Sauvegardes | Rotation automatique avant chaque changement, restauration en un clic | [Sauvegardes](docs/fr/backups-and-restore.md) |
| Administration | Import, export, réinitialisation, mode sombre | [Administration](docs/fr/administration.md) |
| Authentification | Connexion Keycloak optionnelle, rôles par groupe, super-admins | [Authentification](docs/fr/authentication.md) |
| Kafka (optionnel) | Les mêmes règles appliquées aux messages Kafka, avec un journal des messages et un simulateur | [Kafka](docs/fr/kafka-messaging.md) |
| TCP brut (optionnel) | Réponses fixes à des protocoles binaires, reconnus par préfixe ou par expression régulière | [Fonctionnalités optionnelles](#fonctionnalités-optionnelles) |

Tous les guides : [docs/fr/index.md](docs/fr/index.md).

## Configuration

Tout se règle par des variables d'environnement ; aucune n'est obligatoire.

| Variable | Défaut | Rôle |
|---|---|---|
| `PORT` | `7342` | Port HTTP. |
| `BIND_ADDRESS` | `127.0.0.1` | Interface d'écoute. `0.0.0.0` (ou `::`) accepte les connexions distantes ; l'image de conteneur la fixe. |
| `DATA_PATH` | `./data` | Répertoire de `mock-config.yaml` et de ses `backups/`. |
| `STATIC_DIR` | *(l'interface intégrée au binaire)* | Un répertoire d'où servir l'interface à la place, pour développer l'interface ou en servir une version personnalisée. Un binaire construit sans l'interface lit `./frontend/dist`. |
| `RUST_LOG` | `mimicway=info` | Filtre des journaux, par exemple `mimicway=debug`. |
| `BACKUP_MAX_COUNT` | `5` | Sauvegardes conservées dans `{DATA_PATH}/backups/` avant rotation. |
| `API_BASE_URL` | *(vide)* | Où l'interface appelle l'API quand elles sont servies depuis des origines différentes ; voir [Interface et API séparées](#interface-et-api-séparées). |
| `CORS_ALLOWED_ORIGINS` | *(vide)* | Origines, séparées par des virgules, autorisées à appeler l'API de gestion depuis un navigateur, en plus de l'origine de l'interface. Nécessaire seulement avec `API_BASE_URL`. Les services simulés répondent toujours à toute origine. |
| `PROXY_READ_TIMEOUT_SECS` | `120` | Plus long silence accepté d'un backend relayé entre deux lectures ; au-delà, le client reçoit 504. |
| `REDACT_HEADERS` | *(vide)* | Noms d'en-têtes supplémentaires (séparés par des virgules) dont les valeurs sont masquées dans les journaux, l'observation et les suggestions, en plus de `Authorization`, `Proxy-Authorization`, `Cookie`, `Set-Cookie`, `X-Api-Key`, `X-Auth-Token`, `X-Amz-Security-Token`. |
| `REQUEST_LOG_MAX_BODY_SIZE` | `16384` | Octets de chaque corps conservés dans le journal des requêtes. |
| `TRAFFIC_OBSERVATION_MAX_BODY_SIZE` | `16384` | Octets de chaque corps conservés par l'observation de trafic. |
| `TRAFFIC_OBSERVATION_MAX_BUFFER_SIZE` | `10485760` | Plus grand corps de requête ou de réponse (d'après son `Content-Length`) que l'observation de trafic met en mémoire pour capturer un échange ; les corps plus grands ou sans taille sont transmis au fil de l'eau et non observés. |
| `TRAFFIC_OBSERVATION_MAX_KEYS` | `200` | Formes de requête distinctes observées par service. |
| `TRAFFIC_OBSERVATION_SAMPLES_PER_KEY` | `8` | Échanges conservés par forme de requête. |
| `TRAFFIC_OBSERVATION_MIN_SAMPLES` | `3` | Échanges nécessaires avant qu'une règle soit suggérée. |
| `AUTH_ENABLED` | `false` | Active l'authentification Keycloak. `KEYCLOAK_URL`, `KEYCLOAK_REALM` et `KEYCLOAK_CLIENT_ID` deviennent alors obligatoires ; le serveur refuse de démarrer sans eux. |
| `KEYCLOAK_URL` | *(vide)* | URL de base que Mimicway utilise pour joindre Keycloak. |
| `KEYCLOAK_REALM` | *(vide)* | Realm. |
| `KEYCLOAK_CLIENT_ID` | *(vide)* | Client pour lequel les jetons doivent être émis (`azp` ou `aud`). |
| `KEYCLOAK_ISSUER` | *(URL du realm)* | `iss` attendu des jetons, quand Keycloak les émet sous une autre URL que `KEYCLOAK_URL` (derrière un proxy, par exemple). |
| `SUPER_ADMINS` | *(vide)* | Noms d'utilisateurs, séparés par des virgules, qui ont tous les droits, y compris réinitialiser et restaurer. |
| `SHOW_RESET_BUTTON` | `false` | Affiche le bouton de réinitialisation quand l'authentification est désactivée. Affichage seulement : le serveur décide qui peut réinitialiser. |
| `KAFKA_ENABLED` | `false` | Démarre le consommateur Kafka (binaire construit avec `messaging-kafka`). |
| `KAFKA_BROKERS` | *(vide)* | Brokers, séparés par des virgules. |
| `KAFKA_CONSUMER_GROUP` | `mimicway` | Groupe de consommateurs. |
| `KAFKA_LISTEN_TOPIC` | *(vide)* | Topic consommé. |
| `KAFKA_REPLY_TOPIC` | *(vide)* | Topic sur lequel la réponse simulée est publiée ; rien n'est publié s'il est vide. |
| `MESSAGE_LOG_TTL_MS` | `86400000` | Durée de conservation du journal des messages Kafka. |
| `MESSAGE_LOG_MAX_BODY_SIZE` | `16384` | Octets de chaque message conservés dans ce journal. |
| `TCP_MOCK_MAX_MESSAGE_SIZE` | `16384` | Plus grand message lu par un mock TCP brut (binaire construit avec `tcp-mock`). |

Une valeur invalide (un `BIND_ADDRESS` mal formé, l'authentification activée sans ses réglages Keycloak) arrête le serveur au démarrage avec un message qui indique la variable à corriger, et le code de sortie 2.

## API de gestion

Tout est sous `/api`, en JSON à l'entrée comme à la sortie. Les messages d'erreur suivent l'en-tête `Accept-Language` de la requête (anglais par défaut, français disponible). Avec l'authentification activée, chaque route sauf `/api/health`, `/api/auth/status`, `/api/auth/login` et `/api/auth/validate` exige un jeton bearer ; voir [REVIEWING.md](REVIEWING.md#authorization-matrix) pour savoir qui peut appeler quoi.

| Méthode | Chemin | Rôle |
|---|---|---|
| GET | `/api/health` | Sonde de vivacité. |
| GET, PUT | `/api/config` | Configuration complète : lire (filtrée selon ce que l'appelant peut voir), remplacer. |
| DELETE | `/api/config/reset` | Supprimer tous les services et groupes (super-admin ; une sauvegarde protégée est conservée). |
| GET | `/api/config/backups` | Lister les sauvegardes (super-admin). |
| POST | `/api/config/restore/{file}` | Restaurer une sauvegarde (super-admin). |
| GET, POST | `/api/services` | Lister, créer des services hors groupe. |
| GET, PUT, DELETE | `/api/services/{name}` | Lire, remplacer, supprimer un service hors groupe. |
| GET, PUT, DELETE | `/api/groups/{group}/services/{name}` | La même chose pour un service d'un groupe. |
| PUT | `…/{name}/toggle` | Basculer entre mock et proxy. |
| POST | `…/{name}/ping` | Test de disponibilité TCP du backend. |
| PUT | `…/{name}/rules/reorder` | Réordonner les règles. |
| POST, DELETE | `…/{name}/observe` | Démarrer, arrêter l'observation de trafic. |
| GET | `…/{name}/suggestions` | Règles suggérées à partir du trafic observé. |
| GET | `/api/observation/status` | Services en cours d'observation. |
| POST | `/api/rule-test` | Tester un brouillon de règle contre une requête capturée. |
| POST | `/api/rule-conflicts` | Chevauchements entre un brouillon de règle et les autres règles du service. |
| POST | `/api/script/validate` | Compiler un script Rhai. |
| GET | `/api/logs` | Journal des requêtes (`?limit=`, filtré selon ce que l'appelant peut voir). |
| GET, POST | `/api/groups` | Lister, créer des groupes. |
| GET, PUT, DELETE | `/api/groups/{name}` | Lire, modifier, supprimer un groupe. |
| PUT | `/api/groups/{name}/members` | Fixer les administrateurs et les membres d'un groupe. |
| POST | `/api/auth/login`, `/api/auth/validate` | Connexion Keycloak, vérification de jeton. |
| GET | `/api/auth/me`, `/api/auth/status` | Utilisateur courant, authentification activée ou non. |
| GET, POST | `/api/messaging/status`, `/api/messaging/logs`, `/api/messaging/simulate` | Kafka (fonctionnalité `messaging-kafka`). |
| GET, POST, PUT, DELETE | `/api/tcp/status`, `/api/tcp/services[/{name}]` | Mocks TCP bruts (fonctionnalité `tcp-mock`). |

`…/{name}` désigne `/api/services/{name}` ou `/api/groups/{group}/services/{name}`. Un service d'un groupe n'est joignable que par la seconde forme.

## Templates et scripts

Un corps de réponse est fait de fragments. Dans un fragment `Template`, les accolades simples sont littérales (du JSON ou du XML ordinaire), et `{{ }}` contient une expression :

| Expression | Exemple de résultat |
|---|---|
| `{{path.siret}}`, `{{query.page}}`, `{{header.x-request-id}}` | Des valeurs de la requête |
| `{{body.customer/id}}`, `{{xpath.Envelope/Body/order/id}}` | Une valeur d'un corps JSON (JSON Pointer) ou d'un corps XML/SOAP (chemin sans préfixe d'espace de noms) |
| `{{path.siret \| first(9)}}` | `443061841` |
| `{{fake.Email}}` | Une donnée factice |
| `{{uuid}}`, `{{now_ms}}`, `{{now_iso}}`, `{{now_epoch}}`, `{{seq}}` | Des valeurs générées |
| `{{script.total}}` | Une valeur renvoyée par le script de la règle |

Pipes : `lower`, `upper`, `trim`, `capitalize`, `first(n)`, `last(n)`, `substr(start, len)`, `default("x")`, `replace("a", "b")`, `prepend("x")`, `append("x")`, `length`.

Types de données factices : `FirstName`, `LastName`, `Email`, `CompanyName`, `StreetName`, `DatePast`, `DateFuture`, `TimestampMs`, `BoolRandom`, `LoremSentence`, et des formats français (`PhoneNumberFR`, `CityFR`, `PostcodeFR`, `FullAddressFR`, `CountryFR`, `IbanFR`, `Siren`, `Siret`). Le constructeur visuel ajoute les entiers dans une plage (`Integer{min,max}`).

Une règle peut aussi exécuter jusqu'à trois scripts [Rhai](https://rhai.rs) (`pre_script`, `script`, `post_script`) qui lisent la requête et renvoient une valeur ou un objet utilisé par le template. En plus du langage lui-même, ils disposent de `random_int`, `seeded_int`, `seeded_pick` (même entrée, même réponse), `uuid`, `fake`, `now_ms`, `now_iso`, `year`, `date_now`, `date_past`, `date_future`, `parse_date`, `parse_json`, `to_json`, `parse_xml_items` et `xml_element`. Les scripts s'exécutent dans un bac à sable : aucun accès aux fichiers ni au réseau, pas d'`eval`, au plus 10 000 opérations, des chaînes de 1 Mo, des tableaux de 1 000 éléments, 32 niveaux d'appel. Référence complète avec exemples : [Scripts Rhai](docs/fr/rhai-scripts.md).

## Déploiement

### Conteneur

Le [Dockerfile](Dockerfile) construit l'interface, l'intègre au binaire, et copie ce fichier unique dans une image vide par ailleurs (`scratch` : ni système d'exploitation, ni shell), exécutée par un utilisateur non privilégié (uid 1000). L'image écoute sur toutes les interfaces (`BIND_ADDRESS=0.0.0.0`) et garde ses données dans `/data` : montez-y un volume.

```yaml
# compose.yaml
services:
  mimicway:
    build: .
    ports: ["7342:7342"]
    volumes: ["mimicway-data:/data"]
volumes:
  mimicway-data:
```

### Kubernetes

[k8s/](k8s/) contient une base Kustomize (un Deployment à une seule réplique avec un système de fichiers racine en lecture seule, sans élévation de privilèges et sans aucune capacité, un PersistentVolumeClaim, un Service) et deux overlays pour y router le trafic : un `Ingress` standard, ou Gloo Edge. Voir [k8s/README.md](k8s/README.md) (en anglais).

```bash
cd k8s/ingress
kustomize edit set image mimicway=<registry>/mimicway:<version>
kubectl create namespace mimicway
kubectl apply -k .
```

Mimicway doit être servi depuis la racine d'un hôte, et par une seule réplique : la configuration vit dans un seul fichier sur un seul volume.

### Interface et API séparées

Par défaut, l'interface appelle l'API sur sa propre origine (`/api/...`), ce qui fonctionne dès qu'un même Mimicway sert les deux. Si votre ingress route l'API vers une autre origine, fixez sur l'instance qui sert l'interface :

```bash
API_BASE_URL=https://mimicway-api.example.com
```

et sur l'instance qui sert l'API :

```bash
CORS_ALLOWED_ORIGINS=https://mimicway.example.com
```

L'interface lit `API_BASE_URL` au démarrage depuis `GET /runtime-config.json`, si bien qu'une même image sert tous les environnements sans reconstruction. Vérifiez-le avec `curl https://mimicway.example.com/runtime-config.json`.

### Derrière un proxy

Mimicway ne termine pas TLS ; placez-le derrière votre proxy inverse ou votre ingress habituel. Quand l'authentification est activée, utilisez HTTPS de bout en bout pour l'interface, puisqu'elle envoie des mots de passe à `/api/auth/login`.

## Sauvegardes

Avant chaque changement, le `mock-config.yaml` précédent est copié dans `{DATA_PATH}/backups/` (les `BACKUP_MAX_COUNT` derniers sont conservés). Une réinitialisation écrit aussi une copie dans `backups/protected/`, gardée 30 jours quelle que soit la rotation. Les changements s'appliquent immédiatement en mémoire et atteignent le disque en arrière-plan ; sur `SIGTERM` ou Ctrl+C, le serveur termine l'écriture avant de s'arrêter.

Restaurez depuis l'interface (bouton Sauvegardes, super-admins), ou à la main : arrêtez Mimicway, copiez une sauvegarde par-dessus `mock-config.yaml`, puis redémarrez-le.

## Fonctionnalités optionnelles

Les deux sont exclues de la compilation par défaut : leur code et leurs dépendances ne sont pas dans le binaire standard.

- **Kafka** (`cargo build --release --features messaging-kafka`, nécessite cmake, une chaîne de compilation C et, sous Linux, les en-têtes de libcurl (`libcurl4-openssl-dev`) pour construire librdkafka) : consomme `KAFKA_LISTEN_TOPIC`, compare chaque message aux mêmes règles et templates que le HTTP (scripts exceptés), publie éventuellement la réponse, tient un journal des messages et propose un simulateur dans l'interface. Voir [Kafka](docs/fr/kafka-messaging.md).
- **TCP brut** (`--features tcp-mock`) : écoute sur les ports que vous déclarez, sur la même interface que le HTTP (`BIND_ADDRESS`), et répond à chaque message par des octets fixes, choisis par préfixe hexadécimal ou par expression régulière. Pas de mode relais : un mock TCP ne se connecte jamais nulle part. La configuration vit dans `{DATA_PATH}/tcp-config.yaml`.

## Développement

```bash
cargo test                               # tests unitaires et d'intégration Rust
cargo test --features tcp-mock
cargo clippy --all-targets -- -D warnings
cd frontend && npm test                  # Vitest
cd frontend && npm run test:e2e          # Playwright, contre un Mimicway lancé sur :7342
```

Pour travailler sur l'interface avec rechargement à chaud, lancez le binaire, puis `cd frontend && npm run dev` et ouvrez <http://localhost:5173>. `build.rs` intègre `frontend/dist` quand il existe ; `cargo build` sans interface construite donne un binaire qui sert `STATIC_DIR` (`./frontend/dist` par défaut).

La CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) vérifie le formatage, clippy et les tests avec chaque fonctionnalité, les tests et la construction de l'interface, la suite de bout en bout contre le vrai binaire, `cargo-deny`, `npm audit`, une construction d'image analysée par Trivy, et une recherche de secrets ; chaque job tourne quand un fichier qu'il vérifie change, et une modification de la documentation ne lance que les contrôles de documentation et la recherche de secrets.

Plan du code :

```
src/
  main.rs       démarrage, erreurs de configuration, arrêt propre
  server/       routeur Axum, API de gestion, interception, garde navigateur, journaux, observation, suggestions
  engine/       matcher, proxy, moteur de templates, bac à sable Rhai
  auth/         client Keycloak (validation JWKS), middleware, rôles
  store/        persistance YAML, écriture différée, sauvegardes
  models/       schéma de configuration
  i18n.rs       catalogues des messages du serveur
  messaging/    Kafka (fonctionnalité messaging-kafka)
  tcp/          mocks TCP bruts (fonctionnalité tcp-mock)
frontend/       interface Svelte 5, tests Vitest et Playwright
k8s/            manifestes Kubernetes
examples/       configurations prêtes à importer
```

### Traductions

L'anglais est la langue source : chaque message est écrit une fois, en anglais, dans le code. Chaque autre langue est un catalogue indexé par le texte anglais : [frontend/src/locales/fr.json](frontend/src/locales/fr.json) pour l'interface, [src/locales/fr.json](src/locales/fr.json) pour les messages de l'API. Des tests extraient chaque message du code et échouent quand un catalogue en oublie un, en garde un inutilisé ou perd un paramètre ; une exécution en pseudo-langue échoue sur tout texte de l'interface resté non traduit. Pour ajouter une langue, ajoutez les deux catalogues et déclarez la langue dans `frontend/src/lib/i18n.svelte.js` et `src/i18n.rs`. La documentation existe en anglais (`docs/en/`, `README.md`) et en français (`docs/fr/`, `README.fr.md`), avec les mêmes pages, titres et captures dans chaque langue ; la CI le vérifie.

## Dépannage

| Symptôme | Solution |
|---|---|
| Un mock répond 404 | L'URL doit commencer par le nom du service (`/{name}/...`), et par le code du groupe pour un service groupé. La page du service affiche l'URL exacte. |
| Une méthode ou un sous-chemin n'est pas simulé | Les méthodes appartiennent aux règles : ajoutez une règle pour cette méthode et ce sous-chemin. |
| L'interface affiche une page blanche | Le binaire a été construit avant l'interface : construisez l'interface, puis à nouveau le binaire, ou pointez `STATIC_DIR` sur `frontend/dist` (un chemin absolu sous Windows). Le journal de démarrage indique d'où vient l'interface. |
| Le port 7342 est déjà utilisé | Fixez `PORT`, ou arrêtez l'autre processus. |
| D'autres machines n'atteignent pas Mimicway | Il écoute sur `127.0.0.1` par défaut : fixez `BIND_ADDRESS=0.0.0.0`. |
| 403 « Requête inter-sites refusée » | L'interface est servie depuis une autre origine : ajoutez-la à `CORS_ALLOWED_ORIGINS`. |
| 403 « ce Mimicway n'écoute que sur la machine locale » | Appelez-le via `localhost` ou `127.0.0.1`, ou écoutez sur une autre interface. |
| Connexion refusée alors que le mot de passe est bon | Keycloak émet les jetons sous une autre URL que `KEYCLOAK_URL`, ou pour un autre client : fixez `KEYCLOAK_ISSUER`, vérifiez `KEYCLOAK_CLIENT_ID` (le journal du serveur nomme la cause). |
| `link.exe not found` sous Windows | Installez les Visual Studio Build Tools avec la charge de travail C++. |
| La compilation Kafka échoue sous Windows avec une erreur de longueur de chemin | Fixez `CARGO_TARGET_DIR` à un chemin court comme `C:\lm-target`. |

## Projet

- [ROADMAP.md](ROADMAP.md) : ce qui est prévu, et dans quel ordre.
- [MIGRATING.md](MIGRATING.md) : mettre à jour une installation de lightMock.
- [CHANGELOG.md](CHANGELOG.md) : les changements par version.
- [SECURITY.md](SECURITY.md) : versions prises en charge et signalement privé des vulnérabilités.
- [CONTRIBUTING.md](CONTRIBUTING.md) : proposer un changement, lancer les vérifications et ajouter une langue ; [code de conduite](CODE_OF_CONDUCT.md).
- Licence : [MIT](LICENSE).

Ces documents de projet sont en anglais.
