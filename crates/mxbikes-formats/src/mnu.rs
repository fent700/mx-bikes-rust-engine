//! PiBoSo UI layout files: `ui/*.mnu` menus and `ui/ui.ui`.
//!
//! Plain text: `key value value ...` lines and named blocks `name` followed by `{ ... }`. There
//! are no comments or quotes in stock files. Rects are `x0 y0 x1 y1` in screen fractions of the
//! 16:9 layout `ui.ui` declares; colours are `a r g b` (0..255).

#[derive(Debug, Clone, Default)]
pub struct MenuNode {
    pub name: String,
    /// Key lines inside this block, in file order.
    pub values: Vec<(String, Vec<String>)>,
    pub children: Vec<MenuNode>,
}

impl MenuNode {
    pub fn parse(text: &str) -> Result<Self, String> {
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let mut root = MenuNode::default();
        let mut stack: Vec<MenuNode> = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i];
            if l == "}" {
                let done = stack.pop().ok_or(format!("unbalanced '}}' at line {i}"))?;
                stack.last_mut().unwrap_or(&mut root).children.push(done);
            } else if lines.get(i + 1) == Some(&"{") && !l.contains(char::is_whitespace) {
                stack.push(MenuNode { name: l.to_string(), ..Default::default() });
                i += 1;
            } else {
                let mut parts = l.split_whitespace();
                let key = parts.next().unwrap_or_default().to_string();
                let vals = parts.map(str::to_string).collect();
                stack.last_mut().unwrap_or(&mut root).values.push((key, vals));
            }
            i += 1;
        }
        if !stack.is_empty() {
            return Err(format!("{} unclosed block(s)", stack.len()));
        }
        Ok(root)
    }

    pub fn get(&self, key: &str) -> Option<&[String]> {
        self.values.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_slice())
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.first()).map(String::as_str)
    }

    pub fn floats(&self, key: &str) -> Option<Vec<f32>> {
        self.get(key)?.iter().map(|s| s.parse().ok()).collect()
    }

    /// `x0 y0 x1 y1` in screen fractions.
    pub fn rect(&self) -> Option<[f32; 4]> {
        let v = self.floats("rect")?;
        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
    }

    /// `a r g b` (0..255) as linear-ignorant sRGB fractions `[r, g, b, a]`.
    pub fn color(&self, key: &str) -> Option<[f32; 4]> {
        let v = self.floats(key)?;
        (v.len() == 4).then(|| [v[1] / 255.0, v[2] / 255.0, v[3] / 255.0, v[0] / 255.0])
    }

    pub fn child(&self, name: &str) -> Option<&MenuNode> {
        self.children.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }
}

/// `ui/english.str`: alternating id and text lines.
#[derive(Debug, Clone, Default)]
pub struct StringTable {
    entries: Vec<(String, String)>,
}

impl StringTable {
    pub fn parse(text: &str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        Self {
            entries: lines.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0].trim().to_string(), c[1].to_string())).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Text for an id (ids match without case, as in the stock menus that mix `Exit` / `exit`).
    pub fn get(&self, id: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(id)).map(|(_, v)| v.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_blocks() {
        let m = MenuNode::parse("dialog\n{\n\tname main\n\titem_button\n\t{\n\t\tbutton\n\t\t{\n\t\t\trect 0 0.5 0.2 0.6\n\t\t}\n\t\ttextid Exit\n\t\tcolor1 255 240 240 240\n\t}\n}\n").unwrap();
        let d = &m.children[0];
        assert_eq!(d.get_str("name"), Some("main"));
        let b = &d.children[0];
        assert_eq!(b.child("button").and_then(|x| x.rect()), Some([0.0, 0.5, 0.2, 0.6]));
        assert_eq!(b.get_str("textid"), Some("Exit"));
        assert_eq!(b.color("color1").map(|c| c[3]), Some(1.0));
    }

    #[test]
    fn string_table_pairs() {
        let t = StringTable::parse("Testing\r\nTesting\r\nexit_confirm\r\nExit?\r\n");
        assert_eq!(t.get("EXIT_CONFIRM"), Some("Exit?"));
    }
}
