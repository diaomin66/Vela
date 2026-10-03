//! Model capabilities advertised to the native model picker.
//!
//! API capabilities are not the same as a client's curated model picker.
//! The registry records exact IDs, model-specific API defaults, and source URLs.
//! A separate native default keeps the picker on a supported value when the
//! API does not publish its own default. See docs/model-capabilities.md.
use crate::core::ChannelModel;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;

pub const EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub efforts: Vec<String>,
    pub default: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficialModel {
    ids: Vec<String>,
    efforts: Option<Vec<String>>,
    api_default: Option<String>,
    native_default: Option<String>,
}

fn registry() -> &'static [OfficialModel] {
    #[derive(Deserialize)]
    struct Registry {
        models: Vec<OfficialModel>,
    }
    static MODELS: OnceLock<Vec<OfficialModel>> = OnceLock::new();
    MODELS.get_or_init(|| {
        serde_json::from_str::<Registry>(include_str!("../resources/model-capabilities.json"))
            .expect("the built-in official model registry must be valid")
            .models
    })
}

fn known_model(id: &str) -> Option<&'static OfficialModel> {
    registry()
        .iter()
        .find(|entry| entry.ids.iter().any(|known| known == id))
}

pub fn capabilities(model: &ChannelModel) -> Capabilities {
    let known = known_model(&model.id);
    let efforts = match &model.reasoning_efforts {
        Some(selected) => EFFORTS
            .iter()
            .filter(|effort| selected.iter().any(|selected| selected == **effort))
            .map(|effort| (*effort).to_owned())
            .collect::<Vec<_>>(),
        None => known
            .and_then(|entry| entry.efforts.clone())
            .unwrap_or_default(),
    };
    let default = model
        .default_reasoning_effort
        .as_deref()
        .filter(|value| efforts.iter().any(|effort| effort == value))
        .or_else(|| {
            known
                .and_then(|entry| entry.api_default.as_deref())
                .filter(|value| efforts.iter().any(|effort| effort == value))
        })
        .or_else(|| {
            known
                .and_then(|entry| entry.native_default.as_deref())
                .filter(|value| efforts.iter().any(|effort| effort == value))
        })
        // Native model/list treats a null default as `none`. For models which
        // do not support none, choose a clearly documented Vela compatibility
        // default; do not label this as a verified API server default.
        .or_else(|| {
            efforts
                .iter()
                .find(|effort| effort.as_str() == "medium")
                .map(String::as_str)
        })
        .or_else(|| efforts.first().map(String::as_str))
        .map(str::to_owned);
    Capabilities { efforts, default }
}

/// Called at the persistence boundary, before a changed profile is saved.
pub fn normalize_model(model: &mut ChannelModel) -> Result<(), String> {
    if let Some(selected) = &mut model.reasoning_efforts {
        if selected.len() > 32 {
            return Err("推理档位数量无效。".into());
        }
        for effort in selected.iter_mut() {
            *effort = effort.trim().to_ascii_lowercase();
            if !EFFORTS.contains(&effort.as_str()) {
                return Err(
                    "推理档位无效，请使用 none、minimal、low、medium、high、xhigh 或 max。".into(),
                );
            }
        }
        selected.sort_by_key(|effort| EFFORTS.iter().position(|known| *known == effort).unwrap());
        selected.dedup();
    }
    if let Some(default) = &mut model.default_reasoning_effort {
        *default = default.trim().to_ascii_lowercase();
        if !EFFORTS.contains(&default.as_str()) {
            return Err("默认推理档位无效。".into());
        }
    }
    if model
        .default_reasoning_effort
        .as_ref()
        .is_some_and(|default| !capabilities(model).efforts.contains(default))
    {
        return Err("默认推理档位必须在该模型启用的档位中。".into());
    }
    Ok(())
}

pub fn presets(efforts: &[String]) -> Vec<Value> {
    efforts
        .iter()
        .map(|effort| {
            let description = match effort.as_str() {
                "none" => "不使用额外推理",
                "minimal" => "最少推理，优先响应速度",
                "low" => "较快响应，适合简单任务",
                "medium" => "平衡推理深度与响应速度",
                "high" => "深入推理，适合复杂任务",
                "xhigh" => "超高推理强度，处理复杂问题",
                "max" => "最大推理投入，适合最困难的任务",
                _ => "",
            };
            json!({"effort": effort, "description": description})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> ChannelModel {
        serde_json::from_value(json!({"id":id,"enabled":true})).unwrap()
    }

    #[test]
    fn api_model_pages_take_precedence_over_curated_client_picker_defaults() {
        for id in ["gpt-5.2", "gpt-5.4", "gpt-5.4-mini", "gpt-5.4-nano"] {
            let actual = capabilities(&model(id));
            assert_eq!(actual.efforts, ["none", "low", "medium", "high", "xhigh"]);
            assert_eq!(actual.default.as_deref(), Some("none"));
        }
        assert_eq!(
            capabilities(&model("gpt-5.5")).default.as_deref(),
            Some("medium")
        );
        assert_eq!(
            capabilities(&model("gpt-5.1")).efforts,
            ["none", "low", "medium", "high"]
        );
        assert_eq!(
            capabilities(&model("gpt-5.1")).default.as_deref(),
            Some("none")
        );
    }

    #[test]
    fn original_gpt5_and_each_pro_variant_keep_their_own_restrictions() {
        for id in ["gpt-5", "gpt-5-mini", "gpt-5-nano", "gpt-5-mini-2025-08-07"] {
            assert_eq!(
                capabilities(&model(id)).efforts,
                ["minimal", "low", "medium", "high"]
            );
            assert_eq!(capabilities(&model(id)).default.as_deref(), Some("medium"));
        }
        assert_eq!(capabilities(&model("gpt-5-pro")).efforts, ["high"]);
        assert_eq!(
            capabilities(&model("gpt-5-pro")).default.as_deref(),
            Some("high")
        );
        assert_eq!(
            capabilities(&model("gpt-5.2-pro")).efforts,
            ["medium", "high", "xhigh"]
        );
        assert_eq!(known_model("gpt-5.2-pro").unwrap().api_default, None);
        assert_eq!(
            capabilities(&model("gpt-5.4-pro")).default.as_deref(),
            Some("medium")
        );
        assert_eq!(
            capabilities(&model("gpt-5.5-pro")).default.as_deref(),
            Some("high")
        );
    }

    #[test]
    fn newest_api_models_keep_max_and_do_not_share_an_invented_family_policy() {
        for id in [
            "gpt-5.6",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-6-sol",
            "gpt-6-luna",
        ] {
            assert_eq!(
                capabilities(&model(id)).efforts,
                ["none", "low", "medium", "high", "xhigh", "max"]
            );
            assert_eq!(capabilities(&model(id)).default.as_deref(), Some("medium"));
        }
        for id in ["gpt-6-astra", "gpt-6.1-sol"] {
            assert_eq!(
                capabilities(&model(id)).efforts,
                ["low", "medium", "high", "xhigh", "max"]
            );
        }
        assert_eq!(
            capabilities(&model("gpt-6.1-sol")).default.as_deref(),
            Some("medium")
        );
        assert_eq!(known_model("gpt-6-astra").unwrap().api_default, None);
        assert_eq!(
            capabilities(&model("gpt-6-astra")).default.as_deref(),
            Some("low")
        );
        for id in [
            "gpt-6",
            "gpt-6.1",
            "gpt-6-pro",
            "gpt-5.6-pro",
            "gpt-6.1-luna",
            "gpt-6.1-sol-2026-10-01",
        ] {
            assert!(
                capabilities(&model(id)).efforts.is_empty(),
                "Do not invent alias or snapshot {id}"
            );
        }
    }

    #[test]
    fn codex_mini_and_max_are_distinct_models_and_not_suffix_guesses() {
        assert_eq!(
            capabilities(&model("gpt-5.1-codex-mini")).efforts,
            ["medium", "high"]
        );
        assert_eq!(
            capabilities(&model("gpt-5.1-codex")).efforts,
            ["low", "medium", "high"]
        );
        assert_eq!(
            capabilities(&model("gpt-5.1-codex-max")).efforts,
            ["low", "medium", "high", "xhigh"]
        );
        assert_eq!(
            capabilities(&model("gpt-5.2-codex")).efforts,
            ["low", "medium", "high", "xhigh"]
        );
        assert!(known_model("gpt-5.2-codex").unwrap().api_default.is_none());
        assert!(capabilities(&model("gpt-5.1-codex-max-custom"))
            .efforts
            .is_empty());
    }

    #[test]
    fn exact_snapshots_match_only_the_published_ids() {
        assert_eq!(
            capabilities(&model("gpt-5.4-2026-03-05")),
            capabilities(&model("gpt-5.4"))
        );
        assert_eq!(
            capabilities(&model("o3-2025-04-16")).efforts,
            ["low", "medium", "high"]
        );
        assert!(capabilities(&model("gpt-5.4-2026-01-01"))
            .efforts
            .is_empty());
        assert!(
            capabilities(&model("o3-pro")).efforts.is_empty(),
            "An undocumented variant cannot inherit base-model reasoning support"
        );
    }

    #[test]
    fn registry_has_unique_ids_valid_defaults_and_explicit_sources() {
        let raw: Value =
            serde_json::from_str(include_str!("../resources/model-capabilities.json")).unwrap();
        let mut ids = std::collections::HashSet::new();
        assert_eq!(raw["checkedAt"], "2026-10-03");
        for (entry, documented) in registry().iter().zip(raw["models"].as_array().unwrap()) {
            for id in &entry.ids {
                assert!(ids.insert(id), "Duplicate ID {id}");
                let native = capabilities(&model(id));
                assert!(native
                    .default
                    .as_ref()
                    .is_none_or(|default| native.efforts.contains(default)));
            }
            if let Some(efforts) = &entry.efforts {
                assert!(efforts
                    .iter()
                    .all(|effort| EFFORTS.contains(&effort.as_str())));
                assert!(entry
                    .api_default
                    .as_ref()
                    .is_none_or(|default| efforts.contains(default)));
                assert!(entry
                    .native_default
                    .as_ref()
                    .is_none_or(|default| efforts.contains(default)));
            }
            assert!(!documented["sources"].as_array().unwrap().is_empty());
        }
        assert!(ids.len() >= 80);
        assert!(!EFFORTS.contains(&"ultra"));
    }

    #[test]
    fn max_is_a_valid_wire_effort_while_ultra_is_not_an_api_effort() {
        let mut model = model("custom");
        model.reasoning_efforts = Some(vec!["max".into(), "high".into()]);
        model.default_reasoning_effort = Some("max".into());
        normalize_model(&mut model).unwrap();
        assert_eq!(capabilities(&model).efforts, ["high", "max"]);
        assert_eq!(capabilities(&model).default.as_deref(), Some("max"));
        assert_eq!(presets(&["max".into()])[0]["effort"], "max");
    }

    #[test]
    fn unknown_aliases_are_not_assigned_unverified_capabilities() {
        for id in [
            "custom",
            "gpt-5.4-fast",
            "gpt-5.4-mini-custom",
            "vendor/gpt-5.4",
        ] {
            let actual = capabilities(&model(id));
            assert!(actual.efforts.is_empty());
            assert!(actual.default.is_none());
        }
    }

    #[test]
    fn manual_capabilities_and_default_are_normalized_and_respected() {
        let mut model = model("custom");
        model.reasoning_efforts = Some(vec![" HIGH ".into(), "low".into(), "high".into()]);
        model.default_reasoning_effort = Some(" HIGH ".into());
        normalize_model(&mut model).unwrap();
        assert_eq!(
            capabilities(&model),
            Capabilities {
                efforts: vec!["low".into(), "high".into()],
                default: Some("high".into())
            }
        );
    }

    #[test]
    fn explicit_off_clears_native_picker_and_invalid_defaults_are_rejected() {
        let mut model = model("gpt-5.4");
        model.reasoning_efforts = Some(vec![]);
        assert!(capabilities(&model).efforts.is_empty());
        assert!(capabilities(&model).default.is_none());
        model.default_reasoning_effort = Some("high".into());
        assert!(normalize_model(&mut model).is_err());
        model.default_reasoning_effort = None;
        model.reasoning_efforts = Some(vec!["ultra".into()]);
        assert!(normalize_model(&mut model).is_err());
    }
}
