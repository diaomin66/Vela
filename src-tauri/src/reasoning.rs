//! Model capabilities advertised to the native model picker.
//!
//! API capabilities are not the same as a client's curated model picker.
//! The registry records exact IDs, model-specific API defaults, and source URLs.
//! A separate native default keeps the picker on a supported value when the
//! API does not publish its own default. See docs/model-capabilities.md.
use crate::core::{ChannelModel, NativeReasoning};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;

pub const API_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];
pub const EFFORTS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub efforts: Vec<String>,
    pub default: Option<String>,
    pub native: Option<NativeReasoning>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficialModel {
    ids: Vec<String>,
    efforts: Option<Vec<String>>,
    api_default: Option<String>,
    native_default: Option<String>,
    native_efforts: Option<Vec<String>>,
    native_ultra: Option<NativeReasoning>,
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
    let mut efforts = match &model.reasoning_efforts {
        Some(selected) => EFFORTS
            .iter()
            .filter(|effort| selected.iter().any(|selected| selected == **effort))
            .map(|effort| (*effort).to_owned())
            .collect::<Vec<_>>(),
        None => known
            .and_then(|entry| {
                entry
                    .efforts
                    .as_ref()
                    .or(entry.native_efforts.as_ref())
                    .cloned()
            })
            .unwrap_or_default(),
    };
    // Automatic Ultra is an exact official client capability. An explicit user
    // list, including an empty/off list, must never gain options silently.
    if model.reasoning_efforts.is_none() && known.is_some_and(|entry| entry.native_ultra.is_some())
    {
        efforts.push("ultra".into());
    }
    let native = native_metadata(model, &efforts);
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
    Capabilities {
        efforts,
        default,
        native,
    }
}

fn native_metadata(model: &ChannelModel, efforts: &[String]) -> Option<NativeReasoning> {
    if !efforts.iter().any(|value| value == "ultra") {
        return None;
    }
    if let Some(explicit) = &model.native_reasoning {
        return Some(explicit.clone());
    }
    let official = known_model(&model.id).and_then(|entry| entry.native_ultra.as_ref());
    let target = official
        .map(|metadata| metadata.ultra_effort.as_str())
        .filter(|value| efforts.iter().any(|effort| effort == value))
        .or_else(|| {
            ["max", "xhigh", "high", "medium", "low"]
                .into_iter()
                .find(|value| efforts.iter().any(|effort| effort == value))
        })?;
    Some(NativeReasoning {
        multi_agent_version: "v2".into(),
        ultra_effort: target.into(),
    })
}

/// Direct API callers must never confuse the native Ultra mode with a wire value.
pub fn api_efforts(model: &ChannelModel) -> Vec<String> {
    match &model.reasoning_efforts {
        Some(selected) => API_EFFORTS
            .iter()
            .filter(|effort| selected.iter().any(|value| value == **effort))
            .map(|effort| (*effort).to_owned())
            .collect(),
        None => known_model(&model.id)
            .and_then(|entry| entry.efforts.clone())
            .unwrap_or_default(),
    }
}

pub fn validate_api_effort(model: &ChannelModel, effort: Option<&str>) -> Result<(), String> {
    let Some(effort) = effort else {
        return Ok(());
    };
    if effort == "ultra" {
        return Err(
            "Ultra 是 Codex 原生多代理模式，不能直接用于 API 测试。请选择该模型的具体推理档位。"
                .into(),
        );
    }
    if !api_efforts(model).iter().any(|allowed| allowed == effort) {
        return Err("所选推理档位不在该模型已确认的 API 能力中。".into());
    }
    Ok(())
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
                    "推理档位无效，请使用 none、minimal、low、medium、high、xhigh、max 或原生 Ultra。".into(),
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
    let supported = capabilities(model);
    if supported.efforts.iter().any(|effort| effort == "ultra") {
        let Some(native) = &supported.native else {
            return Err("Ultra 需要至少一个可用的底层推理档位（low 至 max）。".into());
        };
        if native.multi_agent_version != "v2"
            || !["low", "medium", "high", "xhigh", "max"].contains(&native.ultra_effort.as_str())
            || !supported.efforts.contains(&native.ultra_effort)
        {
            return Err("Ultra 的原生多代理配置无效，底层强度必须属于该模型支持的档位。".into());
        }
    } else {
        // Editing an old restored model to disable Ultra drops obsolete metadata.
        model.native_reasoning = None;
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
                "ultra" => "最高推理并自动委派任务（Codex 原生多代理）",
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
                api_efforts(&model(id)),
                ["none", "low", "medium", "high", "xhigh", "max"]
            );
            assert_eq!(capabilities(&model(id)).default.as_deref(), Some("medium"));
        }
        for id in ["gpt-6-astra", "gpt-6.1-sol"] {
            assert_eq!(
                api_efforts(&model(id)),
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
                    .all(|effort| API_EFFORTS.contains(&effort.as_str())));
                assert!(entry
                    .api_default
                    .as_ref()
                    .is_none_or(|default| efforts.contains(default)));
                assert!(entry
                    .native_default
                    .as_ref()
                    .is_none_or(|default| efforts.contains(default)));
            }
            if let Some(native) = &entry.native_ultra {
                assert_eq!(native.multi_agent_version, "v2");
                let efforts = entry
                    .efforts
                    .as_ref()
                    .or(entry.native_efforts.as_ref())
                    .unwrap();
                assert!(efforts.contains(&native.ultra_effort));
                assert_ne!(native.ultra_effort, "ultra");
            }
            assert!(!documented["sources"].as_array().unwrap().is_empty());
        }
        assert!(ids.len() >= 80);
        assert!(!API_EFFORTS.contains(&"ultra"));
        assert!(EFFORTS.contains(&"ultra"));
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
                default: Some("high".into()),
                native: None,
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

    #[test]
    fn native_ultra_uses_exact_official_models_and_wire_targets() {
        for (id, target) in [
            ("gpt-6-astra", "xhigh"),
            ("gpt-6.1-sol", "xhigh"),
            ("gpt-6-sol", "max"),
            ("gpt-5.6-sol", "max"),
            ("gpt-5.6", "max"),
            ("gpt-5.6-terra", "max"),
            ("gpt-daybreak-blue-latest", "max"),
            ("gpt-daybreak-red-latest", "max"),
        ] {
            let actual = capabilities(&model(id));
            assert_eq!(
                actual.efforts.last().map(String::as_str),
                Some("ultra"),
                "{id}"
            );
            assert_eq!(
                actual.native,
                Some(NativeReasoning {
                    multi_agent_version: "v2".into(),
                    ultra_effort: target.into(),
                }),
                "{id}"
            );
            assert_ne!(actual.default.as_deref(), Some("ultra"));
            assert!(!api_efforts(&model(id))
                .iter()
                .any(|effort| effort == "ultra"));
        }
        for id in [
            "gpt-6-luna",
            "gpt-5.6-luna",
            "gpt-5.5",
            "gpt-6-astra-custom",
        ] {
            let actual = capabilities(&model(id));
            assert!(
                !actual.efforts.iter().any(|effort| effort == "ultra"),
                "{id}"
            );
            assert!(actual.native.is_none(), "{id}");
        }
        assert!(api_efforts(&model("gpt-daybreak-red-latest")).is_empty());
    }

    #[test]
    fn manual_ultra_maps_to_selected_effort_without_enlarging_explicit_lists() {
        let mut actual = model("gpt-6-astra");
        for selected in [vec![], vec!["low".into(), "high".into()]] {
            actual.reasoning_efforts = Some(selected.clone());
            normalize_model(&mut actual).unwrap();
            assert_eq!(capabilities(&actual).efforts, selected);
            assert_eq!(capabilities(&actual).native, None);
        }
        actual.reasoning_efforts = Some(vec!["high".into(), "ultra".into()]);
        actual.default_reasoning_effort = Some("ultra".into());
        normalize_model(&mut actual).unwrap();
        assert_eq!(capabilities(&actual).native.unwrap().ultra_effort, "high");
        assert_eq!(capabilities(&actual).default.as_deref(), Some("ultra"));
        actual.reasoning_efforts = Some(vec!["xhigh".into(), "max".into(), "ultra".into()]);
        assert_eq!(capabilities(&actual).native.unwrap().ultra_effort, "xhigh");
        actual.id = "channel-alias".into();
        assert_eq!(capabilities(&actual).native.unwrap().ultra_effort, "max");
    }

    #[test]
    fn restored_ultra_mapping_is_validated_and_removed_when_disabled() {
        let mut actual = model("gpt-6-astra");
        actual.reasoning_efforts = Some(vec!["high".into(), "xhigh".into(), "ultra".into()]);
        actual.native_reasoning = Some(NativeReasoning {
            multi_agent_version: "v2".into(),
            ultra_effort: "high".into(),
        });
        normalize_model(&mut actual).unwrap();
        assert_eq!(capabilities(&actual).native.unwrap().ultra_effort, "high");
        actual.native_reasoning.as_mut().unwrap().ultra_effort = "max".into();
        assert!(normalize_model(&mut actual).is_err());
        actual.native_reasoning.as_mut().unwrap().ultra_effort = "ultra".into();
        assert!(normalize_model(&mut actual).is_err());
        actual.native_reasoning.as_mut().unwrap().ultra_effort = "high".into();
        actual
            .native_reasoning
            .as_mut()
            .unwrap()
            .multi_agent_version = "v1".into();
        assert!(normalize_model(&mut actual).is_err());
        actual.reasoning_efforts = Some(vec!["high".into()]);
        normalize_model(&mut actual).unwrap();
        assert_eq!(actual.native_reasoning, None);
        actual.reasoning_efforts = Some(vec!["none".into(), "ultra".into()]);
        assert!(normalize_model(&mut actual).is_err());
    }

    #[test]
    fn api_evaluations_never_treat_ultra_as_an_api_value() {
        let actual = model("gpt-6-astra");
        assert!(validate_api_effort(&actual, None).is_ok());
        assert!(validate_api_effort(&actual, Some("xhigh")).is_ok());
        assert!(validate_api_effort(&actual, Some("max")).is_ok());
        assert!(validate_api_effort(&actual, Some("ultra")).is_err());
        assert!(validate_api_effort(&actual, Some("none")).is_err());
        assert!(validate_api_effort(&model("gpt-daybreak-red-latest"), Some("max")).is_err());
        let mut custom = model("custom");
        custom.reasoning_efforts = Some(vec!["high".into(), "ultra".into()]);
        assert!(validate_api_effort(&custom, Some("high")).is_ok());
        assert!(validate_api_effort(&custom, Some("ultra")).is_err());
        assert!(validate_api_effort(&custom, Some("max")).is_err());
    }
}
