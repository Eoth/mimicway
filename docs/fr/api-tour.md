[English](../en/api-tour.md)

# L'API en cinq étapes

Tout ce que fait l'interface passe par l'[API de gestion](../../README.fr.md#api-de-gestion) : un mock peut donc aussi être écrit par un script ou un job de CI. Cette page le montre avec `curl`, en une passe : un service simulé, une règle qui lit la requête, une règle relayée vers un backend, le journal des requêtes. Le backend est un second service de la même instance : rien ne sort de la machine. Collez les blocs dans l'ordre dans un même terminal bash, depuis la racine d'une copie du dépôt où `cargo build --release` a tourné. Sous chaque commande, la ligne qui commence par `#` est sa réponse, `…` tenant lieu de fausses données ou de valeurs longues ; la CI exécute ces commandes telles quelles et échoue quand une réponse change.

<!-- tour:start -->

## 1. Démarrez une instance vide

Elle écoute sur le port 7343, sur cette machine seulement, et garde ses données dans un dossier temporaire : une instance déjà lancée sur 7342 et son `./data` restent intactes ; depuis une archive de version, lancez `./mimicway`. `c` est un `curl` qui affiche le code de statut après le corps.

```bash
PORT=7343 RUST_LOG=warn DATA_PATH="$(mktemp -d)" ./target/release/mimicway &
until curl -sf localhost:7343/api/health >/dev/null; do sleep 0.2; done
c() { curl -sS -w ' → %{http_code}\n' -H 'Content-Type: application/json' "$@"; }
```

## 2. Simulez un service

`users` répond à `GET /users/{id}` avec l'identifiant appelé et un faux prénom. `svc` est le service avec un emplacement `RULES` : chaque appel ci-dessous envoie le service entier, ses règles à la place de `RULES`.

```bash
user='{"name":"user","method":"GET","action":"mock","conditions":{},
  "response":{"body":[{"type":"Template","template":"{\"id\":\"{{path.id}}\",\"name\":\"{{fake.FirstName}}\"}"}]}}'
svc='{"name":"users","listen_path":"/{id}","real_target_url":"http://localhost:7343/backend","is_mocked":true,"wsdl_mode":"auto","rules":[RULES]}'
c localhost:7343/api/services -d "${svc/RULES/$user}"
# {"name":"users",…} → 201
c localhost:7343/users/42
# {"id":"42","name":"…"} → 200
```

## 3. Ajoutez une règle à condition

Les règles sont essayées dans l'ordre : `vip` répond aux requêtes dont la query contient `plan=vip`, `user` à toutes les autres. `PUT` remplace le service entier.

```bash
vip='{"name":"vip","method":"GET","action":"mock","conditions":{"all_of":[{"source":{"type":"QueryParam","key":"plan"},
  "operator":{"type":"Eq","value":"vip"}}]},"response":{"body":[{"type":"Literal","value":"VIP offer"}]}}'
c -X PUT localhost:7343/api/services/users -d "${svc/RULES/$vip,$user}"
# {"name":"users",…} → 200
c 'localhost:7343/users/42?plan=vip'
# VIP offer → 200
```

## 4. Relayez une règle

`backend` tient le rôle de la vraie API vers laquelle pointe `users` (son `real_target_url`). Avec `"action":"proxy"`, la règle `vip` y envoie les requêtes qu'elle reconnaît, et `user` continue de simuler les autres.

```bash
c localhost:7343/api/services -d '{"name":"backend","listen_path":"","real_target_url":"","is_mocked":true,"wsdl_mode":"auto",
  "rules":[{"name":"all","method":"GET","action":"mock","conditions":{},"response":{"body":[{"type":"Literal","value":"from the backend"}]}}]}'
# {"name":"backend",…} → 201
c -X PUT localhost:7343/api/services/users -d "${svc/RULES/${vip/mock/proxy},$user}"
# {"name":"users",…"action":"proxy",…} → 200
c 'localhost:7343/users/42?plan=vip'
# from the backend → 200
```

## 5. Lisez le journal des requêtes, puis faites le ménage

La dernière requête est passée par `users` en mode proxy, vers `/backend/42`. Une fois l'instance arrêtée, il ne reste que son dossier temporaire.

```bash
c 'localhost:7343/api/logs?limit=1'
# [{…"path":"/users/42","mode":"proxy",…"target_url":"http://localhost:7343/backend/42",…}] → 200
c -X DELETE localhost:7343/api/services/users
# → 204
kill %1
```

<!-- tour:end -->
