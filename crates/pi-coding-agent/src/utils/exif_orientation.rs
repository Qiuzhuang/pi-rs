//! Port of `packages/coding-agent/src/utils/exif-orientation.ts`.

use serde_json::Value;
use std::sync::Arc;

pub type Photon = Value;

pub type DstIndexFn = Arc<dyn Fn(f64, f64, f64, f64) -> f64 + Send + Sync>;

pub fn apply_exif_orientation(photon: &Photon, image: Value, original_bytes: &[u8]) -> Value {
    todo!("port utils/exif-orientation.ts:applyExifOrientation")
}
