//! Arcade Link: Arcade Box's manifest and its Link settings.
//!
//! Box exposes its tools to the other Arcade apps (Box is the ecosystem's
//! transform engine). The manifest is built from the catalog and the cached
//! provider table, so readers never trigger provider probes.

use arcade_link::{Manifest, ids};

use crate::Arcade;

/// Storage keys for the Connected apps settings.
pub const SETTING_ENABLED: &str = "link_enabled";
pub const SETTING_DISABLED_PEERS: &str = "link_disabled_peers";

/// "Connect with other Arcade apps" and the per-app toggles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSettings {
    pub enabled: bool,
    pub disabled_peers: Vec<String>,
}

impl Default for LinkSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            disabled_peers: Vec::new(),
        }
    }
}

impl LinkSettings {
    pub fn load(runtime: &Arcade) -> Self {
        let storage = runtime.storage();
        Self {
            enabled: storage
                .setting(SETTING_ENABLED)
                .ok()
                .flatten()
                .is_none_or(|v| v != "false"),
            disabled_peers: storage
                .setting(SETTING_DISABLED_PEERS)
                .ok()
                .flatten()
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default(),
        }
    }

    pub fn save(&self, runtime: &Arcade) -> Result<(), String> {
        let storage = runtime.storage();
        storage
            .set_setting(SETTING_ENABLED, if self.enabled { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
        storage
            .set_setting(
                SETTING_DISABLED_PEERS,
                &serde_json::to_string(&self.disabled_peers).unwrap_or_else(|_| "[]".into()),
            )
            .map_err(|e| e.to_string())
    }

    /// Whether entries from `peer` may appear in Box.
    pub fn uses(&self, peer: &str) -> bool {
        self.enabled && !self.disabled_peers.iter().any(|p| p == peer)
    }
}

/// Box's manifest. `executable` is the desktop app, which also serves
/// one-shot requests (`--arcade-invoke`) without starting its UI.
pub fn manifest(
    settings: &LinkSettings,
    executable: &str,
    version: &str,
    shortcut: Option<&str>,
) -> Manifest {
    let mut m = Manifest::new(ids::BOX, version, executable);
    m.launch.background = vec!["--background".into()];
    if let Some(s) = shortcut {
        m.shortcuts.push(arcade_link::manifest::Shortcut {
            id: "island".into(),
            accelerator: s.into(),
        });
    }
    m.settings.link_enabled = settings.enabled;
    m
}
