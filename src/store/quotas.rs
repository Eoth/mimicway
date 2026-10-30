// Caps on what users create and the server keeps, so that one signed-in user cannot grow the configuration, the disk
// and the time of every write for everyone. Every change rewrites the whole configuration file, holds a copy of it in
// memory, and leaves copies behind (backups/, backups/protected/).
//
// A change is refused when it takes one of the counts below past its cap AND makes that count grow: a configuration
// already over a cap (the cap was lowered, or it was restored) can still be edited and shrunk, never grown. Restoring
// a backup is not checked: it brings back a state the server already held. The message names the cap and its
// setting.
//
// Defaults, from the reference deployment (k8s/base: a 64 MiB volume, 256 MiB of memory):
// - MAX_CONFIG_BYTES, 2 MiB of YAML: the volume holds up to 17 copies of the file (the file, its temporary copy
//   during a write, 5 backups, 10 protected ones), 34 MiB, with room left; and an export comes back in one import,
//   whose request body the management API bounds at 2 MiB as well (server::limits).
// - MAX_SERVICES_PER_GROUP, 100, and MAX_RULES_PER_GROUP, 1,000: a group, or the services outside any group, is one
//   team's mocks. A rule of the guide's examples takes about 400 bytes of YAML, so 1,000 of them hold 400 KB, a fifth
//   of MAX_CONFIG_BYTES: no group alone fills the configuration, and the size cap still binds the sum.
// - MAX_GROUPS_PER_USER, 20: the groups a user administers, a creator being the first admin of a group; enough for
//   one person in several teams, few enough that one account cannot spread the configuration over hundreds of groups.
// - PROTECTED_BACKUP_MAX_COUNT, 10: the copies kept by full resets, the oldest removed first, on top of their 30-day
//   expiry; ten resets back is more than anyone undoes, and a test suite that resets before each test no longer
//   leaves thousands of files.
use crate::i18n::tr;
use crate::models::MockConfig;
use std::collections::BTreeMap;

const DEFAULT_CONFIG_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_SERVICES_PER_GROUP: usize = 100;
const DEFAULT_RULES_PER_GROUP: usize = 1_000;
const DEFAULT_GROUPS_PER_USER: usize = 20;
const DEFAULT_PROTECTED_BACKUPS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quotas {
    pub config_bytes: usize,
    pub services_per_group: usize,
    pub rules_per_group: usize,
    pub groups_per_user: usize,
    pub protected_backups: usize,
}

impl Quotas {
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let number = |name, default| crate::settings::number(&lookup, name, default);
        Self {
            config_bytes: number("MAX_CONFIG_BYTES", DEFAULT_CONFIG_BYTES),
            services_per_group: number("MAX_SERVICES_PER_GROUP", DEFAULT_SERVICES_PER_GROUP),
            rules_per_group: number("MAX_RULES_PER_GROUP", DEFAULT_RULES_PER_GROUP),
            groups_per_user: number("MAX_GROUPS_PER_USER", DEFAULT_GROUPS_PER_USER),
            protected_backups: number("PROTECTED_BACKUP_MAX_COUNT", DEFAULT_PROTECTED_BACKUPS),
        }
    }

    /// Why `after` may not replace `before`: the first count it takes past its cap while making it grow.
    pub fn refusal(&self, before: &MockConfig, after: &MockConfig) -> Option<String> {
        let (services, rules) = per_group(after);
        let admins = groups_per_admin(after);
        if !over(&services, self.services_per_group)
            && !over(&rules, self.rules_per_group)
            && !over(&admins, self.groups_per_user)
        {
            return None;
        }
        let (services_before, rules_before) = per_group(before);
        let admins_before = groups_per_admin(before);
        if let Some((group, n)) = grown(&services, &services_before, self.services_per_group) {
            return Some(over_cap(
                group,
                n,
                self.services_per_group,
                Counted::Services,
            ));
        }
        if let Some((group, n)) = grown(&rules, &rules_before, self.rules_per_group) {
            return Some(over_cap(group, n, self.rules_per_group, Counted::Rules));
        }
        grown(&admins, &admins_before, self.groups_per_user).map(|(user, n)| {
            tr(
                "This change would make \"{0}\" an admin of {1} groups, over the limit of {2} (MAX_GROUPS_PER_USER).",
                &[&user, &n, &self.groups_per_user],
            )
        })
    }

    /// Why a configuration of `bytes` may not replace one of `bytes_before`.
    pub fn size_refusal(&self, bytes_before: usize, bytes: usize) -> Option<String> {
        (bytes > self.config_bytes && bytes > bytes_before).then(|| {
            tr(
                "This change would make the configuration {0} bytes long, over the limit of {1} (MAX_CONFIG_BYTES).",
                &[&bytes, &self.config_bytes],
            )
        })
    }
}

fn over<K>(counts: &BTreeMap<K, usize>, cap: usize) -> bool {
    counts.values().any(|&n| n > cap)
}

/// The first key whose count in `after` is past `cap` and above its count in `before`.
fn grown<K: Ord + Copy>(
    after: &BTreeMap<K, usize>,
    before: &BTreeMap<K, usize>,
    cap: usize,
) -> Option<(K, usize)> {
    after
        .iter()
        .find(|(key, n)| **n > cap && **n > before.get(*key).copied().unwrap_or(0))
        .map(|(key, n)| (*key, *n))
}

enum Counted {
    Services,
    Rules,
}

fn over_cap(group: Option<&str>, count: usize, cap: usize, counted: Counted) -> String {
    match (group, counted) {
        (Some(group), Counted::Services) => tr(
            "This change would give the group \"{0}\" {1} services, over the limit of {2} (MAX_SERVICES_PER_GROUP).",
            &[&group, &count, &cap],
        ),
        (Some(group), Counted::Rules) => tr(
            "This change would give the group \"{0}\" {1} rules, over the limit of {2} (MAX_RULES_PER_GROUP).",
            &[&group, &count, &cap],
        ),
        (None, Counted::Services) => tr(
            "This change would make {0} services outside any group, over the limit of {1} (MAX_SERVICES_PER_GROUP).",
            &[&count, &cap],
        ),
        (None, Counted::Rules) => tr(
            "This change would make {0} rules in the services outside any group, over the limit of {1} (MAX_RULES_PER_GROUP).",
            &[&count, &cap],
        ),
    }
}

/// A count per group, the services outside any group counted under `None`.
type PerGroup<'a> = BTreeMap<Option<&'a str>, usize>;

/// Services and rules of each group.
fn per_group(config: &MockConfig) -> (PerGroup<'_>, PerGroup<'_>) {
    let mut services = BTreeMap::new();
    let mut rules = BTreeMap::new();
    for service in &config.services {
        let group = service.group_name.as_deref();
        *services.entry(group).or_insert(0) += 1;
        *rules.entry(group).or_insert(0) += service.rules.len();
    }
    (services, rules)
}

fn groups_per_admin(config: &MockConfig) -> BTreeMap<&str, usize> {
    let mut counts = BTreeMap::new();
    for group in &config.groups {
        for admin in &group.admins {
            *counts.entry(admin.as_str()).or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests;
