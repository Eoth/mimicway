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
- La CI échoue sur toute vulnérabilité connue d'une dépendance verrouillée (RustSec via cargo deny, et OSV, qui contient aussi les alertes de GitHub, pour chaque fichier de verrouillage), sur une crate retirée (« yanked »), sur une licence hors d'une liste permissive et sur une dépendance venue d'ailleurs que crates.io ([deny.toml](../../deny.toml)), sur les alertes npm (gravité haute et plus pour les paquets que l'interface livre, critique pour les outils de construction et de test), sur les vulnérabilités critiques ou hautes de l'image de conteneur (Trivy) et sur les secrets versionnés (gitleaks).
- Les analyseurs et matchers du serveur qui lisent des entrées non fiables (conditions des requêtes, sous-chemins, chemins XML, templates, hexadécimal des mocks TCP bruts, imports de configuration) sont soumis au fuzzing par ClusterFuzzLite, cinq minutes à chaque pull request qui les modifie et une heure chaque semaine ; les analyseurs propres à l'interface ont des tests de propriétés.
- CodeQL analyse le serveur (Rust), l'interface (JavaScript) et les workflows de la CI dès qu'ils changent, les workflows à chaque pull request, et les trois chaque semaine ; ses alertes sont listées dans l'onglet Security du dépôt.
- Les actions tierces de la CI sont épinglées par empreinte de commit, et Dependabot propose leurs mises à jour avec celles des crates, des paquets npm et des images de base.
- Le code ne contient aucun Rust `unsafe`, tests compris (`#![forbid(unsafe_code)]`).
- L'image contient le binaire statique et son répertoire de données, rien d'autre (`FROM scratch`) : ni système d'exploitation, ni shell, ni paquet système, donc aucune place pour une vulnérabilité hors du code de Mimicway lui-même. Elle s'exécute sous un utilisateur non root (uid 1000) ; les manifestes Kubernetes ajoutent un système de fichiers racine en lecture seule, aucune élévation de privilèges et aucune capacité Linux.
- Les versions publiées sont construites à partir du commit étiqueté par `.github/workflows/release.yml` : chaque archive a une attestation de provenance de compilation et un SBOM CycloneDX (un pour les crates Rust, un pour les paquets livrés de l'interface), et l'image est signée sans clé avec cosign. [SECURITY.md](../../SECURITY.md#verifying-a-release) donne les commandes de vérification.
- Les binaires Linux et leurs archives sont reproductibles : le workflow de publication construit chacun deux fois, sur deux machines, et ne publie rien si les octets diffèrent, et chacun peut les reconstruire à partir de l'étiquette pour comparer ([SECURITY.md](../../SECURITY.md#rebuilding-a-release)).
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

## Pourquoi ces garanties tiennent

Les sections précédentes donnent les faits. Celle-ci les relie en un raisonnement, l'argumentaire de sécurité du projet : ce que Mimicway promet, face à qui, où la confiance change de mains, et pourquoi sa conception et son code tiennent chaque promesse.

### Exigences de sécurité

1. Mimicway n'ouvre aucune connexion que son opérateur ou un éditeur n'a pas configurée.
2. Sans authentification, seule la machine locale atteint l'API de gestion.
3. Avec authentification, un utilisateur n'atteint que les services et groupes que ses rôles autorisent, sur chaque point d'accès.
4. Une page web ouverte dans un navigateur ne peut pas piloter un Mimicway que ce navigateur peut joindre.
5. Le trafic des applications testées et les réponses des backends relayés ne peuvent ni planter le serveur, ni épuiser sa mémoire, ni exécuter du code, ni atteindre ses fichiers.
6. Les identifiants vus dans le trafic ne sont gardés ni dans les journaux, ni dans les observations, ni dans les suggestions.
7. Un changement de configuration accepté n'est jamais perdu, et la configuration précédente peut être restaurée.

### Modèle de menaces

| Qui | Ce qu'il pourrait tenter | Ce qui l'arrête |
|---|---|---|
| Un site web visité par un développeur pendant que Mimicway tourne sur sa machine | Des requêtes inter-sites vers l'API de gestion, du DNS rebinding | Le garde navigateur : liste CORS, refus des écritures inter-sites, contrôle de l'hôte en écoute locale |
| Quiconque sur un réseau où le port est exposé | Lire ou modifier les mocks | L'écoute locale par défaut ; l'authentification par Keycloak ; TLS à l'ingress |
| Un utilisateur authentifié | Atteindre les services d'un autre groupe, ou l'administration | L'autorisation sur chaque point d'accès, testée à travers le vrai routeur avec des jetons signés |
| Un éditeur, à qui l'on confie la configuration | Sortir d'un script de règle | Le bac à sable Rhai : ni fichier, ni réseau, ni `import`, ni `eval`, des opérations, tailles et profondeurs bornées |
| Les applications testées, un backend relayé | Des entrées énormes ou malformées, une traversée de chemin, l'épuisement des ressources | La limite de corps, la lecture typée, le XML sans DTD, les expressions régulières de taille bornée, les segments `.` et `..` refusés, les délais, les analyseurs soumis au fuzzing |
| Une dépendance ou une action de CI compromise | Du code malveillant ou une vulnérabilité connue dans la construction | Les fichiers de verrouillage, les actions et images épinglées par empreinte, cargo deny, OSV, npm audit, Trivy, Dependabot, CodeQL |

L'opérateur (environnement, conteneur, réseau) est de confiance. On confie aux éditeurs le choix des cibles du proxy : relayer vers l'hôte qu'un éditeur a configuré, c'est le rôle d'un proxy ; restreindre les sorties revient donc au déploiement (point 3 de la liste de durcissement).

### Frontières de confiance

- **Le port HTTP.** Tout ce qui le traverse est non fiable : les requêtes de gestion sont authentifiées et autorisées quand l'authentification est active, et le trafic des services est une donnée pour les règles, jamais une instruction pour Mimicway.
- **L'origine du navigateur.** L'API de gestion ne répond à une autre origine que si `CORS_ALLOWED_ORIGINS` la cite.
- **Les rôles.** Appelant anonyme, utilisateur authentifié, membre de groupe, administrateur de groupe, super-admin ; la [matrice d'autorisation](../../REVIEWING.md#authorization-matrix) (en anglais) dit lequel chaque point d'accès exige.
- **Le bac à sable des scripts.** Le code Rhai écrit par les éditeurs s'exécute dans le processus, sans accès à quoi que ce soit hors de son bac à sable.
- **Les réponses sortantes.** Les backends relayés et Keycloak sont non fiables : leurs réponses sont renvoyées en flux ou lues comme des données, dans des délais bornés.
- **Le système de fichiers.** Mimicway n'écrit que sous `DATA_PATH`, dans des fichiers dont il produit le nom ou dont il vérifie le nom contre leur motif.

### Principes de conception sûre

| Principe | Application |
|---|---|
| Économie de mécanisme | Un seul processus, aucune base de données, un seul module pour tout appel HTTP sortant, un seul stockage pour la configuration. |
| Valeurs par défaut sûres | Écoute locale seulement, aucune origine CORS, redirections non suivies. Avec `AUTH_ENABLED=true` mais sans ses réglages Keycloak, Mimicway refuse de démarrer ; quand Keycloak est injoignable, les routes protégées refusent la requête. |
| Médiation complète | Chaque point d'accès de gestion, sauf les quatre publics, vérifie le droit de l'appelant à chaque requête, déplacements entre groupes et export complet de la configuration compris. |
| Conception ouverte | Le code et ce modèle sont publics ; la protection repose sur les clés de Keycloak et sur la politique réseau, pas sur le secret. |
| Séparation des privilèges | Réinitialiser, restaurer une sauvegarde et gérer les services hors groupe exigent le rôle super-admin ; un administrateur de groupe ne gère que son groupe. |
| Moindre privilège | L'image tourne sous un utilisateur non root, sur un système de fichiers racine en lecture seule, sans capacité Linux ; les jobs de CI reçoivent des jetons en lecture seule, et l'écriture seulement là où un job en a besoin. |
| Mécanismes communs minimaux | L'état est gardé par service (compteurs de séquence, observations), et les vues partagées (configuration, journal des requêtes) sont filtrées par utilisateur. |
| Acceptabilité psychologique | La configuration sûre ne demande aucun réglage sur un poste de travail ; les refus disent pourquoi, dans la langue de l'utilisateur. |
| Surface d'attaque limitée | Un seul port, aucune télémétrie, les fonctionnalités optionnelles absentes du binaire sauf demande, les fichiers de l'interface servis par leur nom exact. |
| Validation des entrées par liste blanche | Les noms suivent `[A-Za-z0-9_-]+` et évitent les routes réservées, les méthodes HTTP viennent d'une liste fixe, les noms de sauvegarde doivent suivre le motif que produit le serveur, et les corps de requête et de configuration sont lus dans des structures typées. |

### Faiblesses courantes contrées

| Faiblesse (OWASP Top 10 2021, CWE) | Contrée par |
|---|---|
| Contrôle d'accès défaillant (A01, CWE-862, CWE-863) | La matrice d'autorisation, appliquée dans chaque gestionnaire et testée avec de vrais jetons (`src/server/api/authz_tests.rs`). |
| Défaillances cryptographiques (A02) | Aucune cryptographie maison ; des signatures de jeton asymétriques seulement, jamais HMAC ni `none` ; TLS sortant par rustls, certificats vérifiés. |
| Injection et cross-site scripting (A03, CWE-79, CWE-94) | Ni SQL ni shell ; les templates insèrent les valeurs de la requête sans les évaluer ; les scripts tournent dans le bac à sable, sans `eval` ; l'interface échappe chaque valeur (aucun HTML brut) sous une politique de sécurité du contenu qui interdit les scripts en ligne. |
| Conception non sécurisée (A04) | Ce modèle, le [guide de revue](../../REVIEWING.md) (en anglais) et les analyseurs soumis au fuzzing. |
| Mauvaise configuration de sécurité (A05) | Des valeurs par défaut sûres, la liste de durcissement et des manifestes qui l'appliquent. |
| Composants vulnérables et obsolètes (A06, CWE-1104) | Les fichiers de verrouillage, les alertes vérifiées à chaque changement d'un manifeste et chaque semaine, les mises à jour de Dependabot. |
| Défaillances d'identification et d'authentification (A07, CWE-287) | Déléguées à Keycloak ; jetons vérifiés localement : signature, émetteur, expiration et client. |
| Défaillances d'intégrité des logiciels et des données (A08, CWE-502) | Désérialisation typée seulement ; actions et images de base épinglées ; versions signées, avec leur provenance ; configuration écrite de façon atomique, avec sauvegardes. |
| Défaillances de journalisation et de surveillance (A09) | Journal des requêtes et journaux du serveur sans identifiants ; un journal d'audit des changements de configuration est prévu. |
| Falsification de requête côté serveur (A10, CWE-918) | Les requêtes ne partent que vers la cible qu'un éditeur a configurée, jamais vers un hôte tiré de la requête ; les redirections ne sont pas suivies et les segments `.` et `..` sont refusés. |
| Traversée de chemin (CWE-22) | Les noms de sauvegarde vérifiés avant tout accès au disque ; les fichiers de l'interface intégrée servis depuis une table fixe, un `STATIC_DIR` par le `ServeDir` de tower-http, qui refuse les segments parents. |
| Consommation de ressources non maîtrisée (CWE-400) | Des limites sur les corps, les journaux, les observations et les files ; des expressions régulières de taille bornée ; des opérations de script bornées ; des délais sur le proxy et Keycloak. |
| Falsification de requête inter-sites (CWE-352) | Les écritures inter-sites refusées par le garde navigateur. |
| Sûreté mémoire (CWE-787, CWE-125) | Le code de Mimicway est du Rust sans `unsafe`, et ses analyseurs d'entrées non fiables sont soumis au fuzzing avec AddressSanitizer. |

Pour signaler une vulnérabilité, voir [SECURITY.md](../../SECURITY.md).
