//! Parse and mutate YAML frontmatter in `SKILL.md` files.

use std::collections::BTreeMap;

use thiserror::Error;

const DISABLE_KEY: &str = "disable-model-invocation";

/// Parsed representation of a `SKILL.md` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMd {
    /// YAML frontmatter key-value pairs; empty when the file has no frontmatter.
    pub frontmatter: BTreeMap<String, serde_yaml::Value>,
    /// Markdown body after the frontmatter block.
    pub body: String,
}

#[derive(Debug, Error)]
pub enum FrontmatterError {
    #[error("invalid SKILL.md frontmatter: {0}")]
    Invalid(String),
    #[error("failed to serialize frontmatter: {0}")]
    Serialize(String),
}

/// Parses a `SKILL.md` string into frontmatter and body.
pub fn parse_skill_md(content: &str) -> Result<SkillMd, FrontmatterError> {
    let trimmed = content.strip_prefix('\u{feff}').unwrap_or(content);
    if !trimmed.starts_with("---") {
        return Ok(SkillMd {
            frontmatter: BTreeMap::new(),
            body: trimmed.to_string(),
        });
    }

    let rest = trimmed.strip_prefix("---").unwrap_or(trimmed);
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"));
    let Some(rest) = rest else {
        return Err(FrontmatterError::Invalid(
            "missing newline after opening ---".into(),
        ));
    };

    let end = rest
        .find("\n---")
        .ok_or_else(|| FrontmatterError::Invalid("unclosed frontmatter block".into()))?;
    let yaml_str = &rest[..end];
    let body_start = end + "\n---".len();
    let body = rest[body_start..]
        .strip_prefix('\n')
        .or_else(|| rest[body_start..].strip_prefix("\r\n"))
        .map(str::to_string)
        .unwrap_or_else(|| rest[body_start..].to_string());

    let value: serde_yaml::Value =
        serde_yaml::from_str(yaml_str).map_err(|e| FrontmatterError::Invalid(e.to_string()))?;

    let frontmatter = match value {
        serde_yaml::Value::Mapping(map) => map
            .into_iter()
            .filter_map(|(k, v)| k.as_str().map(|key| (key.to_string(), v)))
            .collect(),
        serde_yaml::Value::Null => BTreeMap::new(),
        _ => {
            return Err(FrontmatterError::Invalid(
                "frontmatter must be a YAML mapping".into(),
            ));
        }
    };

    Ok(SkillMd { frontmatter, body })
}

/// Returns whether frontmatter contains a truthy `disable-model-invocation` value.
pub fn is_disabled(frontmatter: &BTreeMap<String, serde_yaml::Value>) -> bool {
    frontmatter.get(DISABLE_KEY).is_some_and(value_is_truthy)
}

/// Returns whether frontmatter contains a `disable-model-invocation` key (any value).
pub fn has_disable_key(frontmatter: &BTreeMap<String, serde_yaml::Value>) -> bool {
    frontmatter.contains_key(DISABLE_KEY)
}

fn value_is_truthy(value: &serde_yaml::Value) -> bool {
    match value {
        serde_yaml::Value::Bool(b) => *b,
        serde_yaml::Value::String(s) => matches!(s.to_lowercase().as_str(), "true" | "yes" | "1"),
        serde_yaml::Value::Number(n) => n.as_u64() == Some(1) || n.as_i64() == Some(1),
        _ => false,
    }
}

/// Applies enable/disable to parsed content and returns the serialized `SKILL.md`.
///
/// When disabling a file without frontmatter, injects minimal `name` + flag block.
/// When enabling, removes the flag; collapses injected-only blocks (`name` + flag only).
pub fn set_disabled(
    parsed: &SkillMd,
    disabled: bool,
    skill_name: &str,
) -> Result<String, FrontmatterError> {
    let mut fm = parsed.frontmatter.clone();

    if disabled {
        fm.insert(DISABLE_KEY.to_string(), serde_yaml::Value::Bool(true));
        if !fm.contains_key("name") {
            fm.insert(
                "name".to_string(),
                serde_yaml::Value::String(skill_name.to_string()),
            );
        }
    } else {
        let was_disabled = is_disabled(&parsed.frontmatter);
        let only_injected_keys = was_disabled
            && parsed
                .frontmatter
                .keys()
                .all(|k| k == "name" || k == DISABLE_KEY);
        fm.remove(DISABLE_KEY);
        if only_injected_keys {
            fm.clear();
        }
    }

    render_skill_md(&fm, &parsed.body)
}

fn render_skill_md(
    frontmatter: &BTreeMap<String, serde_yaml::Value>,
    body: &str,
) -> Result<String, FrontmatterError> {
    if frontmatter.is_empty() {
        return Ok(body.to_string());
    }

    let mut map = serde_yaml::Mapping::new();
    for key in ordered_frontmatter_keys(frontmatter) {
        map.insert(
            serde_yaml::Value::String(key.clone()),
            frontmatter[&key].clone(),
        );
    }
    let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(map))
        .map_err(|e| FrontmatterError::Serialize(e.to_string()))?;
    let yaml = yaml.trim_end_matches('\n');
    Ok(format!("---\n{yaml}\n---\n{body}"))
}

fn ordered_frontmatter_keys(frontmatter: &BTreeMap<String, serde_yaml::Value>) -> Vec<String> {
    let mut keys: Vec<String> = frontmatter.keys().cloned().collect();
    keys.sort_by(|a, b| match (a.as_str(), b.as_str()) {
        ("name", _) => std::cmp::Ordering::Less,
        (_, "name") => std::cmp::Ordering::Greater,
        (DISABLE_KEY, _) => std::cmp::Ordering::Less,
        (_, DISABLE_KEY) => std::cmp::Ordering::Greater,
        (a, b) => a.cmp(b),
    });
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_body_only() {
        let md = parse_skill_md("# Hello\n\nWorld").unwrap();
        assert!(md.frontmatter.is_empty());
        assert_eq!(md.body, "# Hello\n\nWorld");
    }

    #[test]
    fn parse_strips_bom_from_body_only() {
        let md = parse_skill_md("\u{feff}# Hello").unwrap();
        assert_eq!(md.body, "# Hello");
    }

    #[test]
    fn parse_with_frontmatter() {
        let content = "---\nname: foo\ndescription: bar\n---\n# Body\n";
        let md = parse_skill_md(content).unwrap();
        assert_eq!(
            md.frontmatter.get("name").and_then(|v| v.as_str()),
            Some("foo")
        );
        assert_eq!(md.body, "# Body\n");
    }

    #[test]
    fn is_disabled_accepts_truthy_scalars() {
        let mut fm = BTreeMap::new();
        fm.insert(DISABLE_KEY.into(), serde_yaml::Value::String("true".into()));
        assert!(is_disabled(&fm));

        fm.insert(DISABLE_KEY.into(), serde_yaml::Value::String("yes".into()));
        assert!(is_disabled(&fm));
    }

    #[test]
    fn disable_injects_frontmatter_when_missing() {
        let md = parse_skill_md("# Only body").unwrap();
        let out = set_disabled(&md, true, "my-skill").unwrap();
        assert!(out.starts_with("---\nname: my-skill\n"));
        assert!(out.contains("disable-model-invocation: true"));
        assert!(out.ends_with("# Only body"));
    }

    #[test]
    fn disable_preserves_other_keys() {
        let content = "---\nname: foo\ndescription: keep me\n---\n# Body\n";
        let md = parse_skill_md(content).unwrap();
        let out = set_disabled(&md, true, "foo").unwrap();
        let reparsed = parse_skill_md(&out).unwrap();
        assert!(is_disabled(&reparsed.frontmatter));
        assert_eq!(
            reparsed
                .frontmatter
                .get("description")
                .and_then(|v| v.as_str()),
            Some("keep me")
        );
    }

    #[test]
    fn enable_removes_flag_and_collapses_injected_block() {
        let content = "---\nname: foo\ndisable-model-invocation: true\n---\n# Body\n";
        let md = parse_skill_md(content).unwrap();
        let out = set_disabled(&md, false, "foo").unwrap();
        assert_eq!(out, "# Body\n");
    }

    #[test]
    fn enable_preserves_name_only_frontmatter() {
        let content = "---\nname: foo\n---\n# Body\n";
        let md = parse_skill_md(content).unwrap();
        let out = set_disabled(&md, false, "foo").unwrap();
        let reparsed = parse_skill_md(&out).unwrap();
        assert_eq!(
            reparsed.frontmatter.get("name").and_then(|v| v.as_str()),
            Some("foo")
        );
        assert!(!is_disabled(&reparsed.frontmatter));
    }

    #[test]
    fn enable_removes_flag_keeps_other_keys() {
        let content =
            "---\nname: foo\ndescription: x\ndisable-model-invocation: true\n---\n# Body\n";
        let md = parse_skill_md(content).unwrap();
        let out = set_disabled(&md, false, "foo").unwrap();
        let reparsed = parse_skill_md(&out).unwrap();
        assert!(!is_disabled(&reparsed.frontmatter));
        assert!(reparsed.frontmatter.contains_key("description"));
    }
}
