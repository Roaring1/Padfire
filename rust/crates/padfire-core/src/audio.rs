//! Output-device identity and fallback resolution shared across platforms.
//!
//! A sink name from PipeWire is not assumed to be a Windows endpoint ID (or
//! vice versa). Adapters enumerate stable IDs and display names, then use this
//! resolver so a missing saved endpoint is surfaced instead of being silently
//! treated as the requested device.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    /// Stable identifier from the native audio backend.
    pub id: String,
    /// User-visible name, optionally including the host API.
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputResolution {
    Default {
        device: Option<OutputDevice>,
    },
    Matched {
        requested: String,
        device: OutputDevice,
    },
    Missing {
        requested: String,
        fallback: Option<OutputDevice>,
    },
}

impl OutputResolution {
    pub fn device(&self) -> Option<&OutputDevice> {
        match self {
            Self::Default { device } => device.as_ref(),
            Self::Matched { device, .. } => Some(device),
            Self::Missing { fallback, .. } => fallback.as_ref(),
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing { .. })
    }

    pub fn used_default_fallback(&self) -> bool {
        matches!(
            self,
            Self::Missing {
                fallback: Some(_),
                ..
            }
        )
    }

    pub fn requested_name(&self) -> Option<&str> {
        match self {
            Self::Matched { requested, .. } | Self::Missing { requested, .. } => Some(requested),
            Self::Default { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRoutePlan {
    pub primary: OutputResolution,
    pub monitor: Option<OutputResolution>,
    pub separate_monitor_stream: bool,
}

fn default_device(devices: &[OutputDevice]) -> Option<OutputDevice> {
    devices.iter().find(|device| device.is_default).cloned()
}

/// Resolve an optional native device name without mutating the saved value.
pub fn resolve_output(requested: &str, devices: &[OutputDevice]) -> OutputResolution {
    let default = default_device(devices);
    if requested.trim().is_empty() {
        return OutputResolution::Default { device: default };
    }

    let wanted = requested.trim();
    if let Some(device) = devices.iter().find(|device| {
        device.id.eq_ignore_ascii_case(wanted) || device.name.eq_ignore_ascii_case(wanted)
    }) {
        return OutputResolution::Matched {
            requested: wanted.to_owned(),
            device: device.clone(),
        };
    }

    OutputResolution::Missing {
        requested: wanted.to_owned(),
        fallback: default,
    }
}

/// Resolve primary and optional monitor routes. A second stream is necessary
/// only when both outputs resolved and their native device IDs differ.
pub fn resolve_route_pair(
    primary_name: &str,
    monitor_enabled: bool,
    monitor_name: &str,
    devices: &[OutputDevice],
) -> AudioRoutePlan {
    let primary = resolve_output(primary_name, devices);
    let monitor = monitor_enabled.then(|| resolve_output(monitor_name, devices));
    let separate_monitor_stream = match (&primary, &monitor) {
        (primary_resolution, Some(monitor_resolution)) => {
            match (primary_resolution.device(), monitor_resolution.device()) {
                (Some(primary_device), Some(monitor_device)) => {
                    primary_device.id != monitor_device.id
                }
                _ => false,
            }
        }
        _ => false,
    };

    AudioRoutePlan {
        primary,
        monitor,
        separate_monitor_stream,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices() -> Vec<OutputDevice> {
        vec![
            OutputDevice {
                id: "pipewire:alsa_output.pci-1".to_owned(),
                name: "Speakers".to_owned(),
                is_default: true,
            },
            OutputDevice {
                id: "pipewire:alsa_output.usb-2".to_owned(),
                name: "USB Headset".to_owned(),
                is_default: false,
            },
        ]
    }

    #[test]
    fn blank_name_resolves_to_the_system_default_device() {
        let resolved = resolve_output("", &devices());
        assert_eq!(
            resolved,
            OutputResolution::Default {
                device: Some(devices()[0].clone())
            }
        );
    }

    #[test]
    fn exact_id_and_display_name_are_both_supported() {
        assert!(matches!(
            resolve_output("pipewire:alsa_output.usb-2", &devices()),
            OutputResolution::Matched { .. }
        ));
        assert!(matches!(
            resolve_output("usb headset", &devices()),
            OutputResolution::Matched { .. }
        ));
    }

    #[test]
    fn missing_cross_platform_name_is_preserved_and_default_fallback_is_visible() {
        let resolved = resolve_output("vm_music", &devices());
        assert_eq!(
            resolved,
            OutputResolution::Missing {
                requested: "vm_music".to_owned(),
                fallback: Some(devices()[0].clone())
            }
        );
        assert!(resolved.is_missing());
        assert!(resolved.used_default_fallback());
        assert_eq!(resolved.requested_name(), Some("vm_music"));
    }

    #[test]
    fn missing_name_without_default_is_not_reported_as_a_valid_route() {
        let mut only_non_default = devices();
        only_non_default[0].is_default = false;
        let resolved = resolve_output("removed-device", &only_non_default);
        assert!(resolved.is_missing());
        assert!(resolved.device().is_none());
        assert!(!resolved.used_default_fallback());
    }

    #[test]
    fn monitor_on_the_same_resolved_device_does_not_open_a_duplicate_stream() {
        let plan = resolve_route_pair("Speakers", true, "", &devices());
        assert!(!plan.separate_monitor_stream);
    }

    #[test]
    fn distinct_monitor_device_gets_a_second_stream() {
        let plan = resolve_route_pair("Speakers", true, "USB Headset", &devices());
        assert!(plan.separate_monitor_stream);
        assert!(matches!(plan.primary, OutputResolution::Matched { .. }));
        assert!(matches!(
            plan.monitor,
            Some(OutputResolution::Matched { .. })
        ));
    }

    #[test]
    fn disabled_monitor_does_not_resolve_or_open_a_second_stream() {
        let plan = resolve_route_pair("Speakers", false, "USB Headset", &devices());
        assert!(plan.monitor.is_none());
        assert!(!plan.separate_monitor_stream);
    }

    #[test]
    fn unresolved_monitor_falls_back_to_primary_if_system_default_matches() {
        let plan = resolve_route_pair("Speakers", true, "old-linux-sink", &devices());
        assert!(matches!(
            plan.monitor,
            Some(OutputResolution::Missing { .. })
        ));
        assert!(!plan.separate_monitor_stream);
    }
}
