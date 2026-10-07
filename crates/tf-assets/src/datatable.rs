//! RPAK datatables (`dtbl` v0), the game's CSV-like tables such as
//! `datatable/titan_properties.rpak`. Layout (from RSX / LegionPlus):
//! - header: column count (i32), row count (i32), column array (ptr), row data (ptr),
//!   row stride (u32);
//! - column (16 bytes): name (ptr), type (u32), offset of the cell in each row (u32);
//! - types: 0 bool, 1 int, 2 float, 3 vector, 4 string, 5 asset, 6 asset (no precache);
//!   strings and assets are pointers to C strings.

use crate::rpak::Rpak;

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Bool(bool),
    Int(i32),
    Float(f32),
    Vector([f32; 3]),
    Str(String),
}

impl Cell {
    pub fn text(&self) -> String {
        match self {
            Cell::Bool(b) => b.to_string(),
            Cell::Int(i) => i.to_string(),
            Cell::Float(f) => f.to_string(),
            Cell::Vector(v) => format!("{} {} {}", v[0], v[1], v[2]),
            Cell::Str(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DataTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

impl DataTable {
    /// Read the datatable asset named `name` (e.g. "datatable/titan_properties.rpak").
    pub fn load(pak: &Rpak, name: &str) -> Option<Self> {
        let a = pak.asset(crate::string_to_guid(name))?;
        if a.kind_str() != "dtbl" {
            return None;
        }
        let h = a.head?;
        let ncol = pak.u32(h) as usize;
        let nrow = pak.u32(h + 4) as usize;
        let cols = pak.ptr(h + 8)?;
        let rows = pak.ptr(h + 0x10)?;
        let stride = pak.u32(h + 0x18) as usize;
        let mut columns = Vec::with_capacity(ncol);
        let mut kinds = Vec::with_capacity(ncol);
        for c in 0..ncol {
            let o = cols + c * 16;
            columns.push(pak.ptr(o).map(|p| pak.cstr(p)).unwrap_or_default());
            kinds.push((pak.u32(o + 8), pak.u32(o + 12) as usize));
        }
        let mut out = Vec::with_capacity(nrow);
        for r in 0..nrow {
            let base = rows + r * stride;
            let row = kinds
                .iter()
                .map(|&(t, off)| {
                    let o = base + off;
                    match t {
                        0 => Cell::Bool(pak.u32(o) != 0),
                        1 => Cell::Int(pak.u32(o) as i32),
                        2 => Cell::Float(f32::from_bits(pak.u32(o))),
                        3 => Cell::Vector([0, 4, 8].map(|k| f32::from_bits(pak.u32(o + k)))),
                        _ => Cell::Str(pak.ptr(o).map(|p| pak.cstr(p)).unwrap_or_default()),
                    }
                })
                .collect();
            out.push(row);
        }
        Some(Self { columns, rows: out })
    }

    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.eq_ignore_ascii_case(name))
    }
}
