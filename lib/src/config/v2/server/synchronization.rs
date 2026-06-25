use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Auto-enqueue behavior for newly downloaded podcast episodes.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AutoEnqueue {
    /// Download and add to playlist (oldest first per podcast).
    #[default]
    Enabled,
    /// Download only, do not add to playlist.
    Disabled,
}

/// Settings for periodic podcast synchronization.
/// Nested under [podcast.synchronization] in the TOML config.
///
/// When absent from the config file, all fields use their defaults
/// due to `#[serde(default)]` on the struct.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct SynchronizationSettings {
    /// How often to check feeds. `Duration::ZERO` means sync is disabled.
    /// Absent field also means disabled (defaults to `Duration::ZERO`).
    /// Example: "1h", "30m", "2h30m"
    #[serde(with = "humantime_serde")]
    pub interval: Duration,

    /// Whether to run a sync immediately on server startup when sync is enabled.
    /// Default: false
    pub refresh_on_startup: bool,

    /// Maximum new episodes to download per podcast per sync pass.
    /// 0 means unlimited. Default: 5
    pub max_new_episodes: u32,

    /// Whether to auto-enqueue downloaded episodes to the playlist.
    /// Default: Enabled
    pub auto_enqueue: AutoEnqueue,
}

impl Default for SynchronizationSettings {
    fn default() -> Self {
        Self {
            // Duration::ZERO means disabled — absent config means sync is off (AC-05, SCENARIO-008)
            interval: Duration::ZERO,
            // Default: false — user must opt in to refresh on startup (AC-06, SCENARIO-009)
            refresh_on_startup: false,
            // Default: 5 — limit new episodes per podcast per sync pass
            max_new_episodes: 5,
            // Default: Enabled — downloaded episodes are added to playlist
            auto_enqueue: AutoEnqueue::Enabled,
        }
    }
}
