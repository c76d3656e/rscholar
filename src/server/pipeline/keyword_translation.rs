use crate::llm::{self, LlmRelevanceFilter};
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::ProgressTracker;

fn needs_translation(keyword: &str) -> bool {
    keyword
        .chars()
        .any(|ch| ch as u32 > 0x7F && ch.is_alphabetic())
        || keyword
            .chars()
            .any(|ch| ('\u{4E00}'..='\u{9FFF}').contains(&ch))
}

pub(super) async fn run_keyword_translation(
    tracker: &mut ProgressTracker,
    keyword: &str,
    llm_filter: Option<&Arc<LlmRelevanceFilter>>,
) -> String {
    if keyword.trim().is_empty() {
        return keyword.to_string();
    }

    if !needs_translation(keyword) {
        debug!(keyword = %keyword, "Keyword looks English/ASCII, skipping translation");
        return keyword.to_string();
    }

    let Some(llm) = llm_filter else {
        warn!(keyword = %keyword, "Keyword may need translation but LLM is unavailable; using original keyword");
        return keyword.to_string();
    };
    if llm.providers.is_empty() {
        warn!(keyword = %keyword, "Keyword may need translation but no LLM providers available; using original keyword");
        return keyword.to_string();
    }

    tracker.update("Translating keyword (LLM)", 3).await;
    let req = llm::keyword_translation::KeywordTranslationRequest {
        keyword: keyword.to_string(),
    };

    for provider in &llm.providers {
        match llm::keyword_translation::translate_keyword(provider.as_ref(), &req).await {
            Ok(result) => {
                let translated = result.english_keyword.trim();
                if translated.is_empty() {
                    warn!(
                        original_keyword = %keyword,
                        provider = %provider.name(),
                        "Keyword translation returned empty result; fallback to original keyword"
                    );
                    return keyword.to_string();
                }
                info!(
                    original_keyword = %keyword,
                    translated_keyword = %translated,
                    provider = %provider.name(),
                    "Keyword translation successful"
                );
                return translated.to_string();
            }
            Err(error) => {
                warn!(
                    original_keyword = %keyword,
                    provider = %provider.name(),
                    error = %error,
                    "Keyword translation provider failed, trying fallback"
                );
            }
        }
    }

    warn!(original_keyword = %keyword, "All keyword translation providers failed; using original keyword");
    keyword.to_string()
}

#[cfg(test)]
mod tests {
    use super::needs_translation;

    #[test]
    fn test_needs_translation_false_for_english() {
        println!("[needs-translation] 'machine learning rock strength' -> false");
        println!("[needs-translation] 'deep learning' -> false");
        assert!(!needs_translation("machine learning rock strength"));
        assert!(!needs_translation("deep learning"));
    }

    #[test]
    fn test_needs_translation_true_for_cjk() {
        println!("[needs-translation] '机器学习' -> true");
        println!("[needs-translation] '岩石 强度 预测' -> true");
        assert!(needs_translation("机器学习"));
        assert!(needs_translation("岩石 强度 预测"));
    }

    #[test]
    fn test_needs_translation_true_for_user_examples_batch() {
        let cases = vec![
            "23价多糖疫苗",
            "23价肺炎链球菌",
            "芭蕉根",
            "吹塑机",
            "大语言模型 心理学",
            "低共熔溶剂",
            "肺炎链球菌",
            "分析方法验证",
            "华为",
            "黄芪囊泡",
            "流感疫苗",
            "水痘",
            "乙酰3F唾液酸",
            "智能交通发展",
        ];
        for keyword in cases {
            println!("[needs-translation-batch] '{}' -> true", keyword);
            assert!(needs_translation(keyword));
        }
    }
}
