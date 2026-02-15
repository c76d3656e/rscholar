//! LLM-based keyword translation for search normalization.
//!
//! Translates user keyword into concise academic English query terms.

use crate::error::{GscholarError, Result};
use super::{ChatMessage, LlmProvider};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct KeywordTranslationRequest {
    pub keyword: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordTranslationResult {
    pub original_keyword: String,
    pub english_keyword: String,
}

fn build_translation_prompt(request: &KeywordTranslationRequest) -> String {
    format!(
        r#"You are an academic search query translator.
Translate the user keyword into concise, natural English for literature search.

Rules:
1) Output JSON ONLY.
2) Keep domain terms/acronyms (e.g., CNN, SVM, XGBoost, LSTM, DOI) unchanged.
3) Do not add explanations, just the search phrase.
4) Prefer 2-8 words phrase suitable for title/abstract search.
5) If input is already English, return it unchanged.

Input JSON:
{{"keyword":"{}"}}

Output JSON schema:
{{"original_keyword":"...","english_keyword":"..."}}"#,
        request.keyword
    )
}

fn extract_json(text: &str) -> String {
    let text = text.trim();
    if let Some(start) = text.find("```json") {
        if let Some(end) = text[start + 7..].find("```") {
            return text[start + 7..start + 7 + end].trim().to_string();
        }
    }
    if let Some(start) = text.find("```") {
        if let Some(end) = text[start + 3..].find("```") {
            let inner = text[start + 3..start + 3 + end].trim();
            if let Some(newline) = inner.find('\n') {
                return inner[newline + 1..].trim().to_string();
            }
            return inner.to_string();
        }
    }
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            return text[start..=end].to_string();
        }
    }
    text.to_string()
}

fn parse_translation_response(response: &str) -> Result<KeywordTranslationResult> {
    let json_str = extract_json(response);
    serde_json::from_str(&json_str).map_err(|e| {
        warn!(response = %response, error = %e, "Failed to parse keyword translation response");
        GscholarError::Parse(format!(
            "Invalid keyword translation response: {}. Raw: {}",
            e,
            &response[..response.len().min(200)]
        ))
    })
}

pub async fn translate_keyword(
    provider: &dyn LlmProvider,
    request: &KeywordTranslationRequest,
) -> Result<KeywordTranslationResult> {
    info!(
        keyword = %request.keyword,
        provider = %provider.name(),
        "Starting keyword translation"
    );

    let prompt = build_translation_prompt(request);
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: prompt,
    }];
    let response = provider.chat_completion(messages).await?;
    debug!(
        keyword = %request.keyword,
        response = %response,
        "Received keyword translation response"
    );
    let result = parse_translation_response(&response)?;
    info!(
        original_keyword = %result.original_keyword,
        english_keyword = %result.english_keyword,
        "Keyword translation complete"
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_translation_response() {
        let response = r#"{"original_keyword":"机器学习","english_keyword":"machine learning"}"#;
        let result = parse_translation_response(response).unwrap();
        println!(
            "[translation-parse] original='{}' -> english='{}'",
            result.original_keyword, result.english_keyword
        );
        assert_eq!(result.original_keyword, "机器学习");
        assert_eq!(result.english_keyword, "machine learning");
    }

    #[test]
    fn test_parse_translation_response_with_rock_example() {
        let response = r#"{"original_keyword":"机器学习 岩石 强度 预测","english_keyword":"machine learning rock strength prediction"}"#;
        let result = parse_translation_response(response).unwrap();
        println!(
            "[translation-parse] original='{}' -> english='{}'",
            result.original_keyword, result.english_keyword
        );
        assert_eq!(result.original_keyword, "机器学习 岩石 强度 预测");
        assert_eq!(
            result.english_keyword,
            "machine learning rock strength prediction"
        );
    }

    #[test]
    fn test_parse_translation_response_user_examples_batch() {
        let cases = vec![
            ("23价多糖疫苗", "23-valent polysaccharide vaccine"),
            ("23价肺炎链球菌", "23-valent pneumococcal"),
            ("芭蕉根", "banana root"),
            ("吹塑机", "blow molding machine"),
            ("大语言模型 心理学", "large language models psychology"),
            ("低共熔溶剂", "deep eutectic solvent"),
            ("肺炎链球菌", "Streptococcus pneumoniae"),
            ("分析方法验证", "analytical method validation"),
            ("华为", "Huawei"),
            ("黄芪囊泡", "astragalus vesicles"),
            ("流感疫苗", "influenza vaccine"),
            ("水痘", "varicella"),
            ("乙酰3F唾液酸", "N-acetyl-3F-sialic acid"),
            ("智能交通发展", "intelligent transportation development"),
        ];

        for (original, english) in cases {
            let response = format!(
                r#"{{"original_keyword":"{}","english_keyword":"{}"}}"#,
                original, english
            );
            let result = parse_translation_response(&response).unwrap();
            println!(
                "[translation-batch] original='{}' -> english='{}'",
                result.original_keyword, result.english_keyword
            );
            assert_eq!(result.original_keyword, original);
            assert_eq!(result.english_keyword, english);
        }
    }
}
