use rscholar::llm::{ChatMessage, LlmRelevanceFilter};
use rscholar::server::config::{LlmSection, ProviderConfig, ServerConfig};
use std::collections::HashMap;

fn load_config() -> ServerConfig {
    ServerConfig::load_from_file("config.toml")
        .expect("failed to load config.toml for llm live test")
}

fn build_single_provider_section(name: &str, cfg: ProviderConfig) -> LlmSection {
    let mut registry = HashMap::new();
    registry.insert(name.to_string(), cfg);

    LlmSection {
        default_provider: name.to_string(),
        enable_filter: true,
        strict_filter: false,
        providers: vec![name.to_string()],
        provider_configs: registry,
    }
}

fn is_poem_response_acceptable(text: &str) -> bool {
    let normalized = text.trim();
    if normalized.is_empty() {
        return false;
    }

    let lower = normalized.to_lowercase();
    let has_summer_signal = normalized.contains("夏") || lower.contains("summer");
    let has_poem_shape = normalized.lines().count() >= 2 || normalized.chars().count() >= 16;

    has_summer_signal && has_poem_shape
}

#[test]
fn test_llm_registry_has_resolvable_provider_configs() {
    let cfg = load_config();
    assert!(
        cfg.llm.enable_filter,
        "llm.enable_filter should be true for live provider checks"
    );

    let order = cfg.llm.provider_order();
    assert!(
        !order.is_empty(),
        "llm.providers (or registry) should not be empty"
    );

    for provider_name in order {
        assert!(
            cfg.llm.resolve_provider_config(&provider_name).is_some(),
            "provider '{}' is in order but has no registry config",
            provider_name
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires network and valid API keys in config.toml"]
async fn test_llm_live_poem_response_for_each_registered_provider() {
    let cfg = load_config();
    let order = cfg.llm.provider_order();
    assert!(
        !order.is_empty(),
        "no provider found in llm config for live checks"
    );

    let prompt = "写一首关于夏天的诗";
    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: prompt.to_string(),
    }];

    println!("registered providers: {:?}", order);
    let mut failures: Vec<String> = Vec::new();

    for provider_name in order {
        let Some(provider_cfg) = cfg.llm.resolve_provider_config(&provider_name) else {
            let msg = format!("provider '{}' missing registry config", provider_name);
            println!("[{}] {}", provider_name, msg);
            failures.push(msg);
            continue;
        };

        let single = build_single_provider_section(&provider_name, provider_cfg);
        let filter = match LlmRelevanceFilter::build_from_config(&single) {
            Ok(Some(v)) => v,
            Ok(None) => {
                let msg = format!("provider '{}' initialization returned None", provider_name);
                println!("[{}] {}", provider_name, msg);
                failures.push(msg);
                continue;
            }
            Err(e) => {
                let msg = format!("provider '{}' build_from_config failed: {}", provider_name, e);
                println!("[{}] {}", provider_name, msg);
                failures.push(msg);
                continue;
            }
        };

        let Some(provider) = filter.providers.first().cloned() else {
            let msg = format!("provider '{}' not built", provider_name);
            println!("[{}] {}", provider_name, msg);
            failures.push(msg);
            continue;
        };

        match provider.chat_completion(messages.clone()).await {
            Ok(answer) => {
                println!("[{}] response: {}", provider_name, answer);
                if !is_poem_response_acceptable(&answer) {
                    failures.push(format!(
                        "provider '{}' returned unexpected poem response: {}",
                        provider_name, answer
                    ));
                }
            }
            Err(e) => {
                let msg = format!("provider '{}' request failed: {}", provider_name, e);
                println!("[{}] {}", provider_name, msg);
                failures.push(msg);
            }
        }
    }

    assert!(
        failures.is_empty(),
        "LLM live test failed:\n{}",
        failures.join("\n")
    );
}
