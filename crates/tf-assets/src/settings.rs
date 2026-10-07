//! Player settings (`scripts/players/**/*.set`): Valve-style keyvalues with `#base`
//! inheritance, `[$sp]`/`[$mp]` conditions and `ClassMods` blocks.

use std::collections::HashMap;

/// Flattened settings: `section.key` -> value (e.g. `stand.speed`, `global.dodgeSpeed`).
#[derive(Debug, Clone, Default)]
pub struct PlayerSettings {
    pub values: HashMap<String, String>,
}

impl PlayerSettings {
    /// Parse `name` and its `#base` chain. `read` returns a file's text by path.
    pub fn load(path: &str, sp: bool, read: &mut dyn FnMut(&str) -> Option<String>) -> Option<Self> {
        let text = read(path)?;
        let mut out = Self::default();
        // Base files first, so the derived file overrides them.
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("#base") {
                let base = rest.trim().trim_matches('"');
                if let Some(b) = Self::load(&format!("{dir}/{base}"), sp, read) {
                    out.values.extend(b.values);
                }
            }
        }
        out.parse(&text, sp);
        Some(out)
    }

    fn parse(&mut self, text: &str, sp: bool) {
        let tokens = tokenize(text);
        let mut stack: Vec<String> = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            match tokens[i].as_str() {
                "{" => {
                    // The block's name is the previous bare token (already pushed below).
                    i += 1;
                }
                "}" => {
                    stack.pop();
                    i += 1;
                }
                t if t.starts_with("#") => i += 2,
                key => {
                    let next = tokens.get(i + 1).map(String::as_str).unwrap_or("");
                    if next == "{" {
                        stack.push(key.to_string());
                        i += 2;
                        continue;
                    }
                    let mut value = next.to_string();
                    i += 2;
                    let mut ok = true;
                    if let Some(cond) = tokens.get(i).filter(|t| t.starts_with("[$")) {
                        ok = (cond == "[$sp]") == sp || (cond != "[$sp]" && cond != "[$mp]");
                        i += 1;
                    }
                    // Skip the outer class block name and anything inside ClassMods.
                    if !ok || stack.iter().any(|s| s.eq_ignore_ascii_case("ClassMods")) {
                        continue;
                    }
                    let section = if stack.len() >= 2 { stack[1].as_str() } else { "" };
                    value = value.trim().to_string();
                    // Weapon mods: `Mods { <mod> { key value } }` also keep their mod name.
                    if stack.len() >= 3 && stack[1].eq_ignore_ascii_case("Mods") {
                        self.values.insert(format!("mods.{}.{key}", stack[2]).to_ascii_lowercase(), value.clone());
                    }
                    self.values.insert(format!("{section}.{key}").to_ascii_lowercase(), value);
                }
            }
        }
    }

    /// Names of the weapon mods defined in a `Mods` block.
    pub fn mod_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.values.keys().filter_map(|k| k.strip_prefix("mods.")?.split_once('.').map(|(m, _)| m.to_string())).collect();
        v.sort();
        v.dedup();
        v
    }

    /// These settings with weapon mods applied, in order. A mod value replaces the base value,
    /// or changes it: `*x` multiplies, `/x` divides, `++x` adds, `--x` subtracts. Both the
    /// top-level key and the `sp_base`/`mp_base` copies are changed, so readers that prefer a
    /// base block see the modded value.
    pub fn with_mods(&self, mods: &[&str]) -> Self {
        let mut out = self.clone();
        for m in mods {
            let prefix = format!("mods.{}.", m.to_ascii_lowercase());
            let entries: Vec<(String, String)> = self.values.iter().filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.to_string(), v.clone()))).collect();
            for (key, v) in entries {
                for section in ["", "sp_base", "mp_base"] {
                    let full = format!("{section}.{key}");
                    let base = out.values.get(&full).cloned();
                    if section != "" && base.is_none() {
                        continue;
                    }
                    if let Some(new) = apply_mod(base.as_deref(), &v) {
                        out.values.insert(full, new);
                    }
                }
            }
        }
        out
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(&key.to_ascii_lowercase()).map(String::as_str)
    }

    pub fn f32(&self, key: &str, default: f32) -> f32 {
        self.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    pub fn vec3(&self, key: &str) -> Option<[f32; 3]> {
        let v: Vec<f32> = self.get(key)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        let mut chars = line.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else if c == '"' {
                chars.next();
                let mut s = String::new();
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    s.push(c);
                }
                out.push(s);
            } else if c == '{' || c == '}' {
                out.push(c.to_string());
                chars.next();
            } else {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                out.push(s);
            }
        }
    }
    out
}

/// One mod value against a base value (see `with_mods`).
fn apply_mod(base: Option<&str>, v: &str) -> Option<String> {
    let num = |s: &str| s.parse::<f32>().ok();
    let b = base.and_then(num);
    let op = |rest: &str, f: fn(f32, f32) -> f32| Some(fmt(f(b.unwrap_or(0.0), num(rest)?)));
    if let Some(r) = v.strip_prefix("++") {
        op(r, |a, x| a + x)
    } else if let Some(r) = v.strip_prefix("--") {
        op(r, |a, x| a - x)
    } else if let Some(r) = v.strip_prefix('*') {
        op(r, |a, x| a * x)
    } else if let Some(r) = v.strip_prefix('/') {
        op(r.trim_start_matches('/'), |a, x| if x != 0.0 { a / x } else { a })
    } else {
        Some(v.to_string())
    }
}

fn fmt(x: f32) -> String {
    if x.fract() == 0.0 && x.abs() < 1e9 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_apply_in_place() {
        let text = r#"WeaponData
{
    "reload_time" "2.0"
    "ammo_clip_size" "24"
    SP_BASE
    {
        "damage_near_value" "20"
    }
    Mods
    {
        pas_fast_reload { "reload_time" "*0.7" }
        extended_ammo { "ammo_clip_size" "30" }
        bump { "damage_near_value" "++10" }
    }
}"#;
        let mut read = |_: &str| Some(text.to_string());
        let s = PlayerSettings::load("w.txt", true, &mut read).unwrap();
        assert_eq!(s.mod_names(), vec!["bump", "extended_ammo", "pas_fast_reload"]);
        let m = s.with_mods(&["pas_fast_reload", "extended_ammo", "bump"]);
        assert!((m.f32(".reload_time", 0.0) - 1.4).abs() < 1e-5);
        assert_eq!(m.get(".ammo_clip_size"), Some("30"));
        assert_eq!(m.get("sp_base.damage_near_value"), Some("30"));
        assert_eq!(s.get(".reload_time"), Some("2.0"));
    }
}
