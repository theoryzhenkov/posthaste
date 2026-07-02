use super::*;

/// Current cache budget and pressure state for admission decisions.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheBudget {
    pub used_bytes: u64,
    pub soft_cap_bytes: u64,
    pub hard_cap_bytes: u64,
    pub interactive_pressure: f64,
}

impl CacheBudget {
    /// Soft cap plus a bounded fraction of the burst space toward the hard cap.
    pub fn effective_target_bytes(self) -> u64 {
        let hard = self.hard_cap_bytes.max(self.soft_cap_bytes);
        let pressure = clamp_unit(self.interactive_pressure);
        let burst_range = hard.saturating_sub(self.soft_cap_bytes) as f64;
        self.soft_cap_bytes + (burst_range * pressure).round() as u64
    }
}

pub fn clamp_unit(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
