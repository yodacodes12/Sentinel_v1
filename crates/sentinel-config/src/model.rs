use std::fmt;

use sentinel_core::{Result, SentinelError};
use serde::{Deserialize, Serialize};

/// Version written to new configuration files. Bump it together with a
/// migration step in `store::migrate`.
pub const CONFIG_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentinelConfig {
    /// Schema version of the file, for migrations.
    pub version: u32,
    /// Whether the user wants monitoring on. Defaults to off: installing or
    /// starting Sentinel never enables background monitoring by itself.
    #[serde(default)]
    pub monitoring_enabled: bool,
    #[serde(default)]
    pub power_mode: PowerMode,
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub theme: Theme,
    /// When on and the computer runs on battery, sampling uses the Eco
    /// profile. Off by default; when on, the UI shows when it applies.
    #[serde(default)]
    pub power_aware_sampling: bool,
    #[serde(default)]
    pub alerts: AlertThresholds,
    #[serde(default)]
    pub retention: Retention,
    /// Phase 6 document discovery. Online discovery and automatic
    /// downloading are off until the user turns them on.
    #[serde(default)]
    pub documents: DocumentSettings,
}

/// Document discovery settings. See docs/DOCUMENT_DISCOVERY.md.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DocumentSettings {
    /// Search online sources. Off: only the local document library is used
    /// and Sentinel makes no network request at all.
    pub online_discovery: bool,
    /// Search the Internet Archive's public catalogue when online.
    pub internet_archive: bool,
    /// HTTPS addresses of documentation catalogues (JSON).
    pub catalogue_urls: Vec<String>,
    pub request_timeout_s: u32,
    /// Least time between two requests to the same site.
    pub min_request_interval_ms: u32,
    /// How long search results are reused before searching again.
    pub cache_days: u32,
    pub max_download_mb: u32,
    /// Download exact or high-confidence matches from the manufacturer's own
    /// site after a search. Off by default; never applies to other sources.
    pub auto_download: bool,
}

impl Default for DocumentSettings {
    fn default() -> Self {
        Self {
            online_discovery: false,
            internet_archive: true,
            catalogue_urls: Vec::new(),
            request_timeout_s: 20,
            min_request_interval_ms: 1500,
            cache_days: 14,
            max_download_mb: 100,
            auto_download: false,
        }
    }
}

impl DocumentSettings {
    pub const MAX_CATALOGUES: usize = 8;

    pub fn validate(&self) -> Result<()> {
        let bad = |why: &str| Err(SentinelError::InvalidConfig(format!("documents: {why}")));
        if !(3..=120).contains(&self.request_timeout_s) {
            return bad("request timeout must be 3–120 seconds");
        }
        if !(250..=60_000).contains(&self.min_request_interval_ms) {
            return bad("the interval between requests to one site must be 250–60000 ms");
        }
        if !(1..=365).contains(&self.cache_days) {
            return bad("results can be reused for 1–365 days");
        }
        if !(1..=1024).contains(&self.max_download_mb) {
            return bad("the download limit must be 1–1024 MB");
        }
        if self.catalogue_urls.len() > Self::MAX_CATALOGUES {
            return bad("at most 8 catalogue addresses");
        }
        for url in &self.catalogue_urls {
            let host = url
                .strip_prefix("https://")
                .map(|r| r.split(['/', '?', '#']).next().unwrap_or(""));
            if url.len() > 512
                || url.chars().any(|c| c.is_whitespace() || c.is_control())
                || host.is_none_or(|h| !h.contains('.') || h.contains('@'))
            {
                return bad("catalogue addresses must be https:// addresses of a named host");
            }
        }
        Ok(())
    }
}

/// Alert thresholds. Defaults are documented in `docs/ALERT_ENGINE.md`.
/// Temperature has no default: limits differ between components, so only
/// driver-reported limits or a threshold the user sets are used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AlertThresholds {
    pub cpu_high_percent: f64,
    pub cpu_high_duration_s: u32,
    /// Load average per logical CPU.
    pub load_per_cpu_high: f64,
    pub memory_available_low_percent: f64,
    pub memory_available_critical_percent: f64,
    pub swap_activity_pages_per_s: f64,
    pub battery_low_percent: f64,
    pub battery_critical_percent: f64,
    pub battery_health_warning_percent: f64,
    pub storage_free_low_percent: f64,
    pub storage_free_critical_percent: f64,
    pub disk_busy_high_percent: f64,
    /// User-defined warning for every temperature sensor. `None`: only
    /// driver-reported limits are used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_warning_c: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_temperature_warning_c: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_discharge_high_w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_high_mbps: Option<f64>,
    /// How long most conditions must hold before an alert is raised.
    pub sustain_s: u32,
    /// Minimum time before the same condition notifies again after it
    /// resolved.
    pub cooldown_s: u32,
}

impl Default for AlertThresholds {
    fn default() -> Self {
        Self {
            cpu_high_percent: 90.0,
            cpu_high_duration_s: 120,
            load_per_cpu_high: 2.0,
            memory_available_low_percent: 10.0,
            memory_available_critical_percent: 5.0,
            swap_activity_pages_per_s: 500.0,
            battery_low_percent: 15.0,
            battery_critical_percent: 5.0,
            battery_health_warning_percent: 60.0,
            storage_free_low_percent: 10.0,
            storage_free_critical_percent: 5.0,
            disk_busy_high_percent: 90.0,
            temperature_warning_c: None,
            battery_temperature_warning_c: None,
            battery_discharge_high_w: None,
            network_high_mbps: None,
            sustain_s: 60,
            cooldown_s: 600,
        }
    }
}

impl AlertThresholds {
    pub fn validate(&self) -> Result<()> {
        let pct = |name: &str, v: f64| {
            if v.is_finite() && (0.0..=100.0).contains(&v) {
                Ok(())
            } else {
                Err(SentinelError::InvalidConfig(format!(
                    "{name} must be between 0 and 100, found {v}"
                )))
            }
        };
        pct("cpu_high_percent", self.cpu_high_percent)?;
        pct(
            "memory_available_low_percent",
            self.memory_available_low_percent,
        )?;
        pct(
            "memory_available_critical_percent",
            self.memory_available_critical_percent,
        )?;
        pct("battery_low_percent", self.battery_low_percent)?;
        pct("battery_critical_percent", self.battery_critical_percent)?;
        pct(
            "battery_health_warning_percent",
            self.battery_health_warning_percent,
        )?;
        pct("storage_free_low_percent", self.storage_free_low_percent)?;
        pct(
            "storage_free_critical_percent",
            self.storage_free_critical_percent,
        )?;
        pct("disk_busy_high_percent", self.disk_busy_high_percent)?;
        if self.memory_available_critical_percent > self.memory_available_low_percent
            || self.battery_critical_percent > self.battery_low_percent
            || self.storage_free_critical_percent > self.storage_free_low_percent
        {
            return Err(SentinelError::InvalidConfig(
                "critical thresholds must not exceed warning thresholds".into(),
            ));
        }
        let positive = |name: &str, v: Option<f64>, max: f64| match v {
            Some(v) if !(v.is_finite() && v > 0.0 && v <= max) => {
                Err(SentinelError::InvalidConfig(format!(
                    "{name} must be between 0 and {max}, found {v}"
                )))
            }
            _ => Ok(()),
        };
        positive("load_per_cpu_high", Some(self.load_per_cpu_high), 1000.0)?;
        positive(
            "swap_activity_pages_per_s",
            Some(self.swap_activity_pages_per_s),
            1e9,
        )?;
        positive("temperature_warning_c", self.temperature_warning_c, 150.0)?;
        positive(
            "battery_temperature_warning_c",
            self.battery_temperature_warning_c,
            100.0,
        )?;
        positive(
            "battery_discharge_high_w",
            self.battery_discharge_high_w,
            1000.0,
        )?;
        positive("network_high_mbps", self.network_high_mbps, 1e6)?;
        if self.cpu_high_duration_s > 86_400
            || self.sustain_s > 86_400
            || self.cooldown_s > 7 * 86_400
        {
            return Err(SentinelError::InvalidConfig(
                "alert durations are too long".into(),
            ));
        }
        Ok(())
    }
}

/// How long history is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Retention {
    /// 30-second detail.
    pub detailed_hours: u32,
    /// Hourly aggregates.
    pub hourly_days: u32,
    pub alert_days: u32,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            detailed_hours: 24,
            hourly_days: 30,
            alert_days: 90,
        }
    }
}

impl Retention {
    pub fn validate(&self) -> Result<()> {
        if !(1..=24 * 14).contains(&self.detailed_hours)
            || !(1..=366).contains(&self.hourly_days)
            || !(1..=3650).contains(&self.alert_days)
        {
            return Err(SentinelError::InvalidConfig(
                "retention must be 1–336 hours of detail, 1–366 days of hourly data and 1–3650 days of alerts".into(),
            ));
        }
        Ok(())
    }
}

fn default_true() -> bool {
    true
}

impl Default for SentinelConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            monitoring_enabled: false,
            power_mode: PowerMode::default(),
            notifications_enabled: true,
            theme: Theme::default(),
            power_aware_sampling: false,
            alerts: AlertThresholds::default(),
            retention: Retention::default(),
            documents: DocumentSettings::default(),
        }
    }
}

impl SentinelConfig {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONFIG_VERSION {
            return Err(SentinelError::InvalidConfig(format!(
                "configuration version {} is not supported (expected {CONFIG_VERSION})",
                self.version
            )));
        }
        self.alerts.validate()?;
        self.retention.validate()?;
        self.documents.validate()?;
        Ok(())
    }

    /// Apply a partial update, returning the new validated configuration.
    pub fn with_patch(&self, patch: &ConfigPatch) -> Result<SentinelConfig> {
        if patch.is_empty() {
            return Err(SentinelError::InvalidConfig(
                "the update does not change any setting".into(),
            ));
        }
        let mut next = self.clone();
        if let Some(value) = patch.monitoring_enabled {
            next.monitoring_enabled = value;
        }
        if let Some(value) = patch.power_mode {
            next.power_mode = value;
        }
        if let Some(value) = patch.notifications_enabled {
            next.notifications_enabled = value;
        }
        if let Some(value) = patch.theme {
            next.theme = value;
        }
        if let Some(value) = patch.power_aware_sampling {
            next.power_aware_sampling = value;
        }
        if let Some(value) = &patch.alerts {
            next.alerts = value.clone();
        }
        if let Some(value) = patch.retention {
            next.retention = value;
        }
        if let Some(value) = &patch.documents {
            next.documents = (**value).clone();
        }
        next.validate()?;
        Ok(next)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerMode {
    Eco,
    #[default]
    Balanced,
    Performance,
}

impl fmt::Display for PowerMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PowerMode::Eco => "eco",
            PowerMode::Balanced => "balanced",
            PowerMode::Performance => "performance",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Black,
}

/// A partial configuration update. Unknown fields are rejected so a typo in a
/// request is an error rather than a silent no-op.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitoring_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_mode: Option<PowerMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notifications_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_aware_sampling: Option<bool>,
    /// Replaces all alert thresholds (validated as a whole).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alerts: Option<AlertThresholds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention: Option<Retention>,
    /// Replaces all document-discovery settings (validated as a whole).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documents: Option<Box<DocumentSettings>>,
}

impl ConfigPatch {
    pub fn is_empty(&self) -> bool {
        *self == ConfigPatch::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_conservative() {
        let config = SentinelConfig::default();
        assert_eq!(config.version, CONFIG_VERSION);
        assert!(!config.monitoring_enabled, "monitoring must default to off");
        assert_eq!(config.power_mode, PowerMode::Balanced);
        assert!(config.notifications_enabled);
        assert_eq!(config.theme, Theme::Dark);
        config.validate().unwrap();
    }

    #[test]
    fn validation_rejects_unknown_versions() {
        let config = SentinelConfig {
            version: CONFIG_VERSION + 1,
            ..Default::default()
        };
        assert!(matches!(
            config.validate(),
            Err(SentinelError::InvalidConfig(_))
        ));
    }

    #[test]
    fn patch_changes_only_named_fields() {
        let base = SentinelConfig::default();
        let next = base
            .with_patch(&ConfigPatch {
                power_mode: Some(PowerMode::Eco),
                theme: Some(Theme::Black),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(next.power_mode, PowerMode::Eco);
        assert_eq!(next.theme, Theme::Black);
        assert_eq!(next.notifications_enabled, base.notifications_enabled);
        assert_eq!(next.monitoring_enabled, base.monitoring_enabled);
    }

    #[test]
    fn thresholds_and_retention_are_validated() {
        let base = SentinelConfig::default();
        assert!(
            base.alerts.temperature_warning_c.is_none(),
            "no invented temperature limit"
        );
        let bad = |alerts: AlertThresholds| {
            base.with_patch(&ConfigPatch {
                alerts: Some(alerts),
                ..Default::default()
            })
        };
        assert!(
            bad(AlertThresholds {
                cpu_high_percent: 150.0,
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            bad(AlertThresholds {
                battery_critical_percent: 50.0,
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            bad(AlertThresholds {
                temperature_warning_c: Some(-3.0),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            bad(AlertThresholds {
                temperature_warning_c: Some(85.0),
                ..Default::default()
            })
            .is_ok()
        );
        let retention = Retention {
            detailed_hours: 0,
            ..Default::default()
        };
        assert!(
            base.with_patch(&ConfigPatch {
                retention: Some(retention),
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn empty_patch_is_rejected() {
        let err = SentinelConfig::default()
            .with_patch(&ConfigPatch::default())
            .unwrap_err();
        assert!(matches!(err, SentinelError::InvalidConfig(_)));
    }

    #[test]
    fn document_discovery_is_off_by_default_and_validated() {
        let c = SentinelConfig::default();
        assert!(!c.documents.online_discovery);
        assert!(!c.documents.auto_download);
        assert!(c.documents.catalogue_urls.is_empty());
        let ok = |d: DocumentSettings| {
            SentinelConfig {
                documents: d,
                ..SentinelConfig::default()
            }
            .validate()
        };
        assert!(
            ok(DocumentSettings {
                catalogue_urls: vec!["https://docs.example.org/index.json".into()],
                ..Default::default()
            })
            .is_ok()
        );
        for bad in [
            "http://docs.example.org/x.json",
            "https://localhost/x",
            "https://user@docs.example.org/",
            "https://docs.example.org/a b",
            "file:///etc/passwd",
        ] {
            assert!(
                ok(DocumentSettings {
                    catalogue_urls: vec![bad.into()],
                    ..Default::default()
                })
                .is_err(),
                "{bad}"
            );
        }
        assert!(
            ok(DocumentSettings {
                request_timeout_s: 0,
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            ok(DocumentSettings {
                max_download_mb: 0,
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            ok(DocumentSettings {
                catalogue_urls: vec!["https://a.example.org/".into(); 9],
                ..Default::default()
            })
            .is_err()
        );
        // A version-2 file without the section loads with the defaults.
        let old: SentinelConfig =
            toml::from_str("version = 2\nmonitoring_enabled = true\n").unwrap();
        assert_eq!(old.documents, DocumentSettings::default());
    }
}
