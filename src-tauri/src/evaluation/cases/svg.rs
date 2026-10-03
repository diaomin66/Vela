//! A conservative static-SVG subset. Preview only as an image, never injected HTML.
use super::{EvaluationCheck, Grade};
use quick_xml::{events::Event, Reader, Writer};

const TAGS: &[&str] = &[
    "svg",
    "g",
    "defs",
    "title",
    "desc",
    "path",
    "rect",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "text",
    "tspan",
    "linearGradient",
    "radialGradient",
    "stop",
    "clipPath",
];
const ATTRS: &[&str] = &[
    "xmlns",
    "viewBox",
    "width",
    "height",
    "x",
    "y",
    "x1",
    "x2",
    "y1",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "d",
    "points",
    "fill",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "fill-rule",
    "clip-rule",
    "opacity",
    "fill-opacity",
    "stroke-opacity",
    "transform",
    "id",
    "offset",
    "stop-color",
    "stop-opacity",
    "gradientUnits",
    "gradientTransform",
    "fx",
    "fy",
    "fr",
    "text-anchor",
    "dominant-baseline",
    "font-family",
    "font-size",
    "font-weight",
    "letter-spacing",
    "dx",
    "dy",
    "preserveAspectRatio",
    "clip-path",
];
const SHAPES: &[&str] = &[
    "path", "rect", "circle", "ellipse", "line", "polyline", "polygon",
];

fn safe_value(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    if lower.contains("url(") {
        return lower
            .strip_prefix("url(#")
            .and_then(|value| value.strip_suffix(')'))
            .is_some_and(|id| {
                !id.is_empty()
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            });
    }
    !lower.contains("://")
        && !lower.contains("javascript:")
        && !lower.contains("data:")
        && !lower.contains('\\')
        && !lower.chars().any(|c| c.is_control() && !c.is_whitespace())
}
fn sanitize(output: &str) -> Option<(String, bool, usize)> {
    let output = output.trim();
    let output = if let Some(rest) = output
        .strip_prefix("```svg")
        .or_else(|| output.strip_prefix("```xml"))
        .or_else(|| output.strip_prefix("```"))
    {
        rest.strip_suffix("```")?.trim()
    } else {
        output
    };
    let mut reader = Reader::from_str(output);
    let mut writer = Writer::new(Vec::new());
    let (mut depth, mut roots, mut nodes, mut shapes, mut viewport, mut root_namespace) =
        (0, 0, 0, 0, false, false);
    loop {
        let event = reader.read_event().ok()?;
        match &event {
            Event::Start(element) | Event::Empty(element) => {
                nodes += 1;
                if nodes > 1500 || depth > 32 {
                    return None;
                }
                let name = element.name().as_ref().to_owned();
                if !TAGS.contains(&name.as_str()) {
                    return None;
                }
                if depth == 0 {
                    if name != "svg" || roots != 0 {
                        return None;
                    }
                    roots += 1;
                }
                if SHAPES.contains(&name.as_str()) {
                    shapes += 1;
                }
                for attr in element.attributes() {
                    let attr = attr.ok()?;
                    let key = attr.key.as_ref();
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .ok()?;
                    if !ATTRS.contains(&key) {
                        return None;
                    }
                    if key == "xmlns" {
                        if value != "http://www.w3.org/2000/svg" || depth != 0 {
                            return None;
                        }
                        root_namespace = true;
                    } else if !safe_value(&value) {
                        return None;
                    }
                    if depth == 0 && key == "viewBox" {
                        let values = value
                            .split(|c: char| c.is_whitespace() || c == ',')
                            .filter(|s| !s.is_empty())
                            .map(str::parse::<f64>)
                            .collect::<Result<Vec<_>, _>>()
                            .ok()?;
                        viewport = values.len() == 4
                            && values.iter().all(|v| v.is_finite())
                            && values[2] > 0.
                            && values[3] > 0.;
                    }
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
                writer.write_event(event).ok()?;
            }
            Event::End(_) => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                writer.write_event(event).ok()?;
            }
            Event::Text(text) => {
                if depth == 0 && !text.xml10_content().trim().is_empty() {
                    return None;
                }
                writer.write_event(event).ok()?;
            }
            Event::Decl(_) | Event::Comment(_) => {}
            Event::GeneralRef(reference) => {
                if depth == 0 || !["amp", "lt", "gt", "quot", "apos"].contains(&reference.as_ref())
                {
                    return None;
                }
                writer.write_event(event).ok()?;
            }
            Event::Eof => break,
            // No DTD, processing instructions or CDATA with hidden markup.
            _ => return None,
        }
    }
    if depth != 0 || roots != 1 {
        return None;
    }
    let svg = String::from_utf8(writer.into_inner()).ok()?;
    // Always use the SVG namespace so an otherwise valid document can be decoded as an image.
    let svg = if root_namespace {
        svg
    } else {
        svg.replacen("<svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"", 1)
    };
    Some((svg, viewport, shapes))
}
pub(super) fn grade(output: &str) -> Grade {
    let parsed = sanitize(output);
    let valid = parsed.is_some();
    let viewport = parsed.as_ref().is_some_and(|value| value.1);
    let shapes = parsed.as_ref().is_some_and(|value| value.2 >= 1);
    let checks = vec![
        EvaluationCheck {
            label: "完整、安全的静态 SVG".into(),
            passed: valid,
        },
        EvaluationCheck {
            label: "按要求声明有效 viewBox".into(),
            passed: viewport,
        },
        EvaluationCheck {
            label: "包含绘图元素（不评价画面内容）".into(),
            passed: shapes,
        },
    ];
    Grade {
        score: u32::from(valid) * 50 + u32::from(viewport) * 25 + u32::from(shapes) * 25,
        checks,
        safe_svg: parsed.map(|value| value.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_safe_static_svg_can_be_previewed() {
        let valid = r##"<svg viewBox="0 0 200 100"><circle cx="30" cy="70" r="20"/><circle cx="150" cy="70" r="20"/><path d="M30 70L90 20L150 70Z" fill="none"/><path d="M80 40L100 20"/></svg>"##;
        assert_eq!(grade(valid).score, 100);
        assert!(grade(valid).safe_svg.unwrap().contains("xmlns="));
        for bad in [
            "<svg><script>alert(1)</script></svg>",
            "<svg onload='alert(1)'/>",
            "<svg><foreignObject><p>unsafe</p></foreignObject></svg>",
            "<svg><image href='https://example.com/track'/></svg>",
            "<!DOCTYPE svg [<!ENTITY x SYSTEM 'file:///test'>]><svg>&x;</svg>",
            "<svg/><svg/>",
            "<svg><g></svg>",
            "<svg><path fill='url(https://example.com/a)'/></svg>",
            "<svg><style>*{fill:red}</style></svg>",
        ] {
            assert!(grade(bad).safe_svg.is_none(), "{bad}");
        }
    }
    #[test]
    fn valid_compound_paths_are_not_penalized_and_viewbox_is_required() {
        let compound = r#"<svg viewBox="0 0 100 100"><path d="M0 0L10 10 M20 20L30 30"/></svg>"#;
        assert_eq!(grade(compound).score, 100);
        assert_eq!(
            grade(r#"<svg width="junk" height="junk"><path d="M0 0L10 10"/></svg>"#).score,
            75
        );
        let title_trick = r#"<svg viewBox="0 0 100 100"><title>xmlns=example</title><path d="M0 0L10 10"/></svg>"#;
        assert!(grade(title_trick)
            .safe_svg
            .unwrap()
            .starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        let spaced = r#"<svg xmlns = "http://www.w3.org/2000/svg" viewBox="0 0 100 100"><path d="M0 0L10 10"/></svg>"#;
        assert_eq!(grade(spaced).safe_svg.unwrap().matches("xmlns").count(), 1);
    }
}
