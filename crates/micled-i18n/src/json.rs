//! JSON mit erhaltener Reihenfolge der Schlüssel, damit die String Catalogs
//! nach dem Schreiben byte-gleich bleiben, solange sich nichts ändert.
//! `serde_json` mit `preserve_order` ginge auch, würde aber im Workspace für
//! alle Crates gelten.

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut J> {
        match self {
            J::Obj(m) => m.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn entries(&self) -> &[(String, J)] {
        match self {
            J::Obj(m) => m,
            _ => &[],
        }
    }

    pub fn str(s: &str) -> J {
        J::Str(s.to_string())
    }

    pub fn obj<const N: usize>(pairs: [(&str, J); N]) -> J {
        J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    /// Wie Xcode: zwei Leerzeichen, `": "`, ohne Zeilenumbruch am Ende
    pub fn pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("JSON schreibt immer")
    }

    pub fn parse(text: &str) -> Result<J, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }
}

impl Serialize for J {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            J::Null => s.serialize_unit(),
            J::Bool(b) => s.serialize_bool(*b),
            J::Num(n) => n.serialize(s),
            J::Str(v) => s.serialize_str(v),
            J::Arr(a) => {
                let mut seq = s.serialize_seq(Some(a.len()))?;
                for v in a {
                    seq.serialize_element(v)?;
                }
                seq.end()
            }
            J::Obj(m) => {
                let mut map = s.serialize_map(Some(m.len()))?;
                for (k, v) in m {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<J, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = J;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON")
            }
            fn visit_unit<E>(self) -> Result<J, E> {
                Ok(J::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<J, E> {
                Ok(J::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_u64<E>(self, n: u64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, n: f64) -> Result<J, E> {
                serde_json::Number::from_f64(n).map(J::Num).ok_or_else(|| E::custom("Zahl"))
            }
            fn visit_str<E>(self, s: &str) -> Result<J, E> {
                Ok(J::Str(s.to_string()))
            }
            fn visit_string<E>(self, s: String) -> Result<J, E> {
                Ok(J::Str(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(J::Arr(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut v = Vec::new();
                while let Some((k, x)) = a.next_entry::<String, J>()? {
                    v.push((k, x));
                }
                Ok(J::Obj(v))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_and_format() {
        let text = "{\n  \"b\": {\n    \"z\": \"ä \\\" \\\\\",\n    \"a\": false\n  },\n  \"a\": [\n    1,\n    null\n  ],\n  \"e\": {}\n}";
        assert_eq!(J::parse(text).unwrap().pretty(), text);
    }
}
