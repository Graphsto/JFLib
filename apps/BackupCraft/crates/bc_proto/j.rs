// j -- JSON mapping of the wire types (agent_core::json): the Field trait for field values,
// the Json trait for records, and the record! / wire_enum! macros that declare a type and its
// mapping in one place.  Missing or mistyped fields read as their default (the server checks
// what it needs); unknown fields are ignored, so old and new programs understand each other.

use alloc::string::String;
use alloc::vec::Vec;

pub use agent_core::json::Value;

/// a value that maps to one JSON field
pub trait Field: Sized + Default {
    fn put(&self) -> Value;
    fn take(v: Option<&Value>) -> Self;
}

/// a record (JSON object)
pub trait Json: Sized {
    fn to_json(&self) -> Value;
    fn from_json(v: &Value) -> Option<Self>;
}

impl Field for String {
    fn put(&self) -> Value {
        Value::string(self.as_str())
    }
    fn take(v: Option<&Value>) -> Self {
        v.and_then(Value::as_str).map(String::from).unwrap_or_default()
    }
}

impl Field for bool {
    fn put(&self) -> Value {
        Value::Bool(*self)
    }
    fn take(v: Option<&Value>) -> Self {
        v.and_then(Value::as_bool).unwrap_or(false)
    }
}

impl Field for u64 {
    fn put(&self) -> Value {
        Value::u64(*self)
    }
    fn take(v: Option<&Value>) -> Self {
        v.and_then(Value::as_u64).unwrap_or(0)
    }
}

impl Field for i64 {
    fn put(&self) -> Value {
        Value::i64(*self)
    }
    fn take(v: Option<&Value>) -> Self {
        v.and_then(Value::as_i64).unwrap_or(0)
    }
}

impl Field for u32 {
    fn put(&self) -> Value {
        Value::u64(*self as u64)
    }
    fn take(v: Option<&Value>) -> Self {
        v.and_then(Value::as_u64).map_or(0, |x| x.min(u32::MAX as u64) as u32)
    }
}

impl Field for i32 {
    fn put(&self) -> Value {
        Value::i64(*self as i64)
    }
    fn take(v: Option<&Value>) -> Self {
        v.and_then(Value::as_i64).map_or(0, |x| x.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }
}

impl Field for u8 {
    fn put(&self) -> Value {
        Value::u64(*self as u64)
    }
    fn take(v: Option<&Value>) -> Self {
        v.and_then(Value::as_u64).map_or(0, |x| x.min(255) as u8)
    }
}

impl<T: Field> Field for Vec<T> {
    fn put(&self) -> Value {
        Value::Array(self.iter().map(Field::put).collect())
    }
    fn take(v: Option<&Value>) -> Self {
        v.and_then(Value::as_array).map(|a| a.iter().map(|x| T::take(Some(x))).collect()).unwrap_or_default()
    }
}

impl<T: Field> Field for Option<T> {
    fn put(&self) -> Value {
        match self {
            Some(x) => x.put(),
            None => Value::Null,
        }
    }
    fn take(v: Option<&Value>) -> Self {
        match v {
            None | Some(Value::Null) => None,
            Some(x) => Some(T::take(Some(x))),
        }
    }
}

/// declare a record: the struct (Clone, Debug, Default, PartialEq) and its Json / Field
/// mapping; the JSON member names are the field names
#[macro_export]
macro_rules! record {
    ($(#[$m:meta])* pub struct $name:ident { $( $(#[$fm:meta])* pub $f:ident : $t:ty ),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, Default, PartialEq)]
        pub struct $name { $( $(#[$fm])* pub $f: $t ),* }
        impl $crate::j::Json for $name {
            fn to_json(&self) -> $crate::j::Value {
                let mut o = $crate::j::Value::Object(alloc::vec::Vec::new());
                $( o.insert(stringify!($f), $crate::j::Field::put(&self.$f)); )*
                o
            }
            fn from_json(v: &$crate::j::Value) -> Option<Self> {
                v.as_object()?;
                Some($name { $( $f: $crate::j::Field::take(v.get(stringify!($f))) ),* })
            }
        }
        impl $crate::j::Field for $name {
            fn put(&self) -> $crate::j::Value {
                $crate::j::Json::to_json(self)
            }
            fn take(v: Option<&$crate::j::Value>) -> Self {
                v.and_then(<$name as $crate::j::Json>::from_json).unwrap_or_default()
            }
        }
    };
}

/// declare an enum carried as a string: Default is the first variant, as_str / parse, Field
#[macro_export]
macro_rules! wire_enum {
    ($(#[$m:meta])* pub enum $name:ident { $first:ident = $fs:literal $(, $v:ident = $s:literal)* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { #[default] $first $(, $v)* }
        impl $name {
            /// every variant in declaration order
            pub const ALL: &'static [$name] = &[$name::$first $(, $name::$v)*];
            /// the wire name
            pub fn as_str(&self) -> &'static str {
                match self { $name::$first => $fs $(, $name::$v => $s)* }
            }
            /// the variant of a wire name
            pub fn parse(s: &str) -> Option<$name> {
                match s { $fs => Some($name::$first) $(, $s => Some($name::$v))*, _ => None }
            }
        }
        impl $crate::j::Field for $name {
            fn put(&self) -> $crate::j::Value {
                $crate::j::Value::string(self.as_str())
            }
            fn take(v: Option<&$crate::j::Value>) -> Self {
                v.and_then($crate::j::Value::as_str).and_then($name::parse).unwrap_or_default()
            }
        }
    };
}

/// a list of records as a JSON array
pub fn list<T: Json>(items: &[T]) -> Value {
    Value::Array(items.iter().map(Json::to_json).collect())
}

/// the records of a JSON array (or of the array under `items` of an object)
pub fn from_list<T: Json>(v: &Value) -> Vec<T> {
    let arr = v.as_array().or_else(|| v.get("items").and_then(Value::as_array));
    arr.map(|a| a.iter().filter_map(T::from_json).collect()).unwrap_or_default()
}

/// a string member of an object ("" when missing)
pub fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// an unsigned member of an object (0 when missing)
pub fn u64_of(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

/// an object built from (name, value) pairs
pub fn obj<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::object(fields)
}

/// the encoded text of a value (empty on a limit violation)
pub fn text(v: &Value) -> String {
    agent_core::json::stringify_with_limits(v, big()).unwrap_or_default()
}

/// parse with the limits of API bodies (8 MiB, 200 000 values)
pub fn parse(bytes: &[u8]) -> Option<Value> {
    agent_core::json::parse_with_limits(bytes, big()).ok()
}

/// the limits of API bodies and stored records
pub fn big() -> agent_core::json::Limits {
    agent_core::json::Limits { bytes: 8 << 20, depth: 32, values: 200_000, string_bytes: 1 << 20, members: 100_000 }
}
