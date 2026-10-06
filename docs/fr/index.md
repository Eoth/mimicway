[English](../en/index.md)

# Guide d'utilisation de Mimicway

Mimicway simule (« mock ») ou relaie (« proxy ») des appels HTTP vers un vrai service : tester une application sans dépendre d'un vrai backend, ou rejouer à la demande des scénarios précis (erreurs, lenteurs, données particulières). Tout se pilote depuis une interface web, sans redémarrage.

Cette page est une **vue d'ensemble rapide** : une ligne ou deux par fonctionnalité, avec un lien vers la page qui explique comment s'en servir. Si vous découvrez Mimicway, parcourez cette liste une fois pour savoir ce qui existe, puis revenez aux pages détaillées au besoin.

> Ces pages s'adressent aux personnes qui utilisent Mimicway et testent avec lui (QA, développeurs, analystes métier). L'installation et la configuration sont décrites dans le [README](../../README.fr.md), l'architecture dans [ARCHITECTURE.md](../../ARCHITECTURE.md) (en anglais) ; le modèle de sécurité dans [security.md](security.md).

![L'écran d'accueil de Mimicway avec la liste des services](screenshots/home-service-list.png)

## Services et routage

| Fonctionnalité | En bref |
|---|---|
| [Services et routage](services.md) | Chaque service simulé a sa propre URL (`/mon-service/...`), avec l'adresse du vrai backend, l'état du mock (actif ou non) et le type d'API (REST ou SOAP). |
| [Groupes de services](groups.md) | Rassembler des services liés (tous ceux d'une équipe, par exemple) sous un même préfixe d'URL, avec des droits par groupe. |
| [Test de disponibilité](availability-check.md) | Un bouton qui vérifie si le vrai backend est joignable sur le réseau, sans jamais lui envoyer de vraie requête. |

## Règles et réponses simulées

| Fonctionnalité | En bref |
|---|---|
| [Règles de correspondance](matching-rules.md) | Un service peut avoir plusieurs règles : chacune fixe une méthode HTTP, un sous-chemin et des conditions (sur les paramètres, les en-têtes, le corps…) qui décident de la réponse à renvoyer. |
| [Réponses et templates](responses-and-templates.md) | Construire la réponse (JSON ou XML) dans un éditeur visuel, avec des données factices (noms, adresses…) et des pannes et lenteurs simulées (mode Chaos). |
| [Scripts Rhai](rhai-scripts.md) | Pour les cas avancés : un court script calcule des valeurs (dates, nombres aléatoires ou stables, UUID…) que la réponse reprend. |
| [Testeur de règle et détection de conflits](rule-tester-and-conflicts.md) | Vérifier qu'une règle fonctionne contre une vraie requête déjà reçue, et être averti quand une nouvelle règle risque d'entrer en conflit avec une règle existante. |

## Suivi et diagnostic

| Fonctionnalité | En bref |
|---|---|
| [Journal des requêtes](request-log.md) | Les dernières requêtes reçues par Mimicway, dans l'interface : comprendre pourquoi une règle a matché, ou pas. |
| [Observation de trafic et suggestions de règles](traffic-observation.md) | Pour un service en mode proxy pur : observer le trafic réel (à la demande) et se voir proposer des règles de mock à partir des appels réellement vus. |

## Sauvegardes et administration

| Fonctionnalité | En bref |
|---|---|
| [Sauvegardes et restauration](backups-and-restore.md) | La configuration est sauvegardée avant chaque changement ; un état antérieur se restaure en un clic. |
| [Administration (import, export, réinitialisation, mode sombre, langue)](administration.md) | Exporter ou importer toute la configuration sous forme de fichier, tout réinitialiser, changer de thème et de langue. |

## Sécurité et intégrations

| Fonctionnalité | En bref |
|---|---|
| [Authentification](authentication.md) | Optionnelle : Mimicway peut exiger une connexion (Keycloak), avec des droits différents selon l'utilisateur et le groupe. |
| [Messagerie Kafka](kafka-messaging.md) | Optionnelle : au-delà du HTTP, Mimicway peut aussi répondre à des messages Kafka (nécessite une version compilée avec ce support). |
| [Modèle de sécurité](security.md) | Ce que Mimicway expose, à quoi il se connecte, à qui il fait confiance, et comment durcir un déploiement. |
