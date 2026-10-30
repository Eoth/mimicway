[Français](../fr/backups-and-restore.md)

# Backups and restore

Mimicway protects your configuration against mistakes: before changes, the previous state is saved, so you can always go back.

## Automatic backups

Before a change to the configuration (creating, editing or deleting a service or a rule, restoring a backup…), the previous state is copied to a backup history, **without any action on your part**.

Only the most recent backups are kept (`BACKUP_MAX_COUNT`, 5 by default): older ones are removed as new ones arrive. So that they cover more than the last few clicks:

- **Changes made in a burst share one backup**, taken before the first of them. A change is backed up only when the last backup is a minute old (`BACKUP_MIN_INTERVAL_SECS`, 60 by default; `0` backs up every change). Merging an import, which sends one request per group and per service, makes one backup at most. Going back to a backup therefore undoes every change made in the minute that followed it.
- **Restoring a backup and importing a file that replaces everything always back up first**, whenever they happen; a full reset makes the special backup below instead.
- **Saving without changing anything writes nothing**, and a backup identical to the newest one is not made twice.

## Restoring a backup

**"Backups"** in the navigation bar lists every available backup with its date and size, and restores one **in a click**.

![The list of backups with a restore button](screenshots/backups-list.png)

A restore itself first backs up the state it overwrites: even a restore can be undone.

![Confirmation before restoring a backup](screenshots/backups-restore-confirmation.png)

## A special backup before a full reset

Before a [full reset](administration.md) (which deletes every service), its backup is written to a protected place, out of reach of the normal rotation for 30 days: enough time to notice and restore it if the reset was a mistake. It holds the last change made before the reset, and no ordinary backup repeats it. The newest 10 are kept (`PROTECTED_BACKUP_MAX_COUNT`): a test suite that resets before each test does not fill the disk.

## Requirements and limits

- No requirement: available in every installation, always on.
- Listing and restoring backups are reserved to **super-admins** when [authentication](authentication.md) is on. Without authentication, everyone can use them (as the rest of the interface in that mode).
- Backups and restores cover **the whole configuration** (every service and group): a single service cannot be restored on its own from an old backup.
- `backups/` and `backups/protected/` are real directories under the data directory: Mimicway resolves them before each copy or deletion and refuses a symbolic link that leads outside the data directory, so that nothing is written or deleted elsewhere. To keep the backups on another disk, mount it at that place instead.
