//! Little-endian binary buffers with typed accessors (glTF-like).
//!
//! A [`BufferTable`] describes buffers (files inside the pack), views (byte
//! ranges of a buffer) and accessors (typed arrays over a view). Documents
//! refer to accessors by their index in the table.

use crate::validate::Issues;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Index of an accessor in the owning document's [`BufferTable`].
pub type AccessorId = u32;

/// Raw bytes of every buffer file, keyed by buffer `uri`.
pub type BufferBytes = BTreeMap<String, Vec<u8>>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BufferTable {
    #[serde(default)]
    pub buffers: Vec<BufferDef>,
    #[serde(default)]
    pub views: Vec<BufferView>,
    #[serde(default)]
    pub accessors: Vec<Accessor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BufferDef {
    /// Relative path inside the pack, forward slashes, no `..`.
    pub uri: String,
    pub byte_length: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BufferView {
    pub buffer: u32,
    pub byte_offset: u64,
    pub byte_length: u64,
    /// Distance between elements; `None` means tightly packed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_stride: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComponentType {
    U8,
    I16,
    U16,
    U32,
    F32,
}

impl ComponentType {
    pub fn size(self) -> u64 {
        match self {
            Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::U32 | Self::F32 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    Scalar,
    Vec2,
    Vec3,
    Vec4,
}

impl ElementKind {
    pub fn components(self) -> u64 {
        match self {
            Self::Scalar => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Accessor {
    pub view: u32,
    #[serde(default)]
    pub byte_offset: u64,
    pub component: ComponentType,
    pub kind: ElementKind,
    pub count: u32,
    /// Integer components map to [0,1] (unsigned) or [-1,1] (signed) on read.
    #[serde(default)]
    pub normalized: bool,
}

impl Accessor {
    pub fn element_size(&self) -> u64 {
        self.component.size() * self.kind.components()
    }
}

impl BufferTable {
    /// Structural validation of the table itself (ranges, strides, alignment).
    pub fn validate(&self, path: &str, bytes: Option<&BufferBytes>, out: &mut Issues) {
        for (i, b) in self.buffers.iter().enumerate() {
            let p = format!("{path}.buffers[{i}]");
            if !crate::validate::is_safe_relative_path(&b.uri) {
                out.add(
                    &p,
                    format!(
                        "uri {:?} must be a relative forward-slash path without '..'",
                        b.uri
                    ),
                );
            }
            if let Some(map) = bytes {
                match map.get(&b.uri) {
                    None => out.add(
                        &p,
                        format!("buffer file {:?} is missing from the pack", b.uri),
                    ),
                    Some(data) if data.len() as u64 != b.byte_length => out.add(
                        &p,
                        format!(
                            "byte_length {} does not match file size {}",
                            b.byte_length,
                            data.len()
                        ),
                    ),
                    _ => {}
                }
            }
        }
        for (i, v) in self.views.iter().enumerate() {
            let p = format!("{path}.views[{i}]");
            match self.buffers.get(v.buffer as usize) {
                None => out.add(
                    &p,
                    format!(
                        "buffer index {} out of range ({} buffers)",
                        v.buffer,
                        self.buffers.len()
                    ),
                ),
                Some(b) => {
                    if v.byte_offset
                        .checked_add(v.byte_length)
                        .is_none_or(|e| e > b.byte_length)
                    {
                        out.add(
                            &p,
                            format!(
                                "range {}+{} exceeds buffer length {}",
                                v.byte_offset, v.byte_length, b.byte_length
                            ),
                        );
                    }
                }
            }
            if v.byte_stride == Some(0) {
                out.add(&p, "byte_stride must be greater than zero");
            }
        }
        for (i, a) in self.accessors.iter().enumerate() {
            let p = format!("{path}.accessors[{i}]");
            let Some(v) = self.views.get(a.view as usize) else {
                out.add(
                    &p,
                    format!(
                        "view index {} out of range ({} views)",
                        a.view,
                        self.views.len()
                    ),
                );
                continue;
            };
            let elem = a.element_size();
            let stride = u64::from(v.byte_stride.unwrap_or(0));
            let stride = if stride == 0 { elem } else { stride };
            if stride < elem {
                out.add(
                    &p,
                    format!("view stride {stride} smaller than element size {elem}"),
                );
                continue;
            }
            let cs = a.component.size();
            if a.byte_offset % cs != 0 || v.byte_offset % cs != 0 {
                out.add(&p, "accessor offset is not aligned to its component size");
            }
            if a.normalized && matches!(a.component, ComponentType::F32 | ComponentType::U32) {
                out.add(
                    &p,
                    "normalized is only valid for 8/16-bit integer components",
                );
            }
            if a.count > 0 {
                let end = a
                    .byte_offset
                    .checked_add(stride * (u64::from(a.count) - 1) + elem);
                if end.is_none_or(|e| e > v.byte_length) {
                    out.add(
                        &p,
                        format!(
                            "{} elements of {} bytes (stride {}) at offset {} exceed view length {}",
                            a.count, elem, stride, a.byte_offset, v.byte_length
                        ),
                    );
                }
            }
        }
    }

    /// Check that accessor `id` exists with the expected layout; records an issue otherwise.
    pub fn expect(
        &self,
        id: AccessorId,
        path: &str,
        components: &[ComponentType],
        kind: ElementKind,
        count: Option<u32>,
        out: &mut Issues,
    ) -> bool {
        let Some(a) = self.accessors.get(id as usize) else {
            out.add(
                path,
                format!(
                    "accessor {id} out of range ({} accessors)",
                    self.accessors.len()
                ),
            );
            return false;
        };
        let mut ok = true;
        if !components.contains(&a.component) {
            out.add(
                path,
                format!(
                    "accessor {id} has component {:?}, expected one of {components:?}",
                    a.component
                ),
            );
            ok = false;
        }
        if a.kind != kind {
            out.add(
                path,
                format!("accessor {id} has kind {:?}, expected {kind:?}", a.kind),
            );
            ok = false;
        }
        if let Some(c) = count {
            if a.count != c {
                out.add(
                    path,
                    format!("accessor {id} has count {}, expected {c}", a.count),
                );
                ok = false;
            }
        }
        ok
    }

    fn slice<'a>(
        &self,
        id: AccessorId,
        bytes: &'a BufferBytes,
    ) -> Result<(&Accessor, &'a [u8], usize), String> {
        let a = self
            .accessors
            .get(id as usize)
            .ok_or_else(|| format!("accessor {id} out of range"))?;
        let v = self.views.get(a.view as usize).ok_or("view out of range")?;
        let b = self
            .buffers
            .get(v.buffer as usize)
            .ok_or("buffer out of range")?;
        let data = bytes
            .get(&b.uri)
            .ok_or_else(|| format!("buffer {:?} not loaded", b.uri))?;
        let elem = a.element_size() as usize;
        let stride = v.byte_stride.map_or(elem, |s| s as usize).max(elem);
        let base = (v.byte_offset + a.byte_offset) as usize;
        if a.count > 0 {
            let span = stride * (a.count as usize - 1) + elem;
            if base + span > data.len() || a.byte_offset as usize + span > v.byte_length as usize {
                return Err(format!("accessor {id} reads past the end of its buffer"));
            }
        }
        Ok((a, &data[base.min(data.len())..], stride))
    }

    /// Read any accessor as flattened `f32` (integers are converted; normalized ones are scaled).
    pub fn read_f32(&self, id: AccessorId, bytes: &BufferBytes) -> Result<Vec<f32>, String> {
        let (a, data, stride) = self.slice(id, bytes)?;
        let n = a.kind.components() as usize;
        let cs = a.component.size() as usize;
        let mut out = Vec::with_capacity(a.count as usize * n);
        for e in 0..a.count as usize {
            for c in 0..n {
                let o = e * stride + c * cs;
                let v = match a.component {
                    ComponentType::F32 => f32::from_le_bytes(data[o..o + 4].try_into().unwrap()),
                    ComponentType::U8 => {
                        let x = f32::from(data[o]);
                        if a.normalized { x / 255.0 } else { x }
                    }
                    ComponentType::U16 => {
                        let x = f32::from(u16::from_le_bytes([data[o], data[o + 1]]));
                        if a.normalized { x / 65535.0 } else { x }
                    }
                    ComponentType::I16 => {
                        let x = f32::from(i16::from_le_bytes([data[o], data[o + 1]]));
                        if a.normalized {
                            (x / 32767.0).max(-1.0)
                        } else {
                            x
                        }
                    }
                    ComponentType::U32 => {
                        u32::from_le_bytes(data[o..o + 4].try_into().unwrap()) as f32
                    }
                };
                out.push(v);
            }
        }
        Ok(out)
    }

    /// Read an unsigned integer accessor (u8/u16/u32) as flattened `u32`.
    pub fn read_u32(&self, id: AccessorId, bytes: &BufferBytes) -> Result<Vec<u32>, String> {
        let (a, data, stride) = self.slice(id, bytes)?;
        let n = a.kind.components() as usize;
        let cs = a.component.size() as usize;
        let mut out = Vec::with_capacity(a.count as usize * n);
        for e in 0..a.count as usize {
            for c in 0..n {
                let o = e * stride + c * cs;
                out.push(match a.component {
                    ComponentType::U8 => u32::from(data[o]),
                    ComponentType::U16 => u32::from(u16::from_le_bytes([data[o], data[o + 1]])),
                    ComponentType::U32 => u32::from_le_bytes(data[o..o + 4].try_into().unwrap()),
                    other => {
                        return Err(format!(
                            "accessor {id}: {other:?} is not an unsigned integer type"
                        ));
                    }
                });
            }
        }
        Ok(out)
    }

    /// Read a signed 16-bit accessor (PCM16 audio) as flattened `i16`.
    pub fn read_i16(&self, id: AccessorId, bytes: &BufferBytes) -> Result<Vec<i16>, String> {
        let (a, data, stride) = self.slice(id, bytes)?;
        if a.component != ComponentType::I16 {
            return Err(format!("accessor {id}: {:?} is not i16", a.component));
        }
        let n = a.kind.components() as usize;
        let mut out = Vec::with_capacity(a.count as usize * n);
        for e in 0..a.count as usize {
            for c in 0..n {
                let o = e * stride + c * 2;
                out.push(i16::from_le_bytes([data[o], data[o + 1]]));
            }
        }
        Ok(out)
    }
}

/// Incremental builder that appends tightly packed little-endian arrays to one buffer.
#[derive(Clone, Debug)]
pub struct BufferBuilder {
    bytes: Vec<u8>,
    table: BufferTable,
}

impl BufferBuilder {
    pub fn new(uri: &str) -> Self {
        let table = BufferTable {
            buffers: vec![BufferDef {
                uri: uri.to_string(),
                byte_length: 0,
            }],
            ..BufferTable::default()
        };
        Self {
            bytes: Vec::new(),
            table,
        }
    }

    fn push(
        &mut self,
        raw: Vec<u8>,
        component: ComponentType,
        kind: ElementKind,
        count: u32,
        normalized: bool,
    ) -> AccessorId {
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        let offset = self.bytes.len() as u64;
        let len = raw.len() as u64;
        self.bytes.extend(raw);
        self.table.views.push(BufferView {
            buffer: 0,
            byte_offset: offset,
            byte_length: len,
            byte_stride: None,
        });
        self.table.accessors.push(Accessor {
            view: (self.table.views.len() - 1) as u32,
            byte_offset: 0,
            component,
            kind,
            count,
            normalized,
        });
        (self.table.accessors.len() - 1) as AccessorId
    }

    pub fn add_f32(&mut self, kind: ElementKind, values: &[f32]) -> AccessorId {
        let n = kind.components() as usize;
        let raw = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.push(
            raw,
            ComponentType::F32,
            kind,
            (values.len() / n) as u32,
            false,
        )
    }

    pub fn add_u16(&mut self, values: &[u16]) -> AccessorId {
        let raw = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.push(
            raw,
            ComponentType::U16,
            ElementKind::Scalar,
            values.len() as u32,
            false,
        )
    }

    pub fn add_u32(&mut self, values: &[u32]) -> AccessorId {
        let raw = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.push(
            raw,
            ComponentType::U32,
            ElementKind::Scalar,
            values.len() as u32,
            false,
        )
    }

    pub fn add_i16(&mut self, values: &[i16]) -> AccessorId {
        let raw = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.push(
            raw,
            ComponentType::I16,
            ElementKind::Scalar,
            values.len() as u32,
            false,
        )
    }

    /// Scalar `u8` array (indices, surface ids).
    pub fn add_u8(&mut self, values: &[u8]) -> AccessorId {
        self.push(
            values.to_vec(),
            ComponentType::U8,
            ElementKind::Scalar,
            values.len() as u32,
            false,
        )
    }

    /// RGBA8 pixels (4 bytes per element, normalized).
    pub fn add_rgba8(&mut self, pixels: &[u8]) -> AccessorId {
        self.push(
            pixels.to_vec(),
            ComponentType::U8,
            ElementKind::Vec4,
            (pixels.len() / 4) as u32,
            true,
        )
    }

    /// Finish: returns the table and the `(uri, bytes)` pair to store in the pack.
    pub fn finish(mut self) -> (BufferTable, String, Vec<u8>) {
        self.table.buffers[0].byte_length = self.bytes.len() as u64;
        let uri = self.table.buffers[0].uri.clone();
        (self.table, uri, self.bytes)
    }
}
