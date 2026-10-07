use std::path::Path;

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
}

pub fn get_codex_models() -> Vec<ModelInfo> {
    let catalog_path = Path::new("/home/ubuntu/.codex/model-catalogs/gateway.json");
    if let Ok(content) = std::fs::read_to_string(catalog_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(models) = v.get("models").and_then(|m| m.as_array()) {
                let mut list = Vec::new();
                for m in models {
                    if let Some(slug) = m.get("slug").and_then(|s| s.as_str()) {
                        let name = m.get("display_name").and_then(|d| d.as_str()).unwrap_or(slug);
                        let desc = if slug.contains("flash") {
                            format!("{} ({}) [Fast]", name, slug)
                        } else if slug.contains("free") {
                            format!("{} ({}) [Free]", name, slug)
                        } else if slug.contains("opus") || slug.contains("astra") {
                            format!("{} ({}) [Frontier Reasoning]", name, slug)
                        } else {
                            format!("{} ({})", name, slug)
                        };
                        list.push(ModelInfo {
                            id: slug.to_string(),
                            display_name: desc,
                        });
                    }
                }
                if !list.is_empty() {
                    return list;
                }
            }
        }
    }

    vec![
        ModelInfo {
            id: "antigravity/gemini-3.8-flash-high".to_string(),
            display_name: "Gemini 3.8 Flash High (antigravity/gemini-3.8-flash-high) [Fast, 1M Context]".to_string(),
        },
        ModelInfo {
            id: "agentrouter/deepseek-v4-flash".to_string(),
            display_name: "DeepSeek V4 Flash (agentrouter/deepseek-v4-flash) [Fast Code]".to_string(),
        },
        ModelInfo {
            id: "gemini-3.5-flash-lite".to_string(),
            display_name: "Gemini 3.5 Flash Lite (gemini-3.5-flash-lite) [Ultra Fast]".to_string(),
        },
    ]
}

pub fn get_opencode_models() -> Vec<ModelInfo> {
    if let Ok(out) = std::process::Command::new("opencode").arg("models").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut list = Vec::new();
            for line in text.lines() {
                let trimmed = line.trim();
                // Filter out offline bifrost models and empty lines
                if !trimmed.is_empty() && !trimmed.contains(' ') && !trimmed.starts_with("bifrost/") {
                    let desc = if trimmed.starts_with("opencode/") {
                        let short = trimmed.strip_prefix("opencode/").unwrap_or(trimmed);
                        format!("{} ({}) [Built-in Free]", short, trimmed)
                    } else if trimmed.starts_with("cliproxy/") {
                        let short = trimmed.strip_prefix("cliproxy/").unwrap_or(trimmed);
                        if short.contains("flash") {
                            format!("{} ({}) [Fast / CLIProxy]", short, trimmed)
                        } else if short.contains("free") {
                            format!("{} ({}) [Free / CLIProxy]", short, trimmed)
                        } else {
                            format!("{} ({}) [CLIProxy]", short, trimmed)
                        }
                    } else {
                        trimmed.to_string()
                    };
                    list.push(ModelInfo {
                        id: trimmed.to_string(),
                        display_name: desc,
                    });
                }
            }
            if !list.is_empty() {
                return list;
            }
        }
    }

    let config_path = dirs::home_dir().map(|h| h.join(".config/opencode/opencode.json"));
    if let Some(path) = config_path {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(providers) = v.get("providers").and_then(|p| p.as_object()) {
                    let mut list = Vec::new();
                    for (prov_name, prov_val) in providers {
                        if prov_name == "bifrost" {
                            continue;
                        }
                        if let Some(models) = prov_val.get("models").and_then(|m| m.as_object()) {
                            for (model_key, _) in models {
                                let full_id = format!("{}/{}", prov_name, model_key);
                                list.push(ModelInfo {
                                    id: full_id.clone(),
                                    display_name: full_id,
                                });
                            }
                        }
                    }
                    if !list.is_empty() {
                        return list;
                    }
                }
            }
        }
    }

    vec![
        ModelInfo {
            id: "cliproxy/antigravity/gemini-3.8-flash-high".to_string(),
            display_name: "Antigravity Gemini 3.8 Flash High [CLIProxy / Fast]".to_string(),
        },
        ModelInfo {
            id: "opencode/nemotron-3.5-lightning-free".to_string(),
            display_name: "Nemotron 3.5 Lightning Free [Built-in Free]".to_string(),
        },
    ]
}
