//! Particle system definitions (`particles/*.pcf` in `englishclient_frontend.bsp.pak000_dir.vpk`),
//! stored as Valve DMX binary (header `<!-- dmx encoding binary 5 format pcf 2 -->`).
//!
//! DMX binary 5 layout (worked out against the game's files):
//! - the header line, NUL-terminated;
//! - a string table: i32 count, then NUL-terminated strings;
//! - i32 element count; per element its type and name (string-table indices) and a 16-byte GUID;
//! - per element: i32 attribute count; per attribute its name (string index), a type byte and the
//!   value. Types: 1 element (i32 index, -1 = none), 2 int, 3 float, 4 bool, 5 string (string
//!   index), 6 binary (i32 length + bytes), 7 time (i32), 8 colour (4 bytes), 9-11 vector2-4,
//!   12 qangle, 13 quaternion, 14 matrix; 15+ are arrays of type-14 (string arrays hold inline
//!   strings).
//!
//! The root element lists `particleSystemDefinitions`. Each `DmeParticleSystemDefinition` has its
//! system-level attributes (material, max_particles, colour, radius, ...), and lists of
//! `DmeParticleOperator`s (renderers, operators, initializers, emitters, forces, constraints)
//! identified by `functionName`, plus `DmeParticleChild`ren (child system, delay, end cap).

use anyhow::{bail, Context, Result};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum Value {
    Element(i32),
    Int(i32),
    Float(f32),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    Time(i32),
    Color([u8; 4]),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Matrix([f32; 16]),
    Array(Vec<Value>),
}

impl Value {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) | Value::Time(i) => Some(*i as f32),
            Value::Bool(b) => Some(*b as u8 as f32),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Element {
    pub type_name: String,
    pub name: String,
    pub attrs: Vec<(String, Value)>,
}

impl Element {
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let s = self.b.get(self.i..self.i + n).context("DMX truncated")?;
        self.i += n;
        Ok(s)
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn cstr(&mut self) -> Result<String> {
        let end = self.b[self.i..].iter().position(|&c| c == 0).context("DMX unterminated string")?;
        let s = String::from_utf8_lossy(&self.b[self.i..self.i + end]).into_owned();
        self.i += end + 1;
        Ok(s)
    }
    fn floats<const N: usize>(&mut self) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Ok(out)
    }
}

/// Parse a DMX binary (version 5) file into its elements.
pub fn parse_dmx(b: &[u8]) -> Result<Vec<Element>> {
    let header_end = b.iter().position(|&c| c == 0).context("no DMX header")?;
    let header = String::from_utf8_lossy(&b[..header_end]);
    if !header.contains("dmx encoding binary") {
        bail!("not binary DMX: {header}");
    }
    let mut r = Reader { b, i: header_end + 1 };
    let ns = r.i32()?.max(0) as usize;
    let mut strings = Vec::with_capacity(ns);
    for _ in 0..ns {
        strings.push(r.cstr()?);
    }
    let s = |i: i32| strings.get(i as usize).cloned().unwrap_or_default();
    let ne = r.i32()?.max(0) as usize;
    let mut elems = Vec::with_capacity(ne);
    for _ in 0..ne {
        let t = r.i32()?;
        let n = r.i32()?;
        r.take(16)?;
        elems.push(Element { type_name: s(t), name: s(n), attrs: Vec::new() });
    }
    fn value(r: &mut Reader, t: u8, strings: &[String]) -> Result<Value> {
        Ok(match t {
            1 => Value::Element(r.i32()?),
            2 => Value::Int(r.i32()?),
            3 => Value::Float(r.f32()?),
            4 => Value::Bool(r.take(1)?[0] != 0),
            5 => Value::Str(strings.get(r.i32()? as usize).cloned().unwrap_or_default()),
            6 => {
                let n = r.i32()?.max(0) as usize;
                Value::Bytes(r.take(n)?.to_vec())
            }
            7 => Value::Time(r.i32()?),
            8 => Value::Color(r.take(4)?.try_into().unwrap()),
            9 => Value::Vec2(r.floats::<2>()?),
            10 | 12 => Value::Vec3(r.floats::<3>()?),
            11 | 13 => Value::Vec4(r.floats::<4>()?),
            14 => Value::Matrix(r.floats::<16>()?),
            _ => bail!("unknown DMX attribute type {t}"),
        })
    }
    for e in elems.iter_mut() {
        let na = r.i32()?.max(0) as usize;
        for _ in 0..na {
            let name = s(r.i32()?);
            let t = r.take(1)?[0];
            let v = if t >= 15 {
                let n = r.i32()?.max(0) as usize;
                let et = t - 14;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(if et == 5 { Value::Str(r.cstr()?) } else { value(&mut r, et, &strings)? });
                }
                Value::Array(items)
            } else {
                value(&mut r, t, &strings)?
            };
            e.attrs.push((name, v));
        }
    }
    Ok(elems)
}

/// One operator (emitter, initializer, operator, renderer, force or constraint).
#[derive(Clone, Debug, Default)]
pub struct Op {
    pub function: String,
    pub params: HashMap<String, Value>,
}

impl Op {
    pub fn f(&self, k: &str, d: f32) -> f32 {
        self.params.get(k).and_then(Value::as_f32).unwrap_or(d)
    }
    pub fn i(&self, k: &str, d: i32) -> i32 {
        self.params.get(k).and_then(Value::as_f32).map(|v| v as i32).unwrap_or(d)
    }
    pub fn b(&self, k: &str, d: bool) -> bool {
        self.params.get(k).and_then(Value::as_f32).map(|v| v != 0.0).unwrap_or(d)
    }
    pub fn v3(&self, k: &str, d: [f32; 3]) -> [f32; 3] {
        match self.params.get(k) {
            Some(Value::Vec3(v)) => *v,
            Some(Value::Vec4(v)) => [v[0], v[1], v[2]],
            _ => d,
        }
    }
    pub fn color(&self, k: &str, d: [u8; 4]) -> [u8; 4] {
        match self.params.get(k) {
            Some(Value::Color(c)) => *c,
            _ => d,
        }
    }
    pub fn s(&self, k: &str) -> Option<&str> {
        match self.params.get(k) {
            Some(Value::Str(s)) => Some(s),
            _ => None,
        }
    }
    /// A `graph` attribute: (x, y) points.
    pub fn points(&self, k: &str) -> Vec<[f32; 2]> {
        match self.params.get(k) {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(|v| match v {
                    Value::Vec2(p) => Some(*p),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Child {
    pub name: String,
    pub delay: f32,
    /// Played when the parent is stopped rather than when it starts.
    pub end_cap: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SystemDef {
    pub name: String,
    /// Material path as written (e.g. `particle\muzzleflash\flash_round_01.vmt`).
    pub material: String,
    pub max_particles: usize,
    pub initial_particles: usize,
    /// Starting radius and colour of every particle before initializers run.
    pub radius: f32,
    pub color: [u8; 4],
    pub hdr_scale: f32,
    /// Drawn in screen space (HUD-like effects).
    pub screen_space: bool,
    pub emitters: Vec<Op>,
    pub initializers: Vec<Op>,
    pub operators: Vec<Op>,
    pub renderers: Vec<Op>,
    pub forces: Vec<Op>,
    pub constraints: Vec<Op>,
    pub children: Vec<Child>,
}

/// Every particle system defined in one `.pcf` file. Muted operators are left out.
pub fn parse_pcf(b: &[u8]) -> Result<Vec<SystemDef>> {
    let els = parse_dmx(b)?;
    let elem = |v: &Value| match v {
        Value::Element(i) if *i >= 0 => els.get(*i as usize),
        _ => None,
    };
    let ops = |e: &Element, k: &str| -> Vec<Op> {
        match e.get(k) {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(&elem)
                .map(|o| Op {
                    function: o.get("functionName").and_then(|v| if let Value::Str(s) = v { Some(s.clone()) } else { None }).unwrap_or_else(|| o.name.clone()),
                    params: o.attrs.iter().filter(|(n, _)| n != "functionName").cloned().collect(),
                })
                .filter(|o| o.f("mute", 0.0) == 0.0)
                .collect(),
            _ => Vec::new(),
        }
    };
    let mut out = Vec::new();
    for e in els.iter().filter(|e| e.type_name == "DmeParticleSystemDefinition") {
        let num = |k: &str, d: f32| e.get(k).and_then(Value::as_f32).unwrap_or(d);
        let children = match e.get("children") {
            Some(Value::Array(a)) => a
                .iter()
                .filter_map(&elem)
                .filter(|c| c.get("mute").and_then(Value::as_f32).unwrap_or(0.0) == 0.0)
                .filter_map(|c| {
                    let child = c.get("child").and_then(&elem)?;
                    Some(Child {
                        name: child.name.clone(),
                        delay: c.get("delay").and_then(Value::as_f32).unwrap_or(0.0),
                        end_cap: c.get("end cap effect").and_then(Value::as_f32).unwrap_or(0.0) != 0.0,
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        out.push(SystemDef {
            name: e.name.clone(),
            material: match e.get("material") {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            },
            max_particles: num("max_particles", 1000.0).max(0.0) as usize,
            initial_particles: num("initial_particles", 0.0).max(0.0) as usize,
            radius: num("radius", 5.0),
            color: match e.get("color") {
                Some(Value::Color(c)) => *c,
                _ => [255; 4],
            },
            hdr_scale: num("color HDR scale", 1.0),
            screen_space: num("screen space effect", 0.0) != 0.0 || num("draw with screen space", 0.0) != 0.0,
            emitters: ops(e, "emitters"),
            initializers: ops(e, "initializers"),
            operators: ops(e, "operators"),
            renderers: ops(e, "renderers"),
            forces: ops(e, "forces"),
            constraints: ops(e, "constraints"),
            children,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built DMX binary 5 file with one system, one emitter and one child.
    fn sample() -> Vec<u8> {
        let strings = ["DmElement", "root", "particleSystemDefinitions", "DmeParticleSystemDefinition", "sys", "material", "m.vmt", "emitters", "DmeParticleOperator", "emit_instantaneously", "functionName", "num_to_emit", "children", "DmeParticleChild", "ch", "child", "delay", "max_particles"];
        let idx = |s: &str| strings.iter().position(|x| *x == s).unwrap() as i32;
        let mut b = b"<!-- dmx encoding binary 5 format pcf 2 -->\n\0".to_vec();
        b.extend((strings.len() as i32).to_le_bytes());
        for s in strings {
            b.extend(s.as_bytes());
            b.push(0);
        }
        let elems = [("DmElement", "root"), ("DmeParticleSystemDefinition", "sys"), ("DmeParticleOperator", "emit_instantaneously"), ("DmeParticleChild", "ch")];
        b.extend((elems.len() as i32).to_le_bytes());
        for (t, n) in elems {
            b.extend(idx(t).to_le_bytes());
            b.extend(idx(n).to_le_bytes());
            b.extend([0u8; 16]);
        }
        let attr = |b: &mut Vec<u8>, name: &str, t: u8| {
            b.extend(idx(name).to_le_bytes());
            b.push(t);
        };
        // root: particleSystemDefinitions = [1]
        b.extend(1i32.to_le_bytes());
        attr(&mut b, "particleSystemDefinitions", 15);
        b.extend(1i32.to_le_bytes());
        b.extend(1i32.to_le_bytes());
        // sys: material, max_particles, emitters = [2], children = [3]
        b.extend(4i32.to_le_bytes());
        attr(&mut b, "material", 5);
        b.extend(idx("m.vmt").to_le_bytes());
        attr(&mut b, "max_particles", 2);
        b.extend(12i32.to_le_bytes());
        attr(&mut b, "emitters", 15);
        b.extend(1i32.to_le_bytes());
        b.extend(2i32.to_le_bytes());
        attr(&mut b, "children", 15);
        b.extend(1i32.to_le_bytes());
        b.extend(3i32.to_le_bytes());
        // emitter: functionName, num_to_emit 7
        b.extend(2i32.to_le_bytes());
        attr(&mut b, "functionName", 5);
        b.extend(idx("emit_instantaneously").to_le_bytes());
        attr(&mut b, "num_to_emit", 2);
        b.extend(7i32.to_le_bytes());
        // child: child = sys (itself, just for the test), delay 0.5
        b.extend(2i32.to_le_bytes());
        attr(&mut b, "child", 1);
        b.extend(1i32.to_le_bytes());
        attr(&mut b, "delay", 3);
        b.extend(0.5f32.to_le_bytes());
        b
    }

    #[test]
    fn parses_a_system() {
        let systems = parse_pcf(&sample()).unwrap();
        assert_eq!(systems.len(), 1);
        let s = &systems[0];
        assert_eq!(s.name, "sys");
        assert_eq!(s.material, "m.vmt");
        assert_eq!(s.max_particles, 12);
        assert_eq!(s.emitters[0].function, "emit_instantaneously");
        assert_eq!(s.emitters[0].i("num_to_emit", 0), 7);
        assert_eq!(s.children[0].name, "sys");
        assert!((s.children[0].delay - 0.5).abs() < 1e-6);
        assert_eq!(s.radius, 5.0);
    }
}
