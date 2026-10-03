pub(super) fn extract_html(output: &str) -> Option<String> {
    let text = output.trim();
    let mut remainder = text;
    while let Some(start) = remainder.find("```") {
        let block = &remainder[start + 3..];
        let Some(line_end) = block.find('\n') else {
            break;
        };
        let language = block[..line_end].trim().to_ascii_lowercase();
        let body = &block[line_end + 1..];
        let Some(end) = body.find("```") else {
            break;
        };
        let candidate = body[..end].trim();
        if matches!(language.as_str(), "html" | "svg" | "xml" | "")
            && candidate.starts_with('<')
            && contains_document(candidate)
        {
            return Some(candidate.into());
        }
        remainder = &body[end + 3..];
    }
    let lower = text.to_ascii_lowercase();
    if text.starts_with('<') && contains_document(text) {
        return Some(text.into());
    }
    for (opening, closing) in [
        ("<!doctype html", "</html>"),
        ("<html", "</html>"),
        ("<svg", "</svg>"),
    ] {
        if let Some(start) = lower.find(opening) {
            if let Some(end) = lower.rfind(closing).filter(|end| *end >= start) {
                return Some(text[start..end + closing.len()].into());
            }
        }
    }
    None
}

fn contains_document(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    ["<html", "<!doctype html", "<svg", "<body"]
        .iter()
        .any(|tag| lower.contains(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fenced_animation_without_rewriting_scripts_styles_or_svg() {
        let html = "<!DOCTYPE html><html><head><style>@keyframes ride{to{transform:rotate(360deg)}}</style></head><body><svg><animateTransform attributeName=\"transform\" type=\"rotate\" dur=\"1s\" repeatCount=\"indefinite\"/></svg><script>requestAnimationFrame(() => {});</script></body></html>";
        assert_eq!(
            extract_html(&format!("这里是动画：\n```html\n{html}\n```\n完成。")),
            Some(html.into())
        );
        assert_eq!(extract_html(html), Some(html.into()));
        assert_eq!(
            extract_html(&format!("预览：\n{html}\n完成。")),
            Some(html.into())
        );
    }

    #[test]
    fn supports_svg_and_skips_non_artifact_fences() {
        let svg = "<svg viewBox=\"0 0 640 400\"><circle><animate attributeName=\"r\" values=\"10;20;10\" dur=\"1s\"/></circle></svg>";
        assert_eq!(extract_html(svg), Some(svg.into()));
        assert_eq!(
            extract_html(&format!(
                "```json\n{{\"ok\":true}}\n```\n```svg\n{svg}\n```"
            )),
            Some(svg.into())
        );
        assert!(extract_html("I could not produce an animation.").is_none());
        assert!(extract_html("```html\nNo markup\n```").is_none());
    }
}
