//! Tests for `SynchronizationSettings` configuration struct.
//!
//! These tests cover the podcast synchronization config after Phase 2 redesign:
//! - AC-04: Config section nested under [podcast.synchronization]
//! - AC-05: interval=0 (or absent) means disabled; no boolean enable flag
//! - AC-06: refresh_on_startup defaults to false
//! - AC-07: Duration defaults have human-readable comments
//! - AC-11: AutoEnqueue enum with Enabled/Disabled variants
//! - SCENARIO-006: Config parsed from [podcast.synchronization]
//! - SCENARIO-007: Interval value of zero disables periodic sync
//! - SCENARIO-008: Absent interval setting disables periodic sync
//! - SCENARIO-009: Refresh-on-startup can be explicitly disabled

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pretty_assertions::assert_eq;

    use crate::config::v2::server::synchronization::{AutoEnqueue, SynchronizationSettings};

    // =========================================================================
    // Default values after Phase 2 redesign
    // =========================================================================

    /// The Default impl for SynchronizationSettings should produce disabled-by-default values.
    #[test]
    fn default_impl_produces_correct_values() {
        let defaults = SynchronizationSettings::default();

        // AC-05: interval=ZERO means disabled
        assert_eq!(defaults.interval, Duration::ZERO);
        // AC-06: refresh_on_startup defaults to false
        assert_eq!(defaults.refresh_on_startup, false);
        // max_new_episodes defaults to 5
        assert_eq!(defaults.max_new_episodes, 5);
        // auto_enqueue defaults to Enabled
        assert_eq!(defaults.auto_enqueue, AutoEnqueue::Enabled);
    }

    // =========================================================================
    // Deserialization from TOML
    // =========================================================================

    /// When the config TOML specifies all synchronization fields explicitly with
    /// non-default values, the deserialized struct should reflect those values.
    #[test]
    fn explicit_non_default_values_deserialized_correctly() {
        let toml_str = r#"
interval = "30m"
refresh_on_startup = true
max_new_episodes = 10
auto_enqueue = "disabled"
"#;

        let settings: SynchronizationSettings =
            toml::from_str(toml_str).expect("should parse explicit values");

        assert_eq!(settings.interval, Duration::from_secs(30 * 60));
        assert_eq!(settings.refresh_on_startup, true);
        assert_eq!(settings.max_new_episodes, 10);
        assert_eq!(settings.auto_enqueue, AutoEnqueue::Disabled);
    }

    /// Test with a different non-default interval value to prevent hardcoding.
    #[test]
    fn explicit_interval_2h30m_deserialized_correctly() {
        let toml_str = r#"
interval = "2h30m"
refresh_on_startup = true
"#;

        let settings: SynchronizationSettings =
            toml::from_str(toml_str).expect("should parse 2h30m interval");

        assert_eq!(settings.interval, Duration::from_secs(2 * 3600 + 30 * 60));
    }

    /// Test with a seconds-only interval to verify varied duration formats.
    #[test]
    fn explicit_interval_seconds_only() {
        let toml_str = r#"
interval = "45s"
"#;

        let settings: SynchronizationSettings =
            toml::from_str(toml_str).expect("should parse 45s interval");

        assert_eq!(settings.interval, Duration::from_secs(45));
    }

    // =========================================================================
    // Configuration roundtrip preserves all fields
    // =========================================================================

    /// Serializing and then deserializing SynchronizationSettings with non-default
    /// values should produce identical output.
    #[test]
    fn serialization_roundtrip_preserves_all_fields() {
        let original = SynchronizationSettings {
            interval: Duration::from_secs(1800), // 30 minutes
            refresh_on_startup: true,
            max_new_episodes: 10,
            auto_enqueue: AutoEnqueue::Disabled,
        };

        let serialized = toml::to_string(&original).expect("should serialize");
        let deserialized: SynchronizationSettings =
            toml::from_str(&serialized).expect("should deserialize roundtrip");

        assert_eq!(original, deserialized);
    }

    /// Roundtrip with default values should also be stable.
    #[test]
    fn serialization_roundtrip_default_values() {
        let original = SynchronizationSettings::default();

        let serialized = toml::to_string(&original).expect("should serialize defaults");
        let deserialized: SynchronizationSettings =
            toml::from_str(&serialized).expect("should deserialize roundtrip defaults");

        assert_eq!(original, deserialized);
    }

    // =========================================================================
    // Invalid duration string rejected
    // =========================================================================

    /// An unparseable duration string should produce a deserialization error.
    #[test]
    fn invalid_duration_string_produces_error() {
        let toml_str = r#"
interval = "not_a_duration"
"#;

        let result = toml::from_str::<SynchronizationSettings>(toml_str);
        assert!(
            result.is_err(),
            "expected deserialization error for invalid duration, got: {:?}",
            result
        );
    }

    /// An empty string should also be rejected as invalid.
    #[test]
    fn empty_duration_string_produces_error() {
        let toml_str = r#"
interval = ""
"#;

        let result = toml::from_str::<SynchronizationSettings>(toml_str);
        assert!(
            result.is_err(),
            "expected deserialization error for empty duration, got: {:?}",
            result
        );
    }

    /// A numeric value without unit should be rejected.
    #[test]
    fn numeric_without_unit_produces_error() {
        let toml_str = r#"
interval = "3600"
"#;

        let result = toml::from_str::<SynchronizationSettings>(toml_str);
        assert!(
            result.is_err(),
            "expected deserialization error for numeric without unit, got: {:?}",
            result
        );
    }

    // =========================================================================
    // Struct-level properties
    // =========================================================================

    /// SynchronizationSettings should implement PartialEq for comparison in tests.
    #[test]
    fn synchronization_settings_equality() {
        let a = SynchronizationSettings {
            interval: Duration::ZERO,
            refresh_on_startup: false,
            max_new_episodes: 5,
            auto_enqueue: AutoEnqueue::Enabled,
        };
        let b = SynchronizationSettings::default();
        assert_eq!(a, b);
    }

    /// SynchronizationSettings with different values should not be equal.
    #[test]
    fn synchronization_settings_inequality() {
        let a = SynchronizationSettings::default();
        let b = SynchronizationSettings {
            interval: Duration::from_secs(1800),
            refresh_on_startup: true,
            max_new_episodes: 10,
            auto_enqueue: AutoEnqueue::Disabled,
        };
        assert_ne!(a, b);
    }
}
