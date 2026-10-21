//! Port of `packages/coding-agent/src/core/pi-manifest.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub themes: Option<Vec<String>>,
}

pub fn read_pi_manifest(package_json_path: &str) -> Option<PiManifest> {
    let Ok(text) = std::fs::read_to_string(package_json_path) else {
        return None;
    };
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(crate::utils::text::strip_bom(&text).as_str()) else {
        return None;
    };
    let pi = pkg.as_object()?.get("pi")?.as_object()?;
    let field = |name: &str| -> Option<Vec<String>> {
        let entries = pi.get(name)?.as_array()?;
        entries.iter().map(|e| e.as_str().map(str::to_string)).collect()
    };
    Some(PiManifest {
        extensions: field("extensions"),
        skills: field("skills"),
        prompts: field("prompts"),
        themes: field("themes"),
    })
}
