//! Immutable labels for the machine-facing library API.
use std::fmt::Display;
include!(concat!(env!("OUT_DIR"), "/english.rs"));

/// Constant lookup also lets plugin choice lists live entirely in read-only data.
pub const fn t(key: &str) -> &'static str {
    let wanted = key.as_bytes();
    let mut low = 0;
    let mut high = ENGLISH.len();
    while low < high {
        let mid = low + (high - low) / 2;
        let candidate = ENGLISH[mid].0.as_bytes();
        let mut i = 0;
        while i < candidate.len() && i < wanted.len() && candidate[i] == wanted[i] {
            i += 1;
        }
        if i == candidate.len() && i == wanted.len() {
            return ENGLISH[mid].1;
        }
        if i == candidate.len() || (i < wanted.len() && candidate[i] < wanted[i]) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    "Missing translation"
}

pub fn tf(key: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut text = t(key).to_owned();
    for (key, value) in args {
        text = text.replace(&format!("{{{key}}}"), &value.to_string());
    }
    text
}

pub fn tn(key: &str, n: u64) -> String {
    tnf(key, n, &[])
}

pub fn tnf(key: &str, n: u64, args: &[(&str, &dyn Display)]) -> String {
    let key = format!("{key}.{}", if n == 1 { "one" } else { "other" });
    let mut args = args.to_vec();
    args.push(("n", &n));
    tf(&key, &args)
}

#[macro_export]
macro_rules! tf {
    ($key:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::tf($key, &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),*])
    };
}

#[macro_export]
macro_rules! tn {
    ($key:expr, $n:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::tnf($key, $n, &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),*])
    };
}
