//! Named connection profiles: data model plus the pure merge / resolve /
//! render logic. All file access lives in [`crate::store`].

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Host used when neither a profile nor an explicit flag supplies one.
pub const DEFAULT_HOST: &str = "localhost";
/// Port used when neither a profile nor an explicit flag supplies one.
pub const DEFAULT_PORT: u16 = 8080;

/// Placeholder shown by `profile list` for a field that has no value.
const UNSET: &str = "-";

/// One saved connection profile.
///
/// Every field is optional so `profile set` can update a single field without
/// clearing the rest (ADR-0016), and so unset fields stay out of the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Destination host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Destination port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Config file path used by `reload`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    /// Default operation chains sent by `send` when no operation is given on
    /// the command line (ADR-0018). Each inner vector is one chain, written as
    /// route strings. Edited through `profile chains`, never through
    /// `profile set`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chains: Option<Vec<Vec<String>>>,
}

/// The whole `profiles.toml` file.
///
/// `BTreeMap` keeps `profile list` output stable and name-sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileStore {
    /// Saved profiles keyed by name.
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

impl ProfileStore {
    /// Merge `patch` into the profile named `name`, creating it when absent.
    /// Fields left `None` in `patch` keep their existing value (ADR-0016).
    ///
    /// `chains` is deliberately not merged: it is owned by `profile chains`
    /// (ADR-0018), so a `profile set` can never clear it.
    pub fn merge(&mut self, name: &str, patch: &Profile) {
        let entry = self.profiles.entry(name.to_string()).or_default();
        if patch.host.is_some() {
            entry.host.clone_from(&patch.host);
        }
        if patch.port.is_some() {
            entry.port = patch.port;
        }
        if patch.config_path.is_some() {
            entry.config_path.clone_from(&patch.config_path);
        }
    }

    /// Remove the named profile.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists.
    pub fn remove(&mut self, name: &str) -> anyhow::Result<()> {
        if self.profiles.remove(name).is_some() {
            return Ok(());
        }
        Err(self.unknown(name))
    }

    /// Look up a profile by name.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists.
    pub fn get(&self, name: &str) -> anyhow::Result<&Profile> {
        self.profiles.get(name).ok_or_else(|| self.unknown(name))
    }

    /// Append `chain` to the named profile's default chains.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists. A typo must not silently
    /// create a profile — `profile set` is the only command that creates one.
    pub fn add_chain(&mut self, name: &str, chain: Vec<String>) -> anyhow::Result<()> {
        if !self.profiles.contains_key(name) {
            return Err(self.unknown(name));
        }
        let entry = self
            .profiles
            .get_mut(name)
            .expect("contains_key checked immediately above");
        entry.chains.get_or_insert_with(Vec::new).push(chain);
        Ok(())
    }

    /// Remove the `index`-th (1-based, as shown by `profile chains show`)
    /// chain from the named profile.
    ///
    /// Removing the last chain drops the field entirely, so the profile ends up
    /// indistinguishable from one that never saved a chain.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists, or when `index` is outside
    /// the saved range. In both cases nothing is modified.
    pub fn del_chain(&mut self, name: &str, index: usize) -> anyhow::Result<()> {
        if !self.profiles.contains_key(name) {
            return Err(self.unknown(name));
        }
        let entry = self
            .profiles
            .get_mut(name)
            .expect("contains_key checked immediately above");
        let saved = entry.chains.as_ref().map_or(0, Vec::len);
        if index == 0 || index > saved {
            return Err(anyhow!(
                "チェーン番号 {index} は範囲外です（プロファイル '{name}' に保存されているチェーンは {saved} 本）\n\
                 確認: vstc_cli profile chains show {name}"
            ));
        }
        if let Some(chains) = entry.chains.as_mut() {
            chains.remove(index - 1);
            if chains.is_empty() {
                entry.chains = None;
            }
        }
        Ok(())
    }

    /// The named profile's saved chains, empty when it has none.
    ///
    /// ## Errors
    ///
    /// Fails when no profile with that name exists.
    pub fn chains_of(&self, name: &str) -> anyhow::Result<&[Vec<String>]> {
        Ok(self.get(name)?.chains.as_deref().unwrap_or(&[]))
    }

    /// Error for an unknown profile name, listing what is available so the
    /// user can fix a typo without a second command.
    fn unknown(&self, name: &str) -> anyhow::Error {
        if self.profiles.is_empty() {
            return anyhow!(
                "プロファイル '{name}' は登録されていません（登録済みプロファイルはありません）\n\
                 作成: vstc_cli profile set {name} --host <HOST> --port <PORT>"
            );
        }
        let known: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
        anyhow!(
            "プロファイル '{name}' は登録されていません\n登録済み: {}",
            known.join(", ")
        )
    }
}

/// Values given explicitly on the command line. `None` means "not given".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `--host`
    pub host: Option<String>,
    /// `--port`
    pub port: Option<u16>,
    /// `--config-path`
    pub config_path: Option<String>,
}

/// The effective values for one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Destination host.
    pub host: String,
    /// Destination port.
    pub port: u16,
    /// Config file path, if any source supplied one.
    pub config_path: Option<String>,
}

impl Resolved {
    /// gRPC endpoint URI for the resolved host and port.
    pub fn uri(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Resolve the effective values: built-in defaults, then the profile, then the
/// explicit flags — later sources win, field by field (ADR-0016).
pub fn resolve(profile: Option<&Profile>, overrides: &Overrides) -> Resolved {
    Resolved {
        host: overrides
            .host
            .clone()
            .or_else(|| profile.and_then(|p| p.host.clone()))
            .unwrap_or_else(|| DEFAULT_HOST.to_string()),
        port: overrides
            .port
            .or_else(|| profile.and_then(|p| p.port))
            .unwrap_or(DEFAULT_PORT),
        config_path: overrides
            .config_path
            .clone()
            .or_else(|| profile.and_then(|p| p.config_path.clone())),
    }
}

/// Render the aligned table shown by `profile list`.
/// Returns an empty string when there is nothing to show.
pub fn render_list(store: &ProfileStore) -> String {
    if store.profiles.is_empty() {
        return String::new();
    }
    let mut rows: Vec<[String; 5]> = vec![[
        "NAME".to_string(),
        "HOST".to_string(),
        "PORT".to_string(),
        "CONFIG_PATH".to_string(),
        "CHAINS".to_string(),
    ]];
    for (name, p) in &store.profiles {
        rows.push([
            name.clone(),
            p.host.clone().unwrap_or_else(|| UNSET.to_string()),
            p.port
                .map_or_else(|| UNSET.to_string(), |port| port.to_string()),
            p.config_path.clone().unwrap_or_else(|| UNSET.to_string()),
            p.chains
                .as_ref()
                .map_or_else(|| UNSET.to_string(), |c| c.len().to_string()),
        ]);
    }
    let widths: Vec<usize> = (0..5)
        .map(|i| rows.iter().map(|r| r[i].chars().count()).max().unwrap_or(0))
        .collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = row
                .iter()
                .zip(&widths)
                .map(|(cell, width)| format!("{cell:<width$}", width = *width))
                .collect();
            cells.join("  ").trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render the numbered chain list shown by `profile chains show`.
/// Returns an empty string when the profile has no saved chains.
pub fn render_chains(chains: &[Vec<String>]) -> String {
    chains
        .iter()
        .enumerate()
        .map(|(i, chain)| format!("[{}] {}", i + 1, chain.join(" -> ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(host: &str, port: u16) -> Profile {
        Profile {
            host: Some(host.to_string()),
            port: Some(port),
            config_path: None,
            chains: None,
        }
    }

    #[test]
    fn merge_creates_profile_when_absent() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        assert_eq!(store.profiles["main"], profile("h", 1));
    }

    #[test]
    fn merge_keeps_fields_the_patch_leaves_unset() {
        let mut store = ProfileStore::default();
        store.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                port: Some(1),
                config_path: Some("c.yml".to_string()),
                chains: None,
            },
        );
        store.merge(
            "main",
            &Profile {
                port: Some(2),
                ..Profile::default()
            },
        );
        let got = &store.profiles["main"];
        assert_eq!(got.host.as_deref(), Some("h"));
        assert_eq!(got.port, Some(2));
        assert_eq!(got.config_path.as_deref(), Some("c.yml"));
    }

    #[test]
    fn remove_existing_profile_succeeds() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store.remove("main").expect("remove should succeed");
        assert!(store.profiles.is_empty());
    }

    #[test]
    fn remove_unknown_profile_errors_and_lists_known_names() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store.merge("sub", &profile("h", 2));
        let err = store.remove("nope").expect_err("unknown name should error");
        let msg = err.to_string();
        assert!(
            msg.contains("nope"),
            "message should name the profile: {msg}"
        );
        assert!(
            msg.contains("main"),
            "message should list known names: {msg}"
        );
        assert!(
            msg.contains("sub"),
            "message should list known names: {msg}"
        );
    }

    #[test]
    fn get_unknown_profile_on_empty_store_suggests_creating_one() {
        let store = ProfileStore::default();
        let err = store.get("nope").expect_err("unknown name should error");
        let msg = err.to_string();
        assert!(
            msg.contains("nope"),
            "message should name the profile: {msg}"
        );
        assert!(
            msg.contains("profile set"),
            "message should suggest how to create one: {msg}"
        );
    }

    #[test]
    fn resolve_falls_back_to_builtin_defaults() {
        let got = resolve(None, &Overrides::default());
        assert_eq!(got.host, DEFAULT_HOST);
        assert_eq!(got.port, DEFAULT_PORT);
        assert_eq!(got.config_path, None);
    }

    #[test]
    fn resolve_prefers_profile_over_defaults() {
        let p = profile("ph", 111);
        let got = resolve(Some(&p), &Overrides::default());
        assert_eq!(got.host, "ph");
        assert_eq!(got.port, 111);
    }

    #[test]
    fn resolve_prefers_explicit_flags_over_profile() {
        let p = Profile {
            host: Some("ph".to_string()),
            port: Some(111),
            config_path: Some("pc.yml".to_string()),
            chains: None,
        };
        let overrides = Overrides {
            host: Some("oh".to_string()),
            port: Some(222),
            config_path: Some("oc.yml".to_string()),
        };
        let got = resolve(Some(&p), &overrides);
        assert_eq!(got.host, "oh");
        assert_eq!(got.port, 222);
        assert_eq!(got.config_path.as_deref(), Some("oc.yml"));
    }

    #[test]
    fn resolve_overrides_each_field_independently() {
        // ADR-0016: 明示した項目だけが勝ち、他はプロファイル値が残る。
        let p = Profile {
            host: Some("ph".to_string()),
            port: Some(111),
            config_path: Some("pc.yml".to_string()),
            chains: None,
        };
        let overrides = Overrides {
            port: Some(222),
            ..Overrides::default()
        };
        let got = resolve(Some(&p), &overrides);
        assert_eq!(got.host, "ph");
        assert_eq!(got.port, 222);
        assert_eq!(got.config_path.as_deref(), Some("pc.yml"));
    }

    #[test]
    fn resolved_uri_is_http_host_port() {
        let got = resolve(None, &Overrides::default());
        assert_eq!(got.uri(), "http://localhost:8080");
    }

    #[test]
    fn render_list_is_empty_for_empty_store() {
        assert_eq!(render_list(&ProfileStore::default()), "");
    }

    #[test]
    fn render_list_sorts_by_name_and_marks_unset_fields() {
        let mut store = ProfileStore::default();
        store.merge(
            "zeta",
            &Profile {
                host: Some("zh".to_string()),
                ..Profile::default()
            },
        );
        store.merge(
            "alpha",
            &Profile {
                host: Some("ah".to_string()),
                port: Some(1),
                config_path: Some("a.yml".to_string()),
                chains: None,
            },
        );
        let rendered = render_list(&store);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        assert!(lines[0].starts_with("NAME"));
        assert!(lines[1].starts_with("alpha"), "sorted first: {}", lines[1]);
        assert!(lines[2].starts_with("zeta"), "sorted second: {}", lines[2]);
        assert!(lines[1].contains("a.yml"));
        assert!(
            lines[2].contains('-'),
            "unset port/config_path shown as dash: {}",
            lines[2]
        );
    }

    #[test]
    fn store_round_trips_through_toml() {
        let mut store = ProfileStore::default();
        store.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                port: Some(19829),
                config_path: Some("c.yml".to_string()),
                chains: None,
            },
        );
        let text = toml::to_string(&store).expect("serialize");
        let back: ProfileStore = toml::from_str(&text).expect("deserialize");
        assert_eq!(back, store);
    }

    #[test]
    fn unset_fields_are_not_written_to_toml() {
        let mut store = ProfileStore::default();
        store.merge(
            "main",
            &Profile {
                host: Some("h".to_string()),
                ..Profile::default()
            },
        );
        let text = toml::to_string(&store).expect("serialize");
        assert!(text.contains("host"), "set field is written: {text}");
        assert!(!text.contains("port"), "unset field is omitted: {text}");
        assert!(
            !text.contains("config_path"),
            "unset field is omitted: {text}"
        );
    }

    #[test]
    fn empty_toml_deserializes_to_empty_store() {
        let back: ProfileStore = toml::from_str("").expect("deserialize empty");
        assert!(back.profiles.is_empty());
    }

    #[test]
    fn render_chains_numbers_from_one_and_shows_hop_order() {
        let chains = vec![
            vec![
                "//localhost:8081/transc".to_string(),
                "//windesk:8080/sub".to_string(),
            ],
            vec![
                "//localhost:8081/transc".to_string(),
                "transl?t=en".to_string(),
            ],
        ];
        let rendered = render_chains(&chains);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "[1] //localhost:8081/transc -> //windesk:8080/sub"
        );
        assert_eq!(lines[1], "[2] //localhost:8081/transc -> transl?t=en");
    }

    #[test]
    fn render_chains_is_empty_when_there_are_none() {
        assert_eq!(render_chains(&[]), "");
    }

    #[test]
    fn render_list_shows_the_saved_chain_count() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store.merge("sub", &profile("h", 2));
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        store
            .add_chain("main", vec!["transl?t=en".to_string()])
            .expect("add");
        let rendered = render_list(&store);
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        assert!(
            lines[0].contains("CHAINS"),
            "header should name the column: {}",
            lines[0]
        );
        assert!(
            lines[1].ends_with('2'),
            "main has 2 saved chains: {}",
            lines[1]
        );
        assert!(
            lines[2].ends_with('-'),
            "sub has none, shown as unset: {}",
            lines[2]
        );
    }

    #[test]
    fn add_chain_appends_to_an_existing_profile() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain("main", vec!["//h:1/transc".to_string(), "sub".to_string()])
            .expect("add");
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        let got = store.profiles["main"].chains.clone().expect("chains saved");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], vec!["//h:1/transc".to_string(), "sub".to_string()]);
        assert_eq!(got[1], vec!["tts".to_string()]);
    }

    #[test]
    fn add_chain_to_an_unknown_profile_errors_and_creates_nothing() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        let err = store
            .add_chain("nope", vec!["tts".to_string()])
            .expect_err("unknown name should error");
        let msg = err.to_string();
        assert!(msg.contains("nope"), "should name the profile: {msg}");
        assert!(msg.contains("main"), "should list known names: {msg}");
        assert_eq!(
            store.profiles.len(),
            1,
            "a typo must not silently create a profile"
        );
    }

    #[test]
    fn del_chain_removes_by_one_based_index() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        for op in ["a-tts", "b-tts", "c-tts"] {
            store.add_chain("main", vec![op.to_string()]).expect("add");
        }
        store.del_chain("main", 2).expect("del the middle one");
        let got = store.profiles["main"].chains.clone().expect("chains saved");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], vec!["a-tts".to_string()]);
        assert_eq!(got[1], vec!["c-tts".to_string()]);
    }

    #[test]
    fn del_chain_of_the_last_chain_clears_the_field() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        store.del_chain("main", 1).expect("del");
        assert_eq!(
            store.profiles["main"].chains, None,
            "the profile must end up identical to one that never saved a chain"
        );
    }

    #[test]
    fn del_chain_rejects_out_of_range_indexes_without_changing_anything() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        for bad in [0, 2, 99] {
            let err = store
                .del_chain("main", bad)
                .expect_err("out-of-range index should error");
            let msg = err.to_string();
            assert!(
                msg.contains("1 本"),
                "should state how many are saved (for {bad}): {msg}"
            );
        }
        assert_eq!(
            store.profiles["main"].chains.as_ref().map(Vec::len),
            Some(1),
            "a rejected delete must leave the chains untouched"
        );
    }

    #[test]
    fn chains_of_returns_empty_for_a_profile_without_chains() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        assert!(store.chains_of("main").expect("known profile").is_empty());
    }

    #[test]
    fn chains_of_an_unknown_profile_errors() {
        let store = ProfileStore::default();
        assert!(store.chains_of("nope").is_err());
    }

    #[test]
    fn merge_never_touches_saved_chains() {
        // `profile set` は host/port/config_path 専用。チェーンは
        // `profile chains` が持つので、set が消してはいけない（ADR-0018）。
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain("main", vec!["tts".to_string()])
            .expect("add");
        store.merge(
            "main",
            &Profile {
                port: Some(2),
                ..Profile::default()
            },
        );
        assert_eq!(
            store.profiles["main"].chains.as_ref().map(Vec::len),
            Some(1)
        );
        assert_eq!(store.profiles["main"].port, Some(2));
    }

    #[test]
    fn chains_round_trip_through_toml() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        store
            .add_chain(
                "main",
                vec![
                    "//localhost:8081/transc".to_string(),
                    "//windesk:8080/sub".to_string(),
                ],
            )
            .expect("add");
        store
            .add_chain(
                "main",
                vec![
                    "//localhost:8081/transc".to_string(),
                    "transl?t=en".to_string(),
                ],
            )
            .expect("add");
        let text = toml::to_string(&store).expect("serialize");
        let back: ProfileStore = toml::from_str(&text).expect("deserialize");
        assert_eq!(back, store);
    }

    #[test]
    fn a_profile_without_chains_writes_no_chains_key() {
        let mut store = ProfileStore::default();
        store.merge("main", &profile("h", 1));
        let text = toml::to_string(&store).expect("serialize");
        assert!(
            !text.contains("chains"),
            "unset chains must stay out of the file: {text}"
        );
    }

    #[test]
    fn misspelled_field_in_a_hand_edited_file_fails_to_deserialize() {
        // ADR-0015 advertises hand-editing profiles.toml as supported. Without
        // `deny_unknown_fields`, a typo like `prot` (instead of `port`) would
        // silently be dropped and the profile would load with `port: None`,
        // sending to the default port instead of erroring. Assert on the
        // error, not on a silently-defaulted value.
        let text = "[profiles.main]\nhost = \"h\"\nprot = 19829\n";
        let err = toml::from_str::<ProfileStore>(text)
            .expect_err("unknown field `prot` must fail to deserialize");
        let msg = err.to_string();
        assert!(
            msg.contains("prot"),
            "error should name the unrecognized field: {msg}"
        );
    }
}
