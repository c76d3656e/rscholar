use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let providers_dir = Path::new("src/llm/providers");
    println!("cargo:rerun-if-changed={}", providers_dir.display());

    let mut modules: Vec<String> = fs::read_dir(providers_dir)
        .expect("failed to read src/llm/providers")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("rs"))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?.to_string();
            if stem == "mod" || stem == "provider_template" {
                None
            } else {
                Some(stem)
            }
        })
        .collect();

    modules.sort();

    let mut out = String::new();
    for module in &modules {
        out.push_str(&format!(
            "pub(crate) mod {} {{ include!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/llm/providers/{}.rs\")); }}\n",
            module, module
        ));
    }
    out.push_str("\npub(crate) fn register_all(registry: &mut super::ProviderRegistry) {\n");
    for module in &modules {
        out.push_str(&format!("    {}::register(registry);\n", module));
    }
    out.push_str("}\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    fs::write(out_dir.join("llm_providers_gen.rs"), out).expect("failed to write llm_providers_gen.rs");
}
