use serde::{Deserialize, Serialize};

/// Ranking metrics from EasyScholar
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RankingMetrics {
    /// Impact Factor
    pub sciif: Option<String>,
    /// Journal Citation Indicator
    pub jci: Option<String>,
    /// SCI partition (Q1, Q2, etc.)
    pub sci: Option<String>,
    /// SCI Up Top
    pub sci_up_top: Option<String>,
    /// SCI Base
    pub sci_base: Option<String>,
    /// SCI Up
    pub sci_up: Option<String>,
}
