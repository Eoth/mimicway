[English](../en/backups-and-restore.md)

# Sauvegardes et restauration

Mimicway protège votre configuration contre les erreurs de manipulation : avant chaque changement, l'état précédent est sauvegardé, si bien que vous pouvez toujours revenir en arrière.

## Sauvegardes automatiques

Avant **chaque changement** de la configuration (création, modification ou suppression d'un service ou d'une règle, restauration d'une sauvegarde…), l'état précédent est copié dans un historique de sauvegardes, **sans aucune action de votre part**.

Seules les sauvegardes les plus récentes sont conservées (`BACKUP_MAX_COUNT`, 5 par défaut) : les plus anciennes sont retirées à mesure que de nouvelles arrivent.

## Restaurer une sauvegarde

**« Sauvegardes »** dans la barre de navigation liste toutes les sauvegardes disponibles avec leur date et leur taille, et en restaure une **en un clic**.

![La liste des sauvegardes avec un bouton de restauration](screenshots/backups-list.png)

Une restauration sauvegarde d'abord l'état qu'elle écrase : même une restauration peut être annulée.

![Confirmation avant la restauration d'une sauvegarde](screenshots/backups-restore-confirmation.png)

## Une sauvegarde spéciale avant une réinitialisation complète

Avant une [réinitialisation complète](administration.md) (qui supprime tous les services), une sauvegarde supplémentaire est écrite dans un emplacement protégé, hors de portée de la rotation normale pendant 30 jours : le temps de s'apercevoir de l'erreur et de la restaurer si la réinitialisation n'était pas voulue.

## Prérequis et limites

- Aucun prérequis : disponible dans toutes les installations, toujours actif.
- Lister et restaurer les sauvegardes est réservé aux **super-admins** quand l'[authentification](authentication.md) est activée. Sans authentification, tout le monde peut s'en servir (comme du reste de l'interface dans ce mode).
- Sauvegardes et restaurations portent sur **toute la configuration** (tous les services et groupes) : un service seul ne peut pas être restauré isolément depuis une ancienne sauvegarde.
- `backups/` et `backups/protected/` sont de vrais répertoires sous le répertoire de données : Mimicway les résout avant chaque copie ou suppression et refuse un lien symbolique qui mène hors du répertoire de données, pour ne rien écrire ni supprimer ailleurs. Pour garder les sauvegardes sur un autre disque, montez-le à cet endroit.
