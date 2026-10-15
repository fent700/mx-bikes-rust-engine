use glam::{Vec2, Vec3};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Unexpected end of input while parsing block '{0}'")]
    UnexpectedEof(String),
    #[error("Invalid syntax at line {line}: {message}")]
    SyntaxError { line: usize, message: String },
}

/// A value parsed from a PiBoSo configuration file.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    String(String),
    Float(f32),
    Int(i64),
    Vec2(Vec2),
    Vec3(Vec3),
    List(Vec<String>),
}

impl ConfigValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ConfigValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            ConfigValue::Float(f) => Some(*f),
            ConfigValue::Int(i) => Some(*i as f32),
            ConfigValue::String(s) => s.parse::<f32>().ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ConfigValue::Int(i) => Some(*i),
            ConfigValue::Float(f) => Some(*f as i64),
            ConfigValue::String(s) => s.parse::<i64>().ok(),
            _ => None,
        }
    }

    pub fn as_vec2(&self) -> Option<Vec2> {
        match self {
            ConfigValue::Vec2(v) => Some(*v),
            ConfigValue::String(s) => parse_vec2(s),
            _ => None,
        }
    }

    pub fn as_vec3(&self) -> Option<Vec3> {
        match self {
            ConfigValue::Vec3(v) => Some(*v),
            ConfigValue::String(s) => parse_vec3(s),
            _ => None,
        }
    }
}

/// A structured node in the PiBoSo configuration hierarchy.
#[derive(Debug, Clone, Default)]
pub struct ConfigNode {
    pub name: String,
    pub properties: HashMap<String, ConfigValue>,
    pub children: Vec<ConfigNode>,
}

impl ConfigNode {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            properties: HashMap::new(),
            children: Vec::new(),
        }
    }

    /// Gets a property value, matching case-insensitively like PiBoSo's engine.
    pub fn get_value(&self, key: &str) -> Option<&ConfigValue> {
        self.properties.get(key).or_else(|| {
            self.properties
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v)
        })
    }

    /// Gets a string property value.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get_value(key).and_then(|v| v.as_str())
    }

    /// Gets a float property value.
    pub fn get_f32(&self, key: &str) -> Option<f32> {
        self.get_value(key).and_then(|v| v.as_f32())
    }

    /// Gets an integer property value.
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get_value(key).and_then(|v| v.as_i64())
    }

    /// Gets a 3D vector property value.
    pub fn get_vec3(&self, key: &str) -> Option<Vec3> {
        self.get_value(key).and_then(|v| v.as_vec3())
    }

    /// Gets a 2D vector property value.
    pub fn get_vec2(&self, key: &str) -> Option<Vec2> {
        self.get_value(key).and_then(|v| v.as_vec2())
    }

    /// Finds the first child block with the specified name.
    pub fn get_child(&self, name: &str) -> Option<&ConfigNode> {
        self.children.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Mutable access to the first child block with the specified name.
    pub fn get_child_mut(&mut self, name: &str) -> Option<&mut ConfigNode> {
        self.children.iter_mut().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Follows a `/`-separated block path (`front_suspension/damper`).
    pub fn path(&self, path: &str) -> Option<&ConfigNode> {
        path.split('/').filter(|p| !p.is_empty()).try_fold(self, |n, p| n.get_child(p))
    }

    /// Mutable form of [`ConfigNode::path`].
    pub fn path_mut(&mut self, path: &str) -> Option<&mut ConfigNode> {
        path.split('/').filter(|p| !p.is_empty()).try_fold(self, |n, p| n.get_child_mut(p))
    }

    /// Sets a property (keys are stored lower case, as the parser does).
    pub fn set(&mut self, key: &str, value: ConfigValue) {
        self.properties.insert(key.to_lowercase(), value);
    }

    /// Finds all child blocks with the specified name.
    pub fn get_children(&self, name: &str) -> Vec<&ConfigNode> {
        self.children
            .iter()
            .filter(|c| c.name.eq_ignore_ascii_case(name))
            .collect()
    }

    /// Parses a PiBoSo configuration text document.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut root = ConfigNode::new("root");
        let lines: Vec<&str> = text.lines().collect();
        let mut idx = 0;
        parse_block_body(&lines, &mut idx, &mut root)?;
        Ok(root)
    }
}

fn parse_block_body(
    lines: &[&str],
    idx: &mut usize,
    current_node: &mut ConfigNode,
) -> Result<(), ParseError> {
    while *idx < lines.len() {
        let _line_num = *idx + 1;
        let raw_line = lines[*idx].trim();
        *idx += 1;

        // Skip comments and empty lines
        if raw_line.is_empty() || raw_line.starts_with(';') || raw_line.starts_with("//") {
            continue;
        }

        // Strip inline comments
        let cleaned = if let Some(pos) = raw_line.find(';') {
            &raw_line[..pos].trim()
        } else if let Some(pos) = raw_line.find("//") {
            &raw_line[..pos].trim()
        } else {
            raw_line
        };

        if cleaned.is_empty() {
            continue;
        }

        // End of block
        if cleaned == "}" {
            return Ok(());
        }

        // Section header like [info] or [ui] (treat as child block)
        if cleaned.starts_with('[') && cleaned.ends_with(']') {
            let section_name = cleaned[1..cleaned.len() - 1].trim();
            let mut section_node = ConfigNode::new(section_name);
            // Collect until next section header
            while *idx < lines.len() {
                let peek = lines[*idx].trim();
                if peek.starts_with('[') && peek.ends_with(']') {
                    break;
                }
                parse_single_property_or_child(lines, idx, &mut section_node)?;
            }
            current_node.children.push(section_node);
            continue;
        }

        // Check if starting a nested block: "name" followed by "{" or "name {"
        if cleaned.ends_with('{') {
            let block_name = cleaned.trim_end_matches('{').trim();
            let mut child = ConfigNode::new(block_name);
            parse_block_body(lines, idx, &mut child)?;
            current_node.children.push(child);
            continue;
        }

        // Check if next non-empty line is "{"
        let mut lookahead = *idx;
        let mut is_block = false;
        while lookahead < lines.len() {
            let l = lines[lookahead].trim();
            if l.is_empty() || l.starts_with(';') || l.starts_with("//") {
                lookahead += 1;
                continue;
            }
            if l == "{" {
                is_block = true;
                *idx = lookahead + 1;
            }
            break;
        }

        if is_block {
            let mut child = ConfigNode::new(cleaned);
            parse_block_body(lines, idx, &mut child)?;
            current_node.children.push(child);
            continue;
        }

        // Regular key = value line
        if let Some((key, val_str)) = cleaned.split_once('=') {
            let key = key.trim().to_lowercase();
            let val = parse_value(val_str.trim());
            current_node.properties.insert(key, val);
        } else {
            // Unpaired identifier or flag
            current_node
                .properties
                .insert(cleaned.to_lowercase(), ConfigValue::String(cleaned.to_string()));
        }
    }

    Ok(())
}

fn parse_single_property_or_child(
    lines: &[&str],
    idx: &mut usize,
    node: &mut ConfigNode,
) -> Result<(), ParseError> {
    if *idx >= lines.len() {
        return Ok(());
    }
    let raw_line = lines[*idx].trim();
    *idx += 1;

    if raw_line.is_empty() || raw_line.starts_with(';') || raw_line.starts_with("//") {
        return Ok(());
    }

    let cleaned = if let Some(pos) = raw_line.find(';') {
        raw_line[..pos].trim()
    } else {
        raw_line
    };

    if cleaned.is_empty() {
        return Ok(());
    }

    if let Some((key, val_str)) = cleaned.split_once('=') {
        let key = key.trim().to_lowercase();
        let val = parse_value(val_str.trim());
        node.properties.insert(key, val);
    }

    Ok(())
}

fn parse_value(s: &str) -> ConfigValue {
    let s = s.trim();

    // Check if comma-separated list / vector
    if s.contains(',') {
        let parts: Vec<&str> = s.split(',').map(|p| p.trim()).collect();
        if parts.len() == 2 {
            if let (Ok(x), Ok(y)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>()) {
                return ConfigValue::Vec2(Vec2::new(x, y));
            }
        } else if parts.len() == 3 {
            if let (Ok(x), Ok(y), Ok(z)) = (
                parts[0].parse::<f32>(),
                parts[1].parse::<f32>(),
                parts[2].parse::<f32>(),
            ) {
                return ConfigValue::Vec3(Vec3::new(x, y, z));
            }
        }
        return ConfigValue::List(parts.into_iter().map(String::from).collect());
    }

    // Try integer
    if let Ok(i) = s.parse::<i64>() {
        return ConfigValue::Int(i);
    }

    // Try float
    if let Ok(f) = s.parse::<f32>() {
        return ConfigValue::Float(f);
    }

    // Fallback to string (strip quotes if any)
    let unquoted = s.trim_matches('"');
    ConfigValue::String(unquoted.to_string())
}

fn parse_vec2(s: &str) -> Option<Vec2> {
    let parts: Vec<&str> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() == 2 {
        let x = parts[0].parse::<f32>().ok()?;
        let y = parts[1].parse::<f32>().ok()?;
        Some(Vec2::new(x, y))
    } else {
        None
    }
}

fn parse_vec3(s: &str) -> Option<Vec3> {
    let parts: Vec<&str> = s.split(',').map(|p| p.trim()).collect();
    if parts.len() == 3 {
        let x = parts[0].parse::<f32>().ok()?;
        let y = parts[1].parse::<f32>().ok()?;
        let z = parts[2].parse::<f32>().ok()?;
        Some(Vec3::new(x, y, z))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bike_config() {
        let sample = r#"
type = bike
ID = 250xf
geom = 250xf.geom

engine
{
	Inertia = 0.006

	mapping0
	{
		id = msm 250xf
		curves = 250xf.engn
		name = Race
	}
	IdleRPM = 2300
    Ratio0 = 24, 73
}
"#;
        let root = ConfigNode::parse(sample).expect("Failed to parse config");
        assert_eq!(root.get_str("type"), Some("bike"));
        assert_eq!(root.get_str("id"), Some("250xf"));

        let engine = root.get_child("engine").expect("Missing engine block");
        assert_eq!(engine.get_f32("inertia"), Some(0.006));
        assert_eq!(engine.get_i64("idlerpm"), Some(2300));

        let mapping = engine.get_child("mapping0").expect("Missing mapping0");
        assert_eq!(mapping.get_str("name"), Some("Race"));
    }
}
