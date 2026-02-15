use crate::error::{GscholarError, Result};
use serde::Serialize;
use tracing::info;

use super::PaperResult;

#[derive(Debug, Serialize)]
struct PaperResultCsv {
    title: String,
    authors: String,
    year: String,
    venue: String,
    doi: String,
    url: String,
    pdf_url: String,
    snippet: String,
    abstract_text: String,
    if_score: String,
    jci_score: String,
    sci_partition: String,
}

impl From<&PaperResult> for PaperResultCsv {
    fn from(p: &PaperResult) -> Self {
        Self {
            title: p.title.clone(),
            authors: p.authors.clone(),
            year: p.year.clone(),
            venue: p.venue.clone(),
            doi: p.doi.clone(),
            url: p.url.clone(),
            pdf_url: p.pdf_url.clone(),
            snippet: p.snippet.clone(),
            abstract_text: p.abstract_text.clone(),
            if_score: p.if_score.clone().unwrap_or_default(),
            jci_score: p.jci_score.clone().unwrap_or_default(),
            sci_partition: p.sci_partition.clone().unwrap_or_default(),
        }
    }
}

pub(super) fn save_results_csv(path: &std::path::Path, data: &[PaperResult]) -> Result<()> {
    let mut wtr = csv::Writer::from_path(path)
        .map_err(|e| GscholarError::Io(std::io::Error::other(e.to_string())))?;

    for item in data {
        let csv_row: PaperResultCsv = item.into();
        wtr.serialize(&csv_row)
            .map_err(|e| GscholarError::Io(std::io::Error::other(e.to_string())))?;
    }

    wtr.flush()
        .map_err(|e| GscholarError::Io(std::io::Error::other(e.to_string())))?;

    info!(path = %path.display(), rows = data.len(), "CSV saved successfully");
    Ok(())
}
