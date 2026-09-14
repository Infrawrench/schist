//! A bounded, data-only subset of MS-NRBF. It never instantiates .NET types.
//! References stay as IDs so cycles in metadata cannot recurse indefinitely.
use std::collections::HashMap;

use anyhow::{ensure, Result};
use schist_i18n::t;

use crate::layered::{invalid, unsupported, Reader};

#[derive(Clone, Debug)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Text(String),
    Ref(i32),
    Null,
    Object(Object),
    Array(Vec<Value>),
    Bytes(Vec<u8>),
    Nulls(usize),
}

#[derive(Clone, Debug)]
pub struct Object {
    pub name: String,
    pub fields: HashMap<String, Value>,
}

impl Object {
    pub fn field(&self, name: &str) -> Result<&Value> {
        self.fields.get(name).ok_or_else(invalid)
    }
    pub fn int(&self, name: &str) -> Result<i64> {
        match self.field(name)? {
            Value::Int(n) => Ok(*n),
            _ => Err(invalid()),
        }
    }
    pub fn boolean(&self, name: &str) -> Result<bool> {
        match self.field(name)? {
            Value::Bool(v) => Ok(*v),
            _ => Err(invalid()),
        }
    }
}

#[derive(Clone)]
struct Class {
    name: String,
    fields: Vec<String>,
    primitives: Vec<Option<u8>>,
}

pub struct Graph {
    pub root: i32,
    pub values: HashMap<i32, Value>,
    pub order: Vec<i32>,
}

impl Graph {
    pub fn resolve<'a>(&'a self, mut value: &'a Value) -> Result<&'a Value> {
        for _ in 0..64 {
            if let Value::Ref(id) = value {
                value = self.values.get(id).ok_or_else(invalid)?;
            } else {
                return Ok(value);
            }
        }
        Err(invalid())
    }
    pub fn object<'a>(&'a self, value: &'a Value) -> Result<&'a Object> {
        match self.resolve(value)? {
            Value::Object(o) => Ok(o),
            _ => Err(invalid()),
        }
    }
    pub fn array<'a>(&'a self, value: &'a Value) -> Result<&'a [Value]> {
        match self.resolve(value)? {
            Value::Array(a) => Ok(a),
            _ => Err(invalid()),
        }
    }
    pub fn text<'a>(&'a self, value: &'a Value) -> Result<&'a str> {
        match self.resolve(value)? {
            Value::Text(s) => Ok(s),
            _ => Err(invalid()),
        }
    }
}

pub fn read(r: &mut Reader<'_>) -> Result<Graph> {
    ensure!(r.byte()? == 0, "{}", t("codec.layered.invalid"));
    let root = r.le32()?;
    r.le32()?; // header ID
    ensure!(
        r.le32()? == 1 && r.le32()? == 0,
        "{}",
        t("codec.layered.invalid")
    );
    let mut p = Parser {
        graph: Graph {
            root,
            values: HashMap::new(),
            order: Vec::new(),
        },
        classes: HashMap::new(),
        remaining: 1_000_000,
        string_bytes: 16 * 1024 * 1024,
    };
    loop {
        let tag = r.byte()?;
        if tag == 11 {
            break;
        }
        p.record(r, tag, 0)?;
    }
    ensure!(
        p.graph.values.contains_key(&root),
        "{}",
        t("codec.layered.invalid")
    );
    Ok(p.graph)
}

struct Parser {
    graph: Graph,
    classes: HashMap<i32, Class>,
    remaining: usize,
    string_bytes: usize,
}

fn string(r: &mut Reader<'_>) -> Result<String> {
    let mut len = 0u32;
    for shift in (0..35).step_by(7) {
        let b = r.byte()?;
        ensure!(shift < 28 || b <= 7, "{}", t("codec.layered.invalid"));
        len |= ((b & 127) as u32) << shift;
        if b < 128 {
            ensure!(len <= 1024 * 1024, "{}", t("codec.layered.too_large"));
            return String::from_utf8(r.take(len as usize)?.to_vec()).map_err(|_| invalid());
        }
    }
    Err(invalid())
}

impl Parser {
    fn charge_strings(&mut self, len: usize) -> Result<()> {
        self.string_bytes = self.string_bytes.checked_sub(len).ok_or_else(invalid)?;
        Ok(())
    }
    fn string(&mut self, r: &mut Reader<'_>) -> Result<String> {
        let value = string(r)?;
        self.charge_strings(value.len())?;
        Ok(value)
    }
    fn charge(&mut self, n: usize) -> Result<()> {
        self.remaining = self.remaining.checked_sub(n).ok_or_else(invalid)?;
        Ok(())
    }
    fn store(&mut self, id: i32, value: Value) -> Result<Value> {
        ensure!(
            id != 0 && !self.graph.values.contains_key(&id),
            "{}",
            t("codec.layered.invalid")
        );
        self.graph.order.push(id);
        self.graph.values.insert(id, value);
        Ok(Value::Ref(id))
    }
    fn primitive(&mut self, r: &mut Reader<'_>, kind: u8) -> Result<Value> {
        Ok(match kind {
            1 => {
                let b = r.byte()?;
                ensure!(b <= 1, "{}", t("codec.layered.invalid"));
                Value::Bool(b != 0)
            }
            2 => Value::Int(r.byte()? as i64),
            10 => Value::Int(r.byte()? as i8 as i64),
            7 => Value::Int(i16::from_le_bytes(r.take(2)?.try_into()?) as i64),
            8 => Value::Int(r.le32()? as i64),
            9 | 12 | 13 => Value::Int(i64::from_le_bytes(r.take(8)?.try_into()?)),
            14 => Value::Int(u16::from_le_bytes(r.take(2)?.try_into()?) as i64),
            15 => Value::Int(r.le32()? as u32 as i64),
            16 => Value::Int(u64::from_le_bytes(r.take(8)?.try_into()?) as i64),
            // These occur only in ancillary metadata, which PDN import does
            // not interpret. Still consume their exact wire representation.
            6 => {
                r.take(8)?;
                Value::Null
            }
            11 => {
                r.take(4)?;
                Value::Null
            }
            5 | 18 => Value::Text(self.string(r)?),
            3 => {
                let b = r.byte()?;
                let len = match b {
                    0..=127 => 1,
                    194..=223 => 2,
                    224..=239 => 3,
                    _ => return Err(invalid()),
                };
                r.take(len - 1)?;
                Value::Null
            }
            _ => return Err(unsupported("PDN primitive type")),
        })
    }
    fn type_info(&mut self, r: &mut Reader<'_>, kind: u8) -> Result<Option<u8>> {
        Ok(match kind {
            0 => Some(r.byte()?),
            1 | 2 | 5 | 6 => None,
            3 => {
                self.string(r)?;
                None
            }
            4 => {
                self.string(r)?;
                r.le32()?;
                None
            }
            7 => {
                r.byte()?;
                None
            }
            _ => return Err(invalid()),
        })
    }
    fn value(&mut self, r: &mut Reader<'_>, depth: usize) -> Result<Value> {
        loop {
            let tag = r.byte()?;
            if tag == 12 {
                self.record(r, tag, depth)?;
                continue;
            }
            return self.record(r, tag, depth);
        }
    }
    fn members(
        &mut self,
        r: &mut Reader<'_>,
        id: i32,
        class: Class,
        depth: usize,
    ) -> Result<Value> {
        let mut fields = HashMap::new();
        for (name, primitive) in class.fields.into_iter().zip(class.primitives) {
            let value = match primitive {
                Some(p) => self.primitive(r, p)?,
                None => self.value(r, depth + 1)?,
            };
            ensure!(
                !matches!(value, Value::Nulls(_)),
                "{}",
                t("codec.layered.invalid")
            );
            ensure!(
                fields.insert(name, value).is_none(),
                "{}",
                t("codec.layered.invalid")
            );
        }
        self.store(
            id,
            Value::Object(Object {
                name: class.name,
                fields,
            }),
        )
    }
    fn array(
        &mut self,
        r: &mut Reader<'_>,
        id: i32,
        count: usize,
        primitive: Option<u8>,
        depth: usize,
    ) -> Result<Value> {
        self.charge(count)?;
        if primitive == Some(2) {
            return self.store(id, Value::Bytes(r.take(count)?.to_vec()));
        }
        let mut values = Vec::with_capacity(count);
        while values.len() < count {
            let v = if let Some(p) = primitive {
                self.primitive(r, p)?
            } else {
                self.value(r, depth + 1)?
            };
            if let Value::Nulls(n) = v {
                ensure!(
                    n > 0 && n <= count - values.len(),
                    "{}",
                    t("codec.layered.invalid")
                );
                values.resize(values.len() + n, Value::Null);
            } else {
                values.push(v);
            }
        }
        self.store(id, Value::Array(values))
    }
    fn record(&mut self, r: &mut Reader<'_>, tag: u8, depth: usize) -> Result<Value> {
        self.charge(1)?;
        ensure!(depth <= 64, "{}", t("codec.layered.invalid"));
        match tag {
            1 => {
                let id = r.le32()?;
                let metadata = r.le32()?;
                let class = self.classes.get(&metadata).ok_or_else(invalid)?.clone();
                self.charge(class.fields.len())?;
                self.charge_strings(
                    class.name.len() + class.fields.iter().map(String::len).sum::<usize>(),
                )?;
                self.members(r, id, class, depth)
            }
            4 | 5 => {
                let id = r.le32()?;
                let name = self.string(r)?;
                let count = r.le32()? as usize;
                ensure!(count <= 4096, "{}", t("codec.layered.invalid"));
                self.charge(count)?;
                let mut fields = Vec::new();
                for _ in 0..count {
                    fields.push(self.string(r)?);
                }
                let types = r.take(count)?.to_vec();
                let mut primitives = Vec::new();
                for kind in types {
                    primitives.push(self.type_info(r, kind)?);
                }
                if tag == 5 {
                    r.le32()?;
                }
                let class = Class {
                    name,
                    fields,
                    primitives,
                };
                self.charge_strings(
                    class.name.len() + class.fields.iter().map(String::len).sum::<usize>(),
                )?;
                ensure!(
                    !self.classes.contains_key(&id),
                    "{}",
                    t("codec.layered.invalid")
                );
                self.classes.insert(id, class.clone());
                self.members(r, id, class, depth)
            }
            6 => {
                let id = r.le32()?;
                let s = self.string(r)?;
                self.store(id, Value::Text(s))
            }
            7 => {
                let id = r.le32()?;
                let kind = r.byte()?;
                let rank = r.le32()?;
                ensure!(
                    rank == 1 && kind == 0,
                    "{}",
                    unsupported("PDN multidimensional array")
                );
                let count = r.le32()? as usize;
                let kind = r.byte()?;
                let primitive = self.type_info(r, kind)?;
                self.array(r, id, count, primitive, depth)
            }
            8 => {
                let kind = r.byte()?;
                self.primitive(r, kind)
            }
            9 => Ok(Value::Ref(r.le32()?)),
            10 => Ok(Value::Null),
            12 => {
                r.le32()?;
                self.string(r)?;
                Ok(Value::Null)
            }
            13 => Ok(Value::Nulls(r.byte()? as usize)),
            14 => Ok(Value::Nulls(r.le32()? as usize)),
            15..=17 => {
                let id = r.le32()?;
                let count = r.le32()? as usize;
                let primitive = if tag == 15 { Some(r.byte()?) } else { None };
                self.array(r, id, count, primitive, depth)
            }
            _ => Err(unsupported("PDN serialization record")),
        }
    }
}

/// Small NRBF writer for the PDN3 interchange object graph. Object fields
/// use typed metadata, and references permit the LayerList parent cycle.
pub struct Writer {
    pub bytes: Vec<u8>,
    next_id: i32,
}

pub enum Field<'a> {
    Int(i32),
    Long(i64),
    Byte(u8),
    Bool(bool),
    Ref(i32),
    Null,
    Text(&'a str),
}

impl Writer {
    pub fn new() -> Self {
        let mut w = Self {
            bytes: vec![0],
            next_id: 2,
        };
        for n in [1, -1, 1, 0] {
            w.int(n);
        }
        w.bytes.push(12);
        w.int(1);
        w.string(
            "PaintDotNet.Data, Version=3.510.4297.28969, Culture=neutral, PublicKeyToken=null",
        );
        w.bytes.push(12);
        w.int(2);
        w.string("System, Version=2.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089");
        w.bytes.push(12);
        w.int(3);
        w.string(
            "PaintDotNet.Core, Version=3.510.4297.28965, Culture=neutral, PublicKeyToken=null",
        );
        w
    }
    pub fn id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    fn int(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn string(&mut self, value: &str) {
        let mut len = value.len();
        while len >= 128 {
            self.bytes.push((len as u8 & 127) | 128);
            len >>= 7;
        }
        self.bytes.push(len as u8);
        self.bytes.extend_from_slice(value.as_bytes());
    }
    pub fn class(
        &mut self,
        id: i32,
        name: &str,
        library: Option<i32>,
        fields: &[(&str, Field<'_>)],
    ) {
        self.bytes.push(if library.is_some() { 5 } else { 4 });
        self.int(id);
        self.string(name);
        self.int(fields.len() as i32);
        for (name, _) in fields {
            self.string(name);
        }
        for (_, field) in fields {
            self.bytes.push(
                if matches!(
                    field,
                    Field::Int(_) | Field::Long(_) | Field::Byte(_) | Field::Bool(_)
                ) {
                    0
                } else {
                    2
                },
            );
        }
        for (_, field) in fields {
            match field {
                Field::Int(_) => self.bytes.push(8),
                Field::Long(_) => self.bytes.push(9),
                Field::Byte(_) => self.bytes.push(2),
                Field::Bool(_) => self.bytes.push(1),
                _ => {}
            }
        }
        if let Some(library) = library {
            self.int(library);
        }
        for (_, field) in fields {
            match field {
                Field::Int(v) => self.int(*v),
                Field::Long(v) => self.bytes.extend_from_slice(&v.to_le_bytes()),
                Field::Byte(v) => self.bytes.push(*v),
                Field::Bool(v) => self.bytes.push(*v as u8),
                Field::Ref(id) => {
                    self.bytes.push(9);
                    self.int(*id);
                }
                Field::Null => self.bytes.push(10),
                Field::Text(s) => {
                    let id = self.id();
                    self.bytes.push(6);
                    self.int(id);
                    self.string(s);
                }
            }
        }
    }
    pub fn array(&mut self, id: i32, values: &[i32]) {
        self.bytes.push(16);
        self.int(id);
        self.int(values.len() as i32);
        for value in values {
            self.bytes.push(9);
            self.int(*value);
        }
    }
    pub fn string_array(&mut self, id: i32) {
        self.bytes.push(17);
        self.int(id);
        self.int(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_cycles_are_inert_references() {
        let mut writer = Writer::new();
        writer.class(1, "Cycle", Some(1), &[("self", Field::Ref(1))]);
        writer.bytes.push(11);
        let graph = read(&mut Reader::new(&writer.bytes)).unwrap();
        let root = Value::Ref(1);
        let object = graph.object(&root).unwrap();
        assert_eq!(
            graph.object(object.field("self").unwrap()).unwrap().name,
            "Cycle"
        );
    }

    #[test]
    fn duplicate_objects_and_unbounded_arrays_are_rejected() {
        let mut writer = Writer::new();
        writer.class(1, "Duplicate", Some(1), &[]);
        writer.class(1, "Duplicate", Some(1), &[]);
        writer.bytes.push(11);
        assert!(read(&mut Reader::new(&writer.bytes)).is_err());
        let mut writer = Writer::new();
        writer.bytes.push(16);
        writer.int(1);
        writer.int(i32::MAX);
        writer.bytes.push(11);
        assert!(read(&mut Reader::new(&writer.bytes)).is_err());
    }
}
