use super::*;
use crate::models::{Group, Rule, Service};
use crate::store::{MockStore, StoreError};
use std::path::{Path, PathBuf};

fn rule(name: &str) -> Rule {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "method": "GET",
        "sub_path": null,
        "action": "mock",
        "script": null,
        "conditions": { "all_of": [], "any_of": [] },
        "response": { "status": 200, "headers": [], "body": [] }
    }))
    .unwrap()
}

fn service(name: &str, group: Option<&str>, rules: usize) -> Service {
    serde_json::from_value(serde_json::json!({
        "name": name,
        "listen_path": format!("/{name}/*"),
        "real_target_url": "",
        "is_mocked": true,
        "group_name": group,
        "wsdl_mode": "auto",
        "rules": (0..rules).map(|i| rule(&format!("r{i}"))).collect::<Vec<_>>()
    }))
    .unwrap()
}

fn group(name: &str, admins: &[&str]) -> Group {
    Group {
        name: name.into(),
        code: name.into(),
        admins: admins.iter().map(|a| a.to_string()).collect(),
        members: vec![],
    }
}

fn config(services: Vec<Service>, groups: Vec<Group>) -> MockConfig {
    MockConfig { services, groups }
}

fn caps(services: usize, rules: usize, groups: usize) -> Quotas {
    Quotas {
        services_per_group: services,
        rules_per_group: rules,
        groups_per_user: groups,
        ..Quotas::from_lookup(|_| None)
    }
}

fn services_in(group: Option<&str>, count: usize) -> Vec<Service> {
    (0..count)
        .map(|i| service(&format!("s{i}"), group, 0))
        .collect()
}

#[test]
fn the_defaults_are_the_documented_ones_and_each_setting_changes_its_cap() {
    let defaults = Quotas::from_lookup(|_| None);
    assert_eq!(
        defaults,
        Quotas {
            config_bytes: 2 * 1024 * 1024,
            services_per_group: 100,
            rules_per_group: 1_000,
            groups_per_user: 20,
            protected_backups: 10,
        }
    );
    let set = Quotas::from_lookup(crate::settings::vars(&[
        ("MAX_CONFIG_BYTES", "1"),
        ("MAX_SERVICES_PER_GROUP", "2"),
        ("MAX_RULES_PER_GROUP", "3"),
        ("MAX_GROUPS_PER_USER", "4"),
        ("PROTECTED_BACKUP_MAX_COUNT", "5"),
    ]));
    assert_eq!(
        set,
        Quotas {
            config_bytes: 1,
            services_per_group: 2,
            rules_per_group: 3,
            groups_per_user: 4,
            protected_backups: 5,
        }
    );
}

#[test]
fn services_of_a_group_are_refused_one_past_the_cap() {
    let quotas = caps(2, 100, 100);
    let groups = vec![group("team", &[])];
    let one = config(services_in(Some("team"), 1), groups.clone());
    let two = config(services_in(Some("team"), 2), groups.clone());
    let three = config(services_in(Some("team"), 3), groups);
    assert_eq!(quotas.refusal(&one, &two), None);
    let refusal = quotas.refusal(&two, &three).unwrap();
    assert!(refusal.contains("\"team\" 3 services"), "{refusal}");
    assert!(
        refusal.contains("limit of 2 (MAX_SERVICES_PER_GROUP)"),
        "{refusal}"
    );
}

#[test]
fn services_outside_any_group_are_counted_together_and_refused_one_past_the_cap() {
    let quotas = caps(2, 100, 100);
    let two = config(services_in(None, 2), vec![]);
    let three = config(services_in(None, 3), vec![]);
    let refusal = quotas.refusal(&two, &three).unwrap();
    assert!(
        refusal.contains("3 services outside any group"),
        "{refusal}"
    );
    // Another group has its own count.
    let elsewhere = config(
        [services_in(None, 2), services_in(Some("team"), 2)].concat(),
        vec![group("team", &[])],
    );
    assert_eq!(quotas.refusal(&two, &elsewhere), None);
}

#[test]
fn rules_of_a_group_are_summed_over_its_services_and_refused_one_past_the_cap() {
    let quotas = caps(100, 3, 100);
    let groups = vec![group("team", &[])];
    let at_cap = config(
        vec![service("a", Some("team"), 2), service("b", Some("team"), 1)],
        groups.clone(),
    );
    let past = config(
        vec![service("a", Some("team"), 2), service("b", Some("team"), 2)],
        groups,
    );
    assert_eq!(quotas.refusal(&config(vec![], vec![]), &at_cap), None);
    let refusal = quotas.refusal(&at_cap, &past).unwrap();
    assert!(refusal.contains("\"team\" 4 rules"), "{refusal}");
    assert!(refusal.contains("(MAX_RULES_PER_GROUP)"), "{refusal}");
    let ungrouped = quotas
        .refusal(
            &config(vec![], vec![]),
            &config(vec![service("a", None, 4)], vec![]),
        )
        .unwrap();
    assert!(
        ungrouped.contains("4 rules in the services outside any group"),
        "{ungrouped}"
    );
}

#[test]
fn groups_an_user_administers_are_refused_one_past_the_cap() {
    let quotas = caps(100, 100, 2);
    let two = config(
        vec![],
        vec![group("a", &["ana"]), group("b", &["ana", "bo"])],
    );
    let three = config(
        vec![],
        vec![
            group("a", &["ana"]),
            group("b", &["ana", "bo"]),
            group("c", &["ana"]),
        ],
    );
    assert_eq!(quotas.refusal(&config(vec![], vec![]), &two), None);
    let refusal = quotas.refusal(&two, &three).unwrap();
    assert!(
        refusal.contains("\"ana\" an admin of 3 groups"),
        "{refusal}"
    );
    assert!(
        refusal.contains("limit of 2 (MAX_GROUPS_PER_USER)"),
        "{refusal}"
    );
}

#[test]
fn a_configuration_already_over_a_cap_can_be_edited_and_shrunk_but_not_grown() {
    let quotas = caps(2, 100, 100);
    let five = config(services_in(None, 5), vec![]);
    let mut edited = five.clone();
    edited.services[0].listen_path = "/moved/*".into();
    assert_eq!(quotas.refusal(&five, &edited), None);
    assert_eq!(
        quotas.refusal(&five, &config(services_in(None, 4), vec![])),
        None
    );
    assert!(
        quotas
            .refusal(&five, &config(services_in(None, 6), vec![]))
            .is_some()
    );
}

#[test]
fn the_size_is_refused_one_byte_past_the_cap_unless_it_shrinks() {
    let quotas = Quotas {
        config_bytes: 100,
        ..Quotas::from_lookup(|_| None)
    };
    assert_eq!(quotas.size_refusal(10, 100), None);
    let refusal = quotas.size_refusal(100, 101).unwrap();
    assert!(
        refusal.contains("101 bytes long, over the limit of 100 (MAX_CONFIG_BYTES)"),
        "{refusal}"
    );
    assert_eq!(quotas.size_refusal(150, 120), None);
}

// --- The store: where every write meets the caps ---

fn temp_dir() -> PathBuf {
    crate::server::test_support::temp_data_dir("quotas")
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

async fn store_with(dir: &Path, quotas: Quotas) -> MockStore {
    MockStore::load_or_init(dir)
        .await
        .unwrap()
        .with_quotas(quotas)
}

#[tokio::test]
async fn a_change_refused_by_a_cap_leaves_no_trace() {
    let dir = temp_dir();
    let store = store_with(&dir, caps(1, 100, 100)).await;
    store
        .update(|cfg| cfg.services.push(service("a", None, 0)))
        .await
        .unwrap();
    store.flush().await;
    let file = std::fs::read(dir.join("mock-config.yaml")).unwrap();
    let backups = files_in(&dir.join("backups"));

    let refused = store
        .try_update(|cfg| {
            cfg.services.push(service("b", None, 0));
            Ok::<(), ()>(())
        })
        .await;
    assert!(
        matches!(refused, Err(StoreError::OverQuota(ref m)) if m.contains("MAX_SERVICES_PER_GROUP"))
    );
    let refused = store
        .update(|cfg| cfg.services.push(service("b", None, 0)))
        .await;
    assert!(matches!(refused, Err(StoreError::OverQuota(_))));

    store.flush().await;
    assert_eq!(store.snapshot().await.services.len(), 1);
    assert_eq!(std::fs::read(dir.join("mock-config.yaml")).unwrap(), file);
    assert_eq!(files_in(&dir.join("backups")), backups);
}

#[tokio::test]
async fn the_store_refuses_a_change_one_byte_past_the_size_cap() {
    let dir = temp_dir();
    let probe = store_with(&dir, Quotas::from_lookup(|_| None)).await;
    let one = config(vec![service("a", None, 0)], vec![]);
    let bytes = serde_yaml::to_string(&one).unwrap().len();

    let store = store_with(
        &temp_dir(),
        Quotas {
            config_bytes: bytes,
            ..probe.quotas()
        },
    )
    .await;
    store.update(|cfg| *cfg = one.clone()).await.unwrap();
    let refused = store
        .update(|cfg| cfg.services[0].listen_path.push('x'))
        .await;
    assert!(
        matches!(refused, Err(StoreError::OverQuota(ref m)) if m.contains("MAX_CONFIG_BYTES")),
        "{refused:?}"
    );
    // Shorter is always accepted.
    store
        .update(|cfg| cfg.services[0].listen_path.truncate(2))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_import_is_held_to_the_caps_and_a_restore_is_not() {
    let dir = temp_dir();
    let store = store_with(&dir, caps(2, 100, 100)).await;
    let three = config(services_in(None, 3), vec![]);

    let refused = store.replace_capped(three.clone()).await;
    assert!(matches!(refused, Err(StoreError::OverQuota(_))));
    assert!(store.snapshot().await.services.is_empty());

    store.replace(three).await.unwrap();
    assert_eq!(store.snapshot().await.services.len(), 3);
}

#[tokio::test]
async fn a_reset_keeps_the_last_change_in_its_protected_copy_and_makes_no_ordinary_backup() {
    let dir = temp_dir();
    let store = store_with(&dir, Quotas::from_lookup(|_| None)).await;
    store
        .replace(config(vec![service("first", None, 1)], vec![]))
        .await
        .unwrap();
    store.flush().await;
    let backups = files_in(&dir.join("backups"));

    // Still on its way to the disk when the reset comes.
    store
        .update(|cfg| cfg.services.push(service("last", None, 1)))
        .await
        .unwrap();
    store.reset().await.unwrap();
    store.flush().await;

    let protected = files_in(&dir.join("backups").join("protected"));
    assert_eq!(protected.len(), 1);
    let copy = std::fs::read_to_string(&protected[0]).unwrap();
    assert!(
        copy.contains("name: last"),
        "the protected copy misses the last change:\n{copy}"
    );
    assert_eq!(
        files_in(&dir.join("backups")),
        backups,
        "a reset made an ordinary backup next to its protected copy"
    );
    assert!(store.snapshot().await.services.is_empty());
}

#[tokio::test]
async fn a_reset_of_an_empty_configuration_keeps_no_copy() {
    let dir = temp_dir();
    let store = store_with(&dir, Quotas::from_lookup(|_| None)).await;
    store.reset().await.unwrap();
    assert!(files_in(&dir.join("backups").join("protected")).is_empty());
}

#[tokio::test]
async fn protected_copies_past_the_cap_lose_the_oldest_first() {
    let dir = temp_dir();
    let store = store_with(
        &dir,
        Quotas {
            protected_backups: 2,
            ..Quotas::from_lookup(|_| None)
        },
    )
    .await;
    for name in ["one", "two", "three"] {
        store
            .replace(config(vec![service(name, None, 0)], vec![]))
            .await
            .unwrap();
        store.reset().await.unwrap();
        // Names hold the millisecond of the reset.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let kept: Vec<String> = files_in(&dir.join("backups").join("protected"))
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect();
    assert_eq!(kept.len(), 2);
    assert!(
        kept[0].contains("name: two") && kept[1].contains("name: three"),
        "{kept:?}"
    );
}
