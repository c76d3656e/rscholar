//! LLM-based keyword expansion for improved search coverage.
//!
//! Uses LLM to expand a single keyword into related academic terms,
//! enabling broader and more accurate literature searches.

use crate::error::{GscholarError, Result};
use super::{ChatMessage, LlmProvider};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Request for keyword expansion
#[derive(Debug, Clone)]
pub struct KeywordExpansionRequest {
    /// Core search term
    pub keyword: String,
    /// Research context/boundary for this keyword
    pub descript: String,
}

/// Result from keyword expansion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordExpansionResult {
    /// Original keyword
    pub keyword: String,
    /// Expanded keywords (synonyms, abbreviations, related terms)
    pub extended_keywords: Vec<String>,
}

/// Build the prompt for keyword expansion
/// 
/// Uses the user-provided prompt structure for academic keyword expansion.
fn build_expansion_prompt(request: &KeywordExpansionRequest) -> String {
    // System prompt with detailed instructions
    let system_prompt = r#"You are an "Academic Literature Search & Keyword Expansion Assistant." Your goal is: given a user-provided pair {keyword: descript}, generate expanded keywords for Web of Science (WoS) searching. The user will provide ONLY ONE keyword each time.
INPUT (single keyword each time):
{
  "keyword": "<core search term>",
  "descript": "<the research context/boundary for this keyword; use it to constrain expansions and avoid cross-domain ambiguity>"
}
ALLOWED EXPANSION TYPES (ONLY these):

Synonyms / near-synonyms
Abbreviations / full forms / aliases
Related hypernyms (broader concepts)
Related hyponyms (more specific concepts)
STRICT RULES:


Output ONLY the expanded keyword terms themselves. Do NOT output any explanations, summaries, methods, applications, exclusion terms, search queries, rubrics, labels, or categories.
Expansions MUST match the research context implied by "descript". If the keyword is ambiguous across domains, keep ONLY the sense consistent with "descript".
Each expanded term should be a common academic phrasing likely to appear in titles/abstracts. Prefer standard multi-word phrases (2–3 words) when appropriate.
De-duplicate and sort by relevance (most relevant first). Prefer the term forms most likely to appear in title/abstract text.
Output in English by default. If "descript" implies a Chinese context, you may include Chinese/English variants as separate keyword entries, but still only as keyword terms (no explanation).
OUTPUT FORMAT (must follow exactly):


Output JSON ONLY (no markdown, no extra text)
JSON schema:
{
  "keyword": "<original keyword>",
  "extended_keywords": ["<term1>", "<term2>", "..."]
}"#;

    // User message with the actual keyword and description
    let user_message = format!(
        r#"{{"keyword": "{}", "descript": "{}"}}"#,
        request.keyword,
        request.descript
    );

    format!("{}\n\nInput:\n{}", system_prompt, user_message)
}

/// Parse the JSON response from LLM
fn parse_expansion_response(response: &str) -> Result<KeywordExpansionResult> {
    // Try to extract JSON from the response (LLM might add extra text)
    let json_str = extract_json(response);
    
    serde_json::from_str(&json_str).map_err(|e| {
        warn!(response = %response, error = %e, "Failed to parse keyword expansion response");
        GscholarError::Parse(format!(
            "Invalid keyword expansion response: {}. Raw: {}",
            e,
            &response[..response.len().min(200)]
        ))
    })
}

/// Extract JSON object from a string (handles markdown code blocks and extra text)
fn extract_json(text: &str) -> String {
    let text = text.trim();
    
    // Try to find JSON in markdown code block
    if let Some(start) = text.find("```json") {
        if let Some(end) = text[start + 7..].find("```") {
            return text[start + 7..start + 7 + end].trim().to_string();
        }
    }
    
    // Try to find JSON in generic code block
    if let Some(start) = text.find("```") {
        if let Some(end) = text[start + 3..].find("```") {
            let inner = text[start + 3..start + 3 + end].trim();
            // Skip language identifier if present
            if let Some(newline) = inner.find('\n') {
                return inner[newline + 1..].trim().to_string();
            }
            return inner.to_string();
        }
    }
    
    // Try to find raw JSON object
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    
    text.to_string()
}

/// Expand keywords using LLM
/// 
/// # Arguments
/// * `provider` - LLM provider to use for expansion
/// * `request` - Keyword expansion request with keyword and context
/// 
/// # Returns
/// Expanded keywords result, or error if expansion fails
pub async fn expand_keywords(
    provider: &dyn LlmProvider,
    request: &KeywordExpansionRequest,
) -> Result<KeywordExpansionResult> {
    info!(
        keyword = %request.keyword,
        descript = %request.descript,
        provider = %provider.name(),
        "Starting keyword expansion"
    );
    
    let prompt = build_expansion_prompt(request);
    
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: prompt,
    }];
    
    let response = provider.chat_completion(messages).await?;
    
    debug!(
        keyword = %request.keyword,
        response = %response,
        "Received keyword expansion response"
    );
    
    let result = parse_expansion_response(&response)?;
    
    info!(
        keyword = %request.keyword,
        expanded_count = result.extended_keywords.len(),
        expanded = ?result.extended_keywords,
        "Keyword expansion complete"
    );
    
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_response() {
        let response = r#"{"keyword": "LWD", "extended_keywords": ["logging while drilling", "MWD", "wellbore measurement"]}"#;
        let result = parse_expansion_response(response).unwrap();
        
        assert_eq!(result.keyword, "LWD");
        assert_eq!(result.extended_keywords.len(), 3);
        assert!(result.extended_keywords.contains(&"logging while drilling".to_string()));
    }

    #[test]
    fn test_parse_markdown_wrapped_response() {
        let response = r#"```json
{"keyword": "ML", "extended_keywords": ["machine learning", "deep learning"]}
```"#;
        let result = parse_expansion_response(response).unwrap();
        
        assert_eq!(result.keyword, "ML");
        assert_eq!(result.extended_keywords.len(), 2);
    }

    #[test]
    fn test_parse_response_with_extra_text() {
        let response = r#"Here is the expansion:
{"keyword": "AI", "extended_keywords": ["artificial intelligence", "neural network"]}
Hope this helps!"#;
        let result = parse_expansion_response(response).unwrap();
        
        assert_eq!(result.keyword, "AI");
        assert_eq!(result.extended_keywords.len(), 2);
    }

    #[test]
    fn test_build_expansion_prompt() {
        let request = KeywordExpansionRequest {
            keyword: "LWD".to_string(),
            descript: "Logging While Drilling for oil and gas".to_string(),
        };
        let prompt = build_expansion_prompt(&request);
        
        assert!(prompt.contains("LWD"));
        assert!(prompt.contains("Logging While Drilling"));
        assert!(prompt.contains("extended_keywords"));
        assert!(prompt.contains("JSON ONLY"));
    }

    #[test]
    fn test_extract_json() {
        // Test raw JSON
        let json = extract_json(r#"{"key": "value"}"#);
        assert_eq!(json, r#"{"key": "value"}"#);
        
        // Test with markdown
        let json = extract_json("```json\n{\"key\": \"value\"}\n```");
        assert_eq!(json, "{\"key\": \"value\"}");
        
        // Test with extra text
        let json = extract_json("Result: {\"key\": \"value\"} end");
        assert_eq!(json, "{\"key\": \"value\"}");
    }
}
