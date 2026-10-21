//! Port of `packages/coding-agent/src/modes/interactive/external-editor.ts`.

use crate::utils::text::strip_bom;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEditorOptions {
    pub command: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all_fields = "camelCase")]
pub enum ExternalEditorResult {
    #[serde(rename = "complete")]
    Complete { content: String },
    #[serde(rename = "failed")]
    Failed {},
}

/// `fs.mkdtempSync(prefix)`: a new private (0700) directory named `<prefix>XXXXXX`.
pub(crate) fn make_temp_dir(prefix: &str) -> std::io::Result<PathBuf> {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    for _ in 0..100 {
        let bytes = uuid::Uuid::new_v4();
        let suffix: String = bytes.as_bytes()[..6].iter().map(|b| CHARS[*b as usize % CHARS.len()] as char).collect();
        let path = std::env::temp_dir().join(format!("{prefix}{suffix}"));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("could not create a temporary directory"))
}

pub async fn edit_in_external_editor(options: &ExternalEditorOptions) -> anyhow::Result<ExternalEditorResult> {
    let directory = make_temp_dir("pi-editor-")?;
    let file_path = directory.join("prompt.md");
    let result = run_editor(options, &file_path).await;
    // Cleanup is best effort.
    let _ = std::fs::remove_dir_all(&directory);
    result
}

async fn run_editor(options: &ExternalEditorOptions, file_path: &Path) -> anyhow::Result<ExternalEditorResult> {
    std::fs::write(file_path, &options.content)?;
    let mut parts = options.command.split(' ');
    let editor = parts.next().unwrap_or_default().to_string();
    let editor_args: Vec<String> = parts.map(str::to_string).collect();
    {
        let mut stdout = std::io::stdout();
        let _ = write!(
            stdout,
            "Launching external editor: {}\nPi will resume when the editor exits.\n",
            options.command
        );
        let _ = stdout.flush();
    }

    let mut command = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(&editor);
        c
    } else {
        tokio::process::Command::new(&editor)
    };
    command
        .args(&editor_args)
        .arg(file_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    let exit_code = match command.status().await {
        Ok(status) => status.code(),
        Err(_) => None,
    };
    if exit_code != Some(0) {
        return Ok(ExternalEditorResult::Failed {});
    }
    let content = strip_bom(&std::fs::read_to_string(file_path)?);
    let content = content.strip_suffix('\n').unwrap_or(&content).to_string();
    Ok(ExternalEditorResult::Complete { content })
}
