//! Port of `packages/coding-agent/src/utils/output-files.ts`.
//!
//! Output files: files pi writes so the model can use output it was not shown in full, such as the
//! full text of truncated tool output, binary MCP resources, and images shown by codemode scripts.
//! Every output file is created here, so where they are stored can change in one place. Today they
//! go to the OS temp directory.

use std::fs::{File, OpenOptions};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Output can carry private data, so only the user may read the files.
#[cfg(unix)]
const OUTPUT_FILE_MODE: u32 = 0o600;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WriteOutputFileData {
    String(String),
    List(Vec<u8>),
}

impl WriteOutputFileData {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::String(s) => s.as_bytes(),
            Self::List(b) => b,
        }
    }
}

/// A new, unused path: `<dir>/<prefix>-<random hex><extension>`. `extension` includes the dot.
fn create_output_file_path(prefix: &str, extension: &str) -> PathBuf {
    let random: [u8; 8] = rand_bytes();
    let hex: String = random.iter().map(|b| format!("{b:02x}")).collect();
    std::env::temp_dir().join(format!("{prefix}-{hex}{extension}"))
}

fn rand_bytes() -> [u8; 8] {
    let uuid = uuid::Uuid::new_v4();
    let mut out = [0u8; 8];
    out.copy_from_slice(&uuid.as_bytes()[..8]);
    out
}

/// `wx`: create a new file, never following a link someone else placed at the path.
fn open_new(path: &PathBuf) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(OUTPUT_FILE_MODE);
    }
    options.open(path)
}

/// Write `data` to a new output file and return its path.
pub async fn write_output_file(prefix: &str, extension: &str, data: &WriteOutputFileData) -> anyhow::Result<String> {
    let path = create_output_file_path(prefix, extension);
    let bytes = data.as_bytes().to_vec();
    let target = path.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;
        let mut file = open_new(&target)?;
        file.write_all(&bytes)
    })
    .await??;
    Ok(path.to_string_lossy().into_owned())
}

/// A new output file opened for streamed output.
pub struct CreateOutputFileStreamResult {
    pub path: String,
    pub stream: File,
}

/// Open a new output file for streamed output.
pub fn create_output_file_stream(prefix: &str, extension: &str) -> anyhow::Result<CreateOutputFileStreamResult> {
    let path = create_output_file_path(prefix, extension);
    let stream = open_new(&path)?;
    Ok(CreateOutputFileStreamResult { path: path.to_string_lossy().into_owned(), stream })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_private_output_files() {
        let path = write_output_file("pi-test-output", ".txt", &WriteOutputFileData::String("hi".into()))
            .await
            .unwrap();
        assert!(path.ends_with(".txt"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hi");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_file(path).unwrap();
        let created = create_output_file_stream("pi-test-output", ".bin").unwrap();
        assert!(std::path::Path::new(&created.path).exists());
        std::fs::remove_file(created.path).unwrap();
    }
}
