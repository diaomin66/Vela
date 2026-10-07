use sha2::{Digest, Sha256};

fn digest(route: &str) -> Option<&str> {
    let value = route
        .strip_prefix("ahax-")
        .or_else(|| route.strip_prefix("vela-"))?;
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(value)
}

pub(crate) fn is_internal_route_id(route: &str) -> bool {
    digest(route).is_some()
}

pub(crate) fn canonical_route_id(route: &str) -> String {
    match digest(route) {
        Some(value) => format!("ahax-{value}"),
        None => route.to_owned(),
    }
}

pub(crate) fn route_ids_match(left: &str, right: &str) -> bool {
    left == right || digest(left).is_some_and(|value| digest(right) == Some(value))
}

pub(super) fn route_id(profile: &str, model: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(profile.as_bytes());
    hash.update([0]);
    hash.update(model.as_bytes());
    format!("ahax-{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renamed_routes_retain_the_original_channel_and_model_identity() {
        let current = route_id("profile-a", "gpt-test");
        let legacy = current.replacen("ahax-", "vela-", 1);
        assert!(route_ids_match(&legacy, &current));
        assert_eq!(canonical_route_id(&legacy), current);
        assert!(!route_ids_match(
            &legacy,
            &route_id("profile-b", "gpt-test")
        ));
        assert!(!route_ids_match(
            &legacy,
            &route_id("profile-a", "gpt-other")
        ));
    }

    #[test]
    fn ordinary_upstream_names_are_not_treated_as_internal_routes() {
        for name in ["vela-model", "ahax-model", "vela-unknown", "gpt-6", ""] {
            assert!(!is_internal_route_id(name));
            assert_eq!(canonical_route_id(name), name);
        }
        assert!(is_internal_route_id(&format!("vela-{}", "a".repeat(64))));
        assert!(is_internal_route_id(&format!("ahax-{}", "f".repeat(64))));
        assert!(!is_internal_route_id(&format!("vela-{}", "z".repeat(64))));
        assert!(!route_ids_match("vela-model", "ahax-model"));
    }
}
