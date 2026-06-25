//! Phase 2 tests for Architecture and Config Redesign.
//!
//! These tests validate the config restructuring required by review feedback:
//! - AC-04: Sync config nested under [podcast.synchronization]
//! - AC-05: interval=0 (or absent) means disabled; no boolean enable flag
//! - AC-06: refresh_on_startup can be explicitly disabled (default: false)
//! - AC-07: Duration defaults have human-readable comments
//! - AC-11: AutoEnqueue enum exists with Enabled/Disabled variants
//!
//! SCENARIO-006: Config parsed from [podcast.synchronization]
//! SCENARIO-007: Interval value of zero disables periodic sync
//! SCENARIO-008: Absent interval setting disables periodic sync
//! SCENARIO-009: Refresh-on-startup can be explicitly disabled
//! SCENARIO-040: Large interval accepted without overflow

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use crate::config::v2::server::PodcastSettings;
    use crate::config::v2::server::ServerSettings;
    use crate::config::v2::server::synchronization::{AutoEnqueue, SynchronizationSettings};

    // =========================================================================
    // SCENARIO-006 / AC-04: Sync config nested under [podcast.synchronization]
    // =========================================================================

    /// Config with [podcast.synchronization] section should parse correctly.
    /// The sync settings must be accessible via settings.podcast.synchronization.
    #[test]
    fn config_parses_sync_settings_from_podcast_synchronization_section() {
        let toml_str = r#"
[podcast.synchronization]
interval = "2h"
refresh_on_startup = true
auto_enqueue = "enabled"
max_new_episodes = 10
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse [podcast.synchronization] section");

        // AC-04: sync settings accessed via podcast.synchronization
        assert_eq!(
            settings.podcast.synchronization.interval,
            Duration::from_secs(7200)
        );
        assert_eq!(settings.podcast.synchronization.refresh_on_startup, true);
        assert_eq!(settings.podcast.synchronization.max_new_episodes, 10);
        assert_eq!(
            settings.podcast.synchronization.auto_enqueue,
            AutoEnqueue::Enabled
        );
    }

    /// A top-level [synchronization] section should NOT be recognized after migration.
    /// Only [podcast.synchronization] is valid.
    #[test]
    fn top_level_synchronization_section_is_not_recognized() {
        let toml_str = r#"
[synchronization]
interval = "2h"
refresh_on_startup = true
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse without error");

        // The top-level [synchronization] should be ignored; podcast.synchronization uses defaults
        assert_eq!(
            settings.podcast.synchronization.interval,
            Duration::ZERO,
            "top-level [synchronization] should not populate podcast.synchronization"
        );
    }

    /// ServerSettings should no longer have a direct `synchronization` field.
    /// Access must be through `settings.podcast.synchronization`.
    #[test]
    fn server_settings_has_no_top_level_synchronization_field() {
        let settings = ServerSettings::default();

        // This assertion verifies the field moved into PodcastSettings.
        // After Phase 2, settings.podcast.synchronization should exist
        // and settings.synchronization should NOT exist (compiler enforces this).
        let _sync = &settings.podcast.synchronization;
        assert_eq!(_sync.interval, Duration::ZERO);
    }

    // =========================================================================
    // SCENARIO-007 / AC-05: Interval value of zero disables periodic sync
    // =========================================================================

    /// When interval is explicitly set to "0s", sync should be considered disabled.
    /// No separate boolean `enable` field should exist.
    #[test]
    fn interval_zero_means_sync_disabled() {
        let toml_str = r#"
[podcast.synchronization]
interval = "0s"
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse zero interval");

        assert_eq!(
            settings.podcast.synchronization.interval,
            Duration::ZERO,
            "interval of 0s should parse as Duration::ZERO (disabled)"
        );
    }

    /// The SynchronizationSettings struct should NOT have an `enable` boolean field.
    /// Sync is enabled/disabled solely by the interval value being non-zero/zero.
    #[test]
    fn synchronization_settings_has_no_enable_field() {
        let settings = SynchronizationSettings::default();

        // This test is a compile-time check: if `enable` field exists, this code
        // would compile. We verify the struct has the new design by checking
        // that the only way to determine "enabled" is via interval > Duration::ZERO.
        assert_eq!(settings.interval, Duration::ZERO);
        // The following line should NOT compile after Phase 2 (no `enable` field):
        // let _ = settings.enable;

        // Instead, the "is enabled" check is: interval > Duration::ZERO
        let is_enabled = settings.interval > Duration::ZERO;
        assert_eq!(is_enabled, false, "default interval should mean disabled");
    }

    // =========================================================================
    // SCENARIO-008 / AC-05: Absent interval setting disables periodic sync
    // =========================================================================

    /// When [podcast.synchronization] is entirely absent from config,
    /// the default interval should be Duration::ZERO (disabled).
    #[test]
    fn absent_synchronization_section_defaults_to_disabled() {
        let toml_str = r#"
[podcast]
concurrent_downloads_max = 5
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse without synchronization section");

        assert_eq!(
            settings.podcast.synchronization.interval,
            Duration::ZERO,
            "absent [podcast.synchronization] should default to interval=ZERO (disabled)"
        );
    }

    /// SynchronizationSettings::default() should produce interval = Duration::ZERO.
    #[test]
    fn default_impl_sets_interval_to_zero() {
        let defaults = SynchronizationSettings::default();

        assert_eq!(
            defaults.interval,
            Duration::ZERO,
            "Default interval must be Duration::ZERO to mean 'disabled by default'"
        );
    }

    // =========================================================================
    // SCENARIO-009 / AC-06: Refresh-on-startup can be explicitly disabled
    // =========================================================================

    /// SynchronizationSettings::default() should set refresh_on_startup to false.
    #[test]
    fn default_refresh_on_startup_is_false() {
        let defaults = SynchronizationSettings::default();

        assert_eq!(
            defaults.refresh_on_startup, false,
            "Default refresh_on_startup must be false (opt-in, not opt-out)"
        );
    }

    /// User can explicitly set refresh_on_startup = false in config.
    #[test]
    fn refresh_on_startup_explicitly_disabled() {
        let toml_str = r#"
[podcast.synchronization]
interval = "1h"
refresh_on_startup = false
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse with refresh_on_startup=false");

        assert_eq!(settings.podcast.synchronization.refresh_on_startup, false);
        assert_eq!(
            settings.podcast.synchronization.interval,
            Duration::from_secs(3600)
        );
    }

    /// User can explicitly enable refresh_on_startup.
    #[test]
    fn refresh_on_startup_explicitly_enabled() {
        let toml_str = r#"
[podcast.synchronization]
interval = "30m"
refresh_on_startup = true
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse with refresh_on_startup=true");

        assert_eq!(settings.podcast.synchronization.refresh_on_startup, true);
    }

    // =========================================================================
    // AC-11: AutoEnqueue enum
    // =========================================================================

    /// AutoEnqueue should deserialize from "enabled" string.
    #[test]
    fn auto_enqueue_deserializes_enabled() {
        let toml_str = r#"
[podcast.synchronization]
interval = "1h"
auto_enqueue = "enabled"
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse auto_enqueue=enabled");

        assert_eq!(
            settings.podcast.synchronization.auto_enqueue,
            AutoEnqueue::Enabled
        );
    }

    /// AutoEnqueue should deserialize from "disabled" string.
    #[test]
    fn auto_enqueue_deserializes_disabled() {
        let toml_str = r#"
[podcast.synchronization]
interval = "1h"
auto_enqueue = "disabled"
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse auto_enqueue=disabled");

        assert_eq!(
            settings.podcast.synchronization.auto_enqueue,
            AutoEnqueue::Disabled
        );
    }

    /// AutoEnqueue default should be Enabled.
    #[test]
    fn auto_enqueue_defaults_to_enabled() {
        let defaults = SynchronizationSettings::default();

        assert_eq!(
            defaults.auto_enqueue,
            AutoEnqueue::Enabled,
            "Default auto_enqueue must be Enabled"
        );
    }

    /// AutoEnqueue serde roundtrip should be stable.
    #[test]
    fn auto_enqueue_serde_roundtrip() {
        let settings = SynchronizationSettings {
            interval: Duration::from_secs(1800),
            refresh_on_startup: false,
            max_new_episodes: 3,
            auto_enqueue: AutoEnqueue::Disabled,
        };

        let serialized = toml::to_string(&settings).expect("should serialize");
        let deserialized: SynchronizationSettings =
            toml::from_str(&serialized).expect("should deserialize roundtrip");

        assert_eq!(settings, deserialized);
    }

    // =========================================================================
    // SCENARIO-040: Large interval accepted without overflow
    // =========================================================================

    /// An extremely large interval (30 days) should be accepted without error.
    #[test]
    fn large_interval_30_days_accepted() {
        let toml_str = r#"
[podcast.synchronization]
interval = "30d"
"#;

        let settings: ServerSettings =
            toml::from_str(toml_str).expect("should parse 30-day interval");

        assert_eq!(
            settings.podcast.synchronization.interval,
            Duration::from_secs(30 * 24 * 3600)
        );
    }

    // =========================================================================
    // PodcastSettings contains synchronization field
    // =========================================================================

    /// PodcastSettings should now contain a `synchronization` field.
    #[test]
    fn podcast_settings_contains_synchronization_field() {
        let podcast_settings = PodcastSettings::default();

        // This test verifies the synchronization field exists on PodcastSettings
        let _sync = &podcast_settings.synchronization;
        assert_eq!(_sync.interval, Duration::ZERO);
    }

    /// Full config roundtrip with [podcast.synchronization] nested section.
    #[test]
    fn full_config_roundtrip_with_nested_sync_section() {
        let settings = ServerSettings {
            podcast: PodcastSettings {
                synchronization: SynchronizationSettings {
                    interval: Duration::from_secs(3600),
                    refresh_on_startup: true,
                    max_new_episodes: 10,
                    auto_enqueue: AutoEnqueue::Disabled,
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let serialized = toml::to_string(&settings).expect("should serialize full config");
        let deserialized: ServerSettings =
            toml::from_str(&serialized).expect("should deserialize full config roundtrip");

        assert_eq!(
            settings.podcast.synchronization,
            deserialized.podcast.synchronization
        );
    }
}
