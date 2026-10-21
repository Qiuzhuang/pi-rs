//! Port of `packages/coding-agent/src/core/timings.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingNamespaceTimings {
    pub label: String,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingNamespace {
    pub timings: Vec<TimingNamespaceTimings>,
    #[serde(with = "pi_ai::serde_util::js_f64")]
    pub last_time: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimingLabel {
    #[serde(rename = "main")]
    Main,
    #[serde(rename = "extensions")]
    Extensions,
}

impl TimingLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Extensions => "extensions",
        }
    }
}

pub fn reset_timings(namespace: Option<&TimingLabel>) {
    todo!("port core/timings.ts:resetTimings")
}

pub fn time(label: &str, namespace: Option<&TimingLabel>) {
    todo!("port core/timings.ts:time")
}

pub fn print_timings() {
    todo!("port core/timings.ts:printTimings")
}
