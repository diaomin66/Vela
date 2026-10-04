pub(super) fn text(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut ranges = Vec::new();
    for (offset, _) in lower.match_indices("sk-") {
        let end = token_end(input, offset);
        if end.saturating_sub(offset) >= 10 {
            ranges.push((offset, end));
        }
    }
    for (offset, _) in lower.match_indices("bearer") {
        let rest = &input[offset + 6..];
        let spaces = rest.len()
            - rest
                .trim_start_matches(|character: char| character.is_ascii_whitespace())
                .len();
        if spaces == 0 {
            continue;
        }
        let start = offset + 6 + spaces;
        let end = token_end(input, start);
        if end > start {
            ranges.push((start, end));
        }
    }
    for marker in ["openai_api_key", "api_key", "api-key"] {
        for (offset, _) in lower.match_indices(marker) {
            let mut start = offset + marker.len();
            while input
                .as_bytes()
                .get(start)
                .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'\"' | b'\''))
            {
                start += 1;
            }
            if !input
                .as_bytes()
                .get(start)
                .is_some_and(|byte| matches!(*byte, b'=' | b':'))
            {
                continue;
            }
            start += 1;
            while input
                .as_bytes()
                .get(start)
                .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'\"' | b'\''))
            {
                start += 1;
            }
            let end = token_end(input, start);
            if end > start {
                ranges.push((start, end));
            }
        }
    }
    ranges.sort_unstable();
    let mut output = String::new();
    let mut cursor = 0;
    for (start, end) in ranges {
        if start < cursor {
            continue;
        }
        output.push_str(&input[cursor..start]);
        output.push_str("[已隐藏凭据]");
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn token_end(input: &str, start: usize) -> usize {
    input[start..]
        .char_indices()
        .find_map(|(offset, character)| {
            (!character.is_ascii_alphanumeric()
                && !matches!(character, '-' | '_' | '.' | '/' | '+' | '='))
            .then_some(start + offset)
        })
        .unwrap_or(input.len())
}

#[cfg(test)]
mod tests {
    #[test]
    fn common_credentials_are_removed_from_previews_without_changing_surrounding_text() {
        let text = "中文 sk-proj-sensitive-example，Bearer custom-token-123; OPENAI_API_KEY='example-secret'; \"api_key\":\"second-secret\"";
        let result = super::text(text);
        assert_eq!(result.matches("[已隐藏凭据]").count(), 4);
        assert!(result.starts_with("中文 "));
        for secret in [
            "sensitive",
            "custom-token",
            "example-secret",
            "second-secret",
        ] {
            assert!(!result.contains(secret));
        }
    }
}
