[English](../en/security.md)

# Modèle de sécurité

Cette page s'adresse à qui doit décider si Mimicway peut tourner dans son environnement : ce qu'il expose, avec quoi il communique, à quoi il fait confiance, ce qu'il protège et ce qu'il laisse au déploiement. Chaque affirmation ci-dessous est appliquée par le code et couverte par des tests ; les noms de fichiers et de tests sont donnés pour que chacune puisse être vérifiée.

## En un paragraphe

Mimicway est un processus unique, sans base de données, sans télémétrie et sans trafic sortant qui lui soit propre. Il écoute sur un port HTTP (et, en option, sur des ports TCP bruts que vous configurez), y sert son interface et son API de gestion, et répond au trafic des services que vous définissez : à partir de règles (mock) ou en transmettant au backend que vous avez configuré (proxy). Il n'appelle que ce backend, Keycloak si vous activez l'authentification, et Kafka si vous le compilez et l'activez.

## Ce qu'il expose

| Surface | Chemin ou port | Qui est censé l'appeler |
|---|---|---|
| API de gestion | `/api/...` | L'interface et vos automatisations. Protégée par des jetons Keycloak quand `AUTH_ENABLED=true`. |
| Coquille de l'interface | `/`, `/index.html`, `/assets/...`, `/runtime-config.json` | Les navigateurs. Publique exprès : elle affiche l'écran de connexion. |
| Trafic des services | `/{service}/...`, `/{code du groupe}/{service}/...` | Les applications testées. N'exige jamais de jeton Mimicway : il transporte les identifiants propres aux applications. |
| Mocks TCP bruts | les ports que vous configurez, sur l'interface `BIND_ADDRESS` (fonctionnalité `tcp-mock`, désactivée par défaut) | Les clients de protocoles binaires. |

Par défaut, le binaire n'écoute que sur `127.0.0.1` (`BIND_ADDRESS`). L'image de conteneur fixe `0.0.0.0`, le réseau du conteneur faisant alors office de frontière.

## Chaque flux réseau sortant

Il n'y en a pas d'autre : pas de télémétrie, pas de vérification de mise à jour, aucun hôte codé en dur. Les appels sortants sont faits dans `src/engine/proxy.rs` (proxy et test de disponibilité), `src/auth/keycloak.rs` (Keycloak) et `src/messaging/` (Kafka, derrière une fonctionnalité de compilation) ; `src/tcp/` ne fait qu'écouter.

| Flux | Déclencheur | Destination | Limites |
|---|---|---|---|
| Test de disponibilité | Un utilisateur clique sur « Tester la cible » | La `real_target_url` de ce service | Connexion TCP seulement, pas de requête HTTP, pas de poignée de main TLS ; délai de 3 s ; résultat gardé 2 minutes |
| Proxy | Une requête matche un service en mode proxy, ou une règle avec `action: proxy` | La `real_target_url` de ce service, jamais un autre hôte | 10 s pour se connecter, 120 s de silence au plus (`PROXY_READ_TIMEOUT_SECS`) ; les redirections ne sont pas suivies ; les chemins avec des segments `.`/`..` sont refusés avant transmission |
| Keycloak | `AUTH_ENABLED=true` seulement | `KEYCLOAK_URL` | Connexion, renouvellement de jeton, jeu de clés du realm (gardé 5 minutes) ; 5 s pour se connecter, 10 s en tout |
| Kafka | Compilé avec `--features messaging-kafka` **et** `KAFKA_ENABLED=true` | `KAFKA_BROKERS` | Consommateur et producteur sur les topics configurés |

## Qui reçoit quelle confiance

**L'exploitant** (variables d'environnement, conteneur, réseau) a une confiance totale.

**Les éditeurs de configuration** (utilisateurs de l'interface et de l'API) définissent les réponses, les cibles de proxy et les scripts. Sans authentification, toute personne qui peut atteindre l'API de gestion est un éditeur avec tous les droits : c'est le mode prévu pour le poste d'un développeur, et c'est pourquoi le binaire n'écoute par défaut que sur la boucle locale. Avec `AUTH_ENABLED=true`, les éditeurs sont des utilisateurs Keycloak et les droits suivent les groupes : un administrateur de groupe gère les services et les membres du groupe, un membre modifie ses services, et seuls les super-admins (`SUPER_ADMINS`) gèrent les services hors groupe, réinitialisent la configuration ou restaurent des sauvegardes. Ces frontières sont vérifiées sur chaque point d'accès, y compris les déplacements entre groupes, l'export complet de la configuration et le journal des requêtes (`src/server/api.rs`, tests dans `src/server/api/authz_tests.rs`).

Comme un éditeur choisit les cibles de proxy, Mimicway enverra des requêtes HTTP à tout hôte qu'un éditeur configure : c'est le principe d'un proxy. Décidez en conséquence qui peut éditer (authentification) et restreignez où le processus peut se connecter (politique réseau).

**Les scripts de règle** (Rhai) sont écrits par les éditeurs et s'exécutent à chaque requête qui matche, dans un bac à sable : au plus 10 000 opérations, profondeur d'appel, tailles des chaînes, des tableaux et des objets bornées, aucun accès aux fichiers (`import` ne résout rien), pas d'`eval`, pas de réseau, et la sortie de `print`/`debug` envoyée au journal de débogage. Les fonctions natives ne paniquent jamais, quels que soient leurs arguments (`src/engine/script.rs`, tests à côté).

**Le trafic des services** (chemins, en-têtes, corps envoyés par les applications testées) est une entrée non fiable : les corps sont limités à 10 Mio, le XML est analysé sans DTD ni expansion d'entités, les expressions régulières des règles sont compilées une fois avec une limite de taille, et les paramètres de chemin sont décodés en UTF-8.

**Les backends relayés** ne sont pas fiables non plus : leurs réponses sont renvoyées au fil de l'eau ; seule l'observation de trafic, quand un utilisateur l'active pour un service, met en mémoire des copies bornées.

**Les autres sites ouverts dans un navigateur** ne doivent pas pouvoir piloter un Mimicway joignable depuis ce navigateur (`src/server/browser_guard.rs`) :
- l'API de gestion ne répond aux appels d'une autre origine que pour les origines listées dans `CORS_ALLOWED_ORIGINS` (aucune par défaut) ;
- les requêtes d'API qui modifient l'état et que le navigateur marque comme intersites sont refusées, sauf si leur origine est listée ;
- en écoute sur la boucle locale seulement, l'API ne répond qu'aux requêtes adressées à `localhost`, `*.localhost`, `127.0.0.0/8` ou `::1`, ce qui déjoue le DNS rebinding ;
- l'interface et l'API sont servies avec une politique de sécurité du contenu (aucun script en ligne, aucun cadrage), `X-Content-Type-Options: nosniff` et `Referrer-Policy: no-referrer`. Les réponses des services simulés et relayés ne sont pas modifiées.

## Authentification

Quand elle est activée, Mimicway valide les jetons d'accès Keycloak localement, avec les clés publiées par le realm : signature asymétrique (RSA, RSA-PSS, ECDSA, EdDSA ; jamais HMAC ni `none`), émetteur (`KEYCLOAK_ISSUER` si les jetons portent une autre URL que `KEYCLOAK_URL`), expiration, et client pour lequel le jeton a été émis (`azp` ou `aud` doit nommer `KEYCLOAK_CLIENT_ID`). Keycloak n'est jamais sollicité pour valider un jeton à la place de Mimicway. Un identifiant de clé inconnu fait recharger le jeu de clés au plus une fois toutes les 30 secondes. Les réponses d'erreur ne contiennent aucun détail interne. Les tests tournent contre un faux realm qui signe de vrais jetons (`src/auth/keycloak/tests.rs`).

Le formulaire de connexion utilise l'octroi par mot de passe de Keycloak. Le remplacer par le flux du code d'autorisation avec PKCE, et prendre en charge n'importe quel fournisseur OpenID Connect, sont prévus (voir [ROADMAP.md](../../ROADMAP.md)).

## Données

- **Configuration** : un fichier YAML dans `DATA_PATH` (`mock-config.yaml`), écrit de façon atomique, avec des sauvegardes en rotation dans `backups/` et une sauvegarde protégée avant chaque réinitialisation (gardée 30 jours). Il contient ce que les éditeurs saisissent : évitez d'y mettre de vrais identifiants dans les réponses simulées ou les scripts.
- **Journal des requêtes** : les 200 dernières requêtes, en mémoire seulement, corps tronqués à 16 Kio. Les en-têtes d'identification (`Authorization`, `Proxy-Authorization`, `Cookie`, `Set-Cookie`, `X-Api-Key`, `X-Auth-Token`, `X-Amz-Security-Token`, plus `REDACT_HEADERS`) sont enregistrés comme `[redacted]`, et il en va de même pour l'observation de trafic ; les règles suggérées ne les recopient jamais. Avec l'authentification, les utilisateurs ne voient que les entrées des services auxquels ils ont accès.
- **Journaux** (sortie standard) : aucun corps de requête, aucun identifiant ; les URL de proxy perdent leur partie `utilisateur:mot-de-passe@`.

## Chaîne d'approvisionnement et compilation

- `Cargo.lock` et `frontend/package-lock.json` sont versionnés : les compilations résolvent exactement le même graphe de dépendances.
- La CI échoue sur toute vulnérabilité connue ou crate retirée (« yanked »), sur une licence hors d'une liste permissive et sur une dépendance venue d'ailleurs que crates.io ([deny.toml](../../deny.toml)), sur les alertes npm de gravité haute, sur les vulnérabilités critiques ou hautes de l'image de conteneur (Trivy) et sur les secrets versionnés (gitleaks).
- Les analyseurs et matchers du serveur qui lisent des entrées non fiables (conditions des requêtes, sous-chemins, chemins XML, templates, hexadécimal des mocks TCP bruts, imports de configuration) sont soumis au fuzzing par ClusterFuzzLite, cinq minutes à chaque pull request qui les modifie et une heure chaque semaine ; les analyseurs propres à l'interface ont des tests de propriétés.
- CodeQL analyse le serveur (Rust), l'interface (JavaScript) et les workflows de la CI dès qu'ils changent, les workflows à chaque pull request, et les trois chaque semaine ; ses alertes sont listées dans l'onglet Security du dépôt.
- Les actions tierces de la CI sont épinglées par empreinte de commit, et Dependabot propose leurs mises à jour avec celles des crates, des paquets npm et des images de base.
- Le code de production ne contient aucun Rust `unsafe` (`#![forbid(unsafe_code)]` hors tests).
- L'image contient le binaire statique et son répertoire de données, rien d'autre (`FROM scratch`) : ni système d'exploitation, ni shell, ni paquet système, donc aucune place pour une vulnérabilité hors du code de Mimicway lui-même. Elle s'exécute sous un utilisateur non root (uid 1000) ; les manifestes Kubernetes ajoutent un système de fichiers racine en lecture seule, aucune élévation de privilèges et aucune capacité Linux.
- Les versions publiées sont construites à partir du commit étiqueté par `.github/workflows/release.yml` : chaque archive a une attestation de provenance de compilation et un SBOM CycloneDX (un pour les crates Rust, un pour les paquets livrés de l'interface), et l'image est signée sans clé avec cosign. [SECURITY.md](../../SECURITY.md#verifying-a-release) donne les commandes de vérification.
- Les images de base des deux Dockerfiles sont épinglées par empreinte.

## Liste de durcissement

1. Instance partagée : fixez `AUTH_ENABLED=true` avec Keycloak et listez les `SUPER_ADMINS`.
2. Gardez l'écoute par défaut sur la boucle locale sur les postes de travail ; dans un cluster, exposez Mimicway via votre ingress avec TLS (Mimicway lui-même sert du HTTP simple).
3. Restreignez les sorties réseau (`NetworkPolicy` Kubernetes, pare-feu) aux backends que vous relayez réellement et à Keycloak.
4. Ne fixez `CORS_ALLOWED_ORIGINS` que si l'interface est servie depuis une autre origine que l'API.
5. Ajoutez vos propres noms d'en-têtes d'identification à `REDACT_HEADERS`.
6. Utilisez l'image et les manifestes fournis, ou reproduisez leurs restrictions (non root, système de fichiers racine en lecture seule, aucune capacité).

## Limites connues

- Pas de TLS intégré, pas de limitation de débit ni de journal d'audit des changements de configuration (les sauvegardes gardent les versions précédentes ; un journal d'audit est prévu).
- Sans authentification, chaque utilisateur est super-admin. C'est prévu pour un usage local seulement.
- L'authentification ne prend en charge que Keycloak pour l'instant.

Pour signaler une vulnérabilité, voir [SECURITY.md](../../SECURITY.md).
