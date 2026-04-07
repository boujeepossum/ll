use crate::level::Level;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
pub struct Data {
    pub map: BTreeMap<String, DataEntry>,
}

impl Data {
    pub fn empty() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }
}

impl Data {
    pub fn add<S: Into<String>, V: Into<DataValue>>(&mut self, key: S, value: V) {
        let (key, tags) = crate::utils::extract_tags(key.into());
        let data_entry = DataEntry(value.into(), tags);
        self.map.insert(key, data_entry);
    }

    pub fn merge(&mut self, other: &Data) {
        for (k, v) in &other.map {
            self.map.insert(k.clone(), v.clone());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    // Filter out data entries that are not supposed to be logger for
    // the set log level, based on event tags.
    // e.g. if the event is `some_event#trace` and current level is Info,
    // we would not want to log it.
    pub fn filter_for_level(&mut self, level: Level) {
        let mut to_remove = vec![];
        for (key, entry) in &self.map {
            let entry_log_level = crate::utils::extract_log_level_from_tags(&entry.1);

            if let Some(entry_log_level) = entry_log_level {
                if entry_log_level > level {
                    to_remove.push(key.clone());
                }
            }
        }

        for key_to_remove in &to_remove {
            self.map.remove(key_to_remove);
        }
    }
}

#[derive(Debug, Clone)]
pub enum DataValue {
    String(String),
    Int(i64),
    Float(f64),
    None,
}

impl std::fmt::Display for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut result = String::new();
        for (k, v) in &self.map {
            result.push_str(&format!("  {k}: {}\n", v.0));
        }
        write!(f, "{result}")
    }
}

impl std::fmt::Display for DataValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let result = match self {
            DataValue::String(string) => string.to_owned(),
            DataValue::Int(i) => format!("{i}"),
            DataValue::Float(f) => format!("{f}"),
            DataValue::None => String::new(),
        };
        write!(f, "{result}")
    }
}

#[derive(Debug, Clone)]
pub struct DataEntry(pub DataValue, pub BTreeSet<String>);

impl<'a> From<&'a str> for DataValue {
    fn from(v: &'a str) -> Self {
        DataValue::String(v.to_owned())
    }
}

impl<'a> From<&'a String> for DataValue {
    fn from(v: &'a String) -> Self {
        DataValue::String(v.clone())
    }
}

impl From<String> for DataValue {
    fn from(v: String) -> Self {
        DataValue::String(v)
    }
}

impl From<bool> for DataValue {
    fn from(v: bool) -> Self {
        DataValue::String(format!("{v}"))
    }
}

impl From<f64> for DataValue {
    fn from(value: f64) -> Self {
        DataValue::Float(value)
    }
}

impl From<Option<String>> for DataValue {
    fn from(maybe_value: Option<String>) -> Self {
        match maybe_value {
            Some(v) => v.into(),
            None => DataValue::None,
        }
    }
}

impl From<&Option<String>> for DataValue {
    fn from(maybe_value: &Option<String>) -> Self {
        match maybe_value {
            Some(v) => v.into(),
            None => DataValue::None,
        }
    }
}

macro_rules! from_int_types {
    ( $( $t:ty ),* ) => {
        $(
            impl From<$t> for DataValue {
                fn from(value: $t) -> Self {
                    DataValue::Int(value as i64)
                }
            }
        )*
    };
}

from_int_types!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
