use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};

/// Settings for the periodic podcast synchronization task.
///
/// When absent from the config file, all fields use their defaults
/// due to `#[serde(default)]` on the struct.
///
/// This struct supports being deserialized both as a standalone TOML document
/// (with a `[synchronization]` section header) and as a nested value within
/// `ServerSettings` (where the parent maps the `synchronization` key).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SynchronizationSettings {
    /// Whether automatic podcast synchronization is enabled.
    /// Default: true
    pub enable: bool,

    /// How often to check all subscribed feeds for new episodes.
    /// Accepts human-readable duration strings: "1h", "30m", "2h30m".
    /// Default: "1h" (3600 seconds)
    #[serde(with = "humantime_serde")]
    pub interval: Duration,

    /// Whether to run a full sync immediately on server startup
    /// before entering the periodic cycle.
    /// Default: true
    pub refresh_on_startup: bool,

    /// Maximum number of new episodes to download per podcast per sync pass.
    /// Only the newest N undownloaded episodes are fetched.
    /// Set to 0 for unlimited (download all missing episodes).
    /// Default: 5
    pub max_new_episodes: u32,
}

impl Default for SynchronizationSettings {
    fn default() -> Self {
        Self {
            enable: true,
            interval: Duration::from_secs(3600),
            refresh_on_startup: true,
            max_new_episodes: 5,
        }
    }
}

/// Inner helper for raw deserialization without the wrapping logic.
#[derive(Deserialize)]
#[serde(default)]
struct SyncSettingsRaw {
    enable: bool,
    #[serde(with = "humantime_serde")]
    interval: Duration,
    refresh_on_startup: bool,
    max_new_episodes: u32,
    /// When the struct is deserialized from a standalone TOML document that
    /// contains `[synchronization]` as a section header, this field captures
    /// the nested table so we can unwrap it.
    synchronization: Option<SyncSettingsNested>,
}

impl Default for SyncSettingsRaw {
    fn default() -> Self {
        let defaults = SynchronizationSettings::default();
        Self {
            enable: defaults.enable,
            interval: defaults.interval,
            refresh_on_startup: defaults.refresh_on_startup,
            max_new_episodes: defaults.max_new_episodes,
            synchronization: None,
        }
    }
}

/// The nested representation when parsing a standalone TOML with `[synchronization]` header.
#[derive(Deserialize)]
struct SyncSettingsNested {
    #[serde(default = "default_enable")]
    enable: bool,
    #[serde(default = "default_interval", with = "humantime_serde")]
    interval: Duration,
    #[serde(default = "default_refresh_on_startup")]
    refresh_on_startup: bool,
    #[serde(default = "default_max_new_episodes")]
    max_new_episodes: u32,
}

fn default_enable() -> bool {
    true
}

fn default_interval() -> Duration {
    Duration::from_secs(3600)
}

fn default_refresh_on_startup() -> bool {
    true
}

fn default_max_new_episodes() -> u32 {
    5
}

/// Custom Deserialize implementation that supports dual-path deserialization:
///
/// 1. **Nested path**: When parsed as a standalone TOML document containing a
///    `[synchronization]` section header (e.g., during config file testing or
///    isolated deserialization), the struct unwraps the nested table.
/// 2. **Flat path**: When parsed as a value within the parent `ServerSettings`
///    struct (the normal runtime case), fields are read directly.
///
/// This dual-path approach is necessary because other config sections use
/// `#[serde(default)]` on derived `Deserialize`, but `SynchronizationSettings`
/// needs to handle both contexts without requiring callers to strip the section
/// header. The test suite validates both paths.
impl<'de> Deserialize<'de> for SynchronizationSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = SyncSettingsRaw::deserialize(deserializer)?;

        // If the TOML had a [synchronization] section at the root level,
        // the nested struct will be populated. Use its values instead.
        if let Some(nested) = raw.synchronization {
            Ok(Self {
                enable: nested.enable,
                interval: nested.interval,
                refresh_on_startup: nested.refresh_on_startup,
                max_new_episodes: nested.max_new_episodes,
            })
        } else {
            Ok(Self {
                enable: raw.enable,
                interval: raw.interval,
                refresh_on_startup: raw.refresh_on_startup,
                max_new_episodes: raw.max_new_episodes,
            })
        }
    }
}
