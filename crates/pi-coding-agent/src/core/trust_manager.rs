//! Port of `packages/coding-agent/src/core/trust-manager.ts`.

use crate::config::CONFIG_DIR_NAME;
use crate::utils::lockfile::{LockError, LockGuard, LockOptions, lock_sync_with_retry};
use crate::utils::paths::{canonicalize_path, node_dirname, node_join, resolve_path};
use crate::utils::text::strip_bom;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

pub type ProjectTrustDecision = Option<bool>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustStoreEntry {
    pub path: String,
    pub decision: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustUpdate {
    pub path: String,
    pub decision: ProjectTrustDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrustOption {
    pub label: String,
    pub trusted: bool,
    pub updates: Vec<ProjectTrustUpdate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_path: Option<String>,
}

pub type TrustFile = IndexMap<String, Option<bool>>;

const TRUST_REQUIRING_PROJECT_CONFIG_RESOURCES: &[&str] = &[
    "settings.json",
    "mcp.json",
    "extensions",
    "skills",
    "prompts",
    "themes",
    "SYSTEM.md",
    "APPEND_SYSTEM.md",
];

fn normalize_cwd(cwd: &str) -> String {
    canonicalize_path(&resolve_path(cwd, None, None))
}

fn find_nearest_trust_entry(data: &TrustFile, cwd: &str) -> Option<ProjectTrustStoreEntry> {
    let mut current_dir = normalize_cwd(cwd);
    loop {
        if let Some(Some(decision)) = data.get(&current_dir) {
            return Some(ProjectTrustStoreEntry {
                path: current_dir,
                decision: *decision,
            });
        }
        let parent_dir = node_dirname(&current_dir);
        if parent_dir == current_dir {
            return None;
        }
        current_dir = parent_dir;
    }
}

pub fn get_project_trust_parent_path(cwd: &str) -> Option<String> {
    let trust_path = normalize_cwd(cwd);
    let parent_dir = node_dirname(&trust_path);
    if parent_dir == trust_path {
        None
    } else {
        Some(parent_dir)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProjectTrustOptionsOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_session_only: Option<bool>,
}

pub fn get_project_trust_options(
    cwd: &str,
    options: Option<&GetProjectTrustOptionsOptions>,
) -> Vec<ProjectTrustOption> {
    let include_session_only = options.and_then(|o| o.include_session_only).unwrap_or(false);
    let trust_path = normalize_cwd(cwd);
    let mut trust_options = vec![ProjectTrustOption {
        label: "Trust".to_string(),
        trusted: true,
        updates: vec![ProjectTrustUpdate {
            path: trust_path.clone(),
            decision: Some(true),
        }],
        saved_path: Some(trust_path.clone()),
    }];
    if let Some(parent_path) = get_project_trust_parent_path(cwd) {
        trust_options.push(ProjectTrustOption {
            label: format!("Trust parent folder ({parent_path})"),
            trusted: true,
            updates: vec![
                ProjectTrustUpdate {
                    path: parent_path.clone(),
                    decision: Some(true),
                },
                ProjectTrustUpdate {
                    path: trust_path.clone(),
                    decision: None,
                },
            ],
            saved_path: Some(parent_path),
        });
    }
    if include_session_only {
        trust_options.push(ProjectTrustOption {
            label: "Trust (this session only)".to_string(),
            trusted: true,
            updates: vec![],
            saved_path: None,
        });
    }
    trust_options.push(ProjectTrustOption {
        label: "Do not trust".to_string(),
        trusted: false,
        updates: vec![ProjectTrustUpdate {
            path: trust_path.clone(),
            decision: Some(false),
        }],
        saved_path: Some(trust_path),
    });
    if include_session_only {
        trust_options.push(ProjectTrustOption {
            label: "Do not trust (this session only)".to_string(),
            trusted: false,
            updates: vec![],
            saved_path: None,
        });
    }
    trust_options
}

fn read_trust_file(path: &str) -> anyhow::Result<TrustFile> {
    if !Path::new(path).exists() {
        return Ok(TrustFile::new());
    }

    let parsed: Value = std::fs::read_to_string(path)
        .map_err(anyhow::Error::from)
        .and_then(|content| Ok(serde_json::from_str(&strip_bom(&content))?))
        .map_err(|error| anyhow::anyhow!("Failed to read trust store {path}: {error}"))?;

    let Value::Object(entries) = parsed else {
        anyhow::bail!("Invalid trust store {path}: expected an object");
    };

    let mut data = TrustFile::new();
    for (key, value) in entries {
        let decision = match value {
            Value::Bool(b) => Some(b),
            Value::Null => None,
            _ => anyhow::bail!(
                "Invalid trust store {path}: value for {} must be true, false, or null",
                serde_json::to_string(&key).unwrap_or_default()
            ),
        };
        data.insert(key, decision);
    }
    Ok(data)
}

fn write_trust_file(path: &str, data: &TrustFile) -> anyhow::Result<()> {
    let mut keys: Vec<&String> = data.keys().collect();
    keys.sort_by(|a, b| crate::core::trust_manager::js_compare(a, b));
    let mut sorted = serde_json::Map::new();
    for key in keys {
        sorted.insert(
            key.clone(),
            match data[key] {
                Some(b) => Value::Bool(b),
                None => Value::Null,
            },
        );
    }
    std::fs::create_dir_all(node_dirname(path))?;
    std::fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(&Value::Object(sorted))?),
    )?;
    Ok(())
}

/// Default `Array.prototype.sort` order: UTF-16 code unit comparison.
pub(crate) fn js_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn acquire_trust_lock_sync(path: &str) -> anyhow::Result<LockGuard> {
    let trust_dir = node_dirname(path);
    std::fs::create_dir_all(&trust_dir)?;
    let options = LockOptions {
        lockfile_path: Some(format!("{path}.lock").into()),
        ..Default::default()
    };
    lock_sync_with_retry(&trust_dir, &options, 10, Duration::from_millis(20)).map_err(|e| match e {
        LockError::Locked => anyhow::anyhow!("Lock file is already being held"),
        LockError::Io(e) => anyhow::Error::new(e),
    })
}

fn with_trust_file_lock<T>(path: &str, f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    let _release = acquire_trust_lock_sync(path)?;
    f()
}

/// Returns true when cwd has project-local resources that must be gated by
/// project trust: trust-requiring entries under cwd/.pi, or .agents/skills in
/// cwd or one of its ancestors. Returns false when no such project resources
/// exist. The user/global ~/.agents/skills directory is always treated as a
/// trusted user resource and is ignored here, even when cwd is $HOME.
pub fn has_trust_requiring_project_resources(cwd: &str) -> bool {
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| dirs::home_dir().map(|h| h.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let home_dir = canonicalize_path(&resolve_path(&home, None, None));
    let user_agents_skills_dir = node_join(&[&home_dir, ".agents", "skills"]);
    let mut current_dir = canonicalize_path(&resolve_path(cwd, None, None));

    let config_dir = node_join(&[&current_dir, CONFIG_DIR_NAME]);
    if TRUST_REQUIRING_PROJECT_CONFIG_RESOURCES
        .iter()
        .any(|entry| Path::new(&node_join(&[&config_dir, entry])).exists())
    {
        return true;
    }

    loop {
        let agents_skills_dir = node_join(&[&current_dir, ".agents", "skills"]);
        if agents_skills_dir != user_agents_skills_dir && Path::new(&agents_skills_dir).exists() {
            return true;
        }
        let parent_dir = node_dirname(&current_dir);
        if parent_dir == current_dir {
            return false;
        }
        current_dir = parent_dir;
    }
}

pub struct ProjectTrustStore {
    trust_path: String,
}

impl ProjectTrustStore {
    pub fn new(agent_dir: &str) -> Self {
        Self {
            trust_path: node_join(&[&resolve_path(agent_dir, None, None), "trust.json"]),
        }
    }

    pub fn get(&self, cwd: &str) -> anyhow::Result<ProjectTrustDecision> {
        Ok(self.get_entry(cwd)?.map(|entry| entry.decision))
    }

    pub fn get_entry(&self, cwd: &str) -> anyhow::Result<Option<ProjectTrustStoreEntry>> {
        with_trust_file_lock(&self.trust_path, || {
            let data = read_trust_file(&self.trust_path)?;
            Ok(find_nearest_trust_entry(&data, cwd))
        })
    }

    pub fn set(&self, cwd: &str, decision: ProjectTrustDecision) -> anyhow::Result<()> {
        self.set_many(&[ProjectTrustUpdate {
            path: cwd.to_string(),
            decision,
        }])
    }

    pub fn set_many(&self, decisions: &[ProjectTrustUpdate]) -> anyhow::Result<()> {
        with_trust_file_lock(&self.trust_path, || {
            let mut data = read_trust_file(&self.trust_path)?;
            for update in decisions {
                let key = normalize_cwd(&update.path);
                match update.decision {
                    None => {
                        data.shift_remove(&key);
                    }
                    Some(decision) => {
                        data.insert(key, Some(decision));
                    }
                }
            }
            write_trust_file(&self.trust_path, &data)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_nearest_trust_entries() {
        let temp = tempfile::tempdir().unwrap();
        let agent_dir = temp.path().join("agent");
        let project = temp.path().join("project");
        let nested = project.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let store = ProjectTrustStore::new(agent_dir.to_str().unwrap());

        assert_eq!(store.get(project.to_str().unwrap()).unwrap(), None);
        store.set(project.to_str().unwrap(), Some(true)).unwrap();
        assert_eq!(store.get(nested.to_str().unwrap()).unwrap(), Some(true));
        let entry = store.get_entry(nested.to_str().unwrap()).unwrap().unwrap();
        assert_eq!(entry.path, canonicalize_path(project.to_str().unwrap()));

        store.set(nested.to_str().unwrap(), Some(false)).unwrap();
        assert_eq!(store.get(nested.to_str().unwrap()).unwrap(), Some(false));
        store.set(nested.to_str().unwrap(), None).unwrap();
        assert_eq!(store.get(nested.to_str().unwrap()).unwrap(), Some(true));

        let content = std::fs::read_to_string(agent_dir.join("trust.json")).unwrap();
        assert!(content.ends_with("}\n"));
        assert!(!agent_dir.join("trust.json.lock").exists());
    }

    #[test]
    fn rejects_invalid_trust_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("trust.json"), "[]").unwrap();
        let store = ProjectTrustStore::new(temp.path().to_str().unwrap());
        let err = store.get(temp.path().to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("expected an object"));

        std::fs::write(temp.path().join("trust.json"), "{\"/x\": 1}").unwrap();
        let err = store.get(temp.path().to_str().unwrap()).unwrap_err();
        assert!(
            err.to_string()
                .contains("value for \"/x\" must be true, false, or null")
        );
    }

    #[test]
    fn trust_options_include_parent_and_session_only_entries() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = canonicalize_path(temp.path().to_str().unwrap());
        let parent = node_dirname(&cwd);
        let options = get_project_trust_options(
            &cwd,
            Some(&GetProjectTrustOptionsOptions {
                include_session_only: Some(true),
            }),
        );
        let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Trust".to_string(),
                format!("Trust parent folder ({parent})"),
                "Trust (this session only)".to_string(),
                "Do not trust".to_string(),
                "Do not trust (this session only)".to_string(),
            ]
        );
        assert_eq!(
            options[1].updates,
            vec![
                ProjectTrustUpdate {
                    path: parent.clone(),
                    decision: Some(true)
                },
                ProjectTrustUpdate {
                    path: cwd.clone(),
                    decision: None
                },
            ]
        );
    }

    #[test]
    fn detects_trust_requiring_project_resources() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        assert!(!has_trust_requiring_project_resources(
            project.to_str().unwrap()
        ));
        std::fs::create_dir_all(project.join(".pi")).unwrap();
        assert!(!has_trust_requiring_project_resources(
            project.to_str().unwrap()
        ));
        std::fs::write(project.join(".pi").join("settings.json"), "{}").unwrap();
        assert!(has_trust_requiring_project_resources(
            project.to_str().unwrap()
        ));

        let other = temp.path().join("other").join("child");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::create_dir_all(temp.path().join("other").join(".agents").join("skills")).unwrap();
        assert!(has_trust_requiring_project_resources(other.to_str().unwrap()));
    }
}
