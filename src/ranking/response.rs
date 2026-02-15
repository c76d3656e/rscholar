use super::types::RankingMetrics;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::debug;

#[derive(Debug, Deserialize)]
pub(crate) struct EasyScholarResponse {
    pub code: i32,
    #[serde(default)]
    pub msg: Option<String>,
    #[serde(default)]
    pub data: Option<EasyScholarData>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EasyScholarData {
    #[serde(rename = "officialRank", default)]
    official_rank: Option<OfficialRank>,
}

#[derive(Debug, Deserialize)]
struct OfficialRank {
    #[serde(default)]
    select: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    all: Option<HashMap<String, serde_json::Value>>,
}

/// Extract metrics from EasyScholar response data
pub(crate) fn extract_metrics(data: &EasyScholarData) -> RankingMetrics {
    let mut metrics = RankingMetrics::default();

    if let Some(ref official) = data.official_rank {
        let select = official.select.as_ref();
        let all = official.all.as_ref();

        metrics.sciif = get_value(select, all, "sciif");
        metrics.jci = get_value(select, all, "jci");
        metrics.sci = get_value(select, all, "sci");
        metrics.sci_up_top = get_value(select, all, "sciUpTop");
        metrics.sci_base = get_value(select, all, "sciBase");
        metrics.sci_up = get_value(select, all, "sciUp");
    }

    debug!(
        has_sciif = metrics.sciif.is_some(),
        has_jci = metrics.jci.is_some(),
        has_sci = metrics.sci.is_some(),
        "Extracted EasyScholar metrics from response payload"
    );

    metrics
}

fn get_value(
    select: Option<&HashMap<String, serde_json::Value>>,
    all: Option<&HashMap<String, serde_json::Value>>,
    key: &str,
) -> Option<String> {
    if let Some(map) = select {
        if let Some(val) = map.get(key) {
            return value_to_string(val);
        }
    }
    if let Some(map) = all {
        if let Some(val) = map.get(key) {
            return value_to_string(val);
        }
    }
    None
}

fn value_to_string(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Null => None,
        _ => Some(val.to_string()),
    }
}
