//! Filesystem helpers that report errors the way Node's `fs` does (`ENOENT: no such file or
//! directory, open '<path>'`, with a `code` property). The tools put these messages in front of
//! the model, so they keep the TS wording.

use std::fmt;
use std::io;

/// A Node-style filesystem error (`err.code`, `err.message`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFsError {
    /// Node error code such as `ENOENT`.
    pub code: String,
    pub message: String,
}

impl fmt::Display for NodeFsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for NodeFsError {}

/// Node error code (`ENOENT`, ...) and libuv description of an IO error.
pub fn io_error_code(err: &io::Error) -> (String, String) {
    #[cfg(unix)]
    if let Some(errno) = err.raw_os_error() {
        let pair = match errno {
            libc::ENOENT => Some(("ENOENT", "no such file or directory")),
            libc::EACCES => Some(("EACCES", "permission denied")),
            libc::EPERM => Some(("EPERM", "operation not permitted")),
            libc::EISDIR => Some(("EISDIR", "illegal operation on a directory")),
            libc::ENOTDIR => Some(("ENOTDIR", "not a directory")),
            libc::EEXIST => Some(("EEXIST", "file already exists")),
            libc::ENOTEMPTY => Some(("ENOTEMPTY", "directory not empty")),
            libc::ENOSPC => Some(("ENOSPC", "no space left on device")),
            libc::EROFS => Some(("EROFS", "read-only file system")),
            libc::EMFILE => Some(("EMFILE", "too many open files")),
            libc::ELOOP => Some(("ELOOP", "too many symbolic links encountered")),
            libc::ENAMETOOLONG => Some(("ENAMETOOLONG", "name too long")),
            libc::EBUSY => Some(("EBUSY", "resource busy or locked")),
            libc::ETXTBSY => Some(("ETXTBSY", "text file is busy")),
            libc::EINVAL => Some(("EINVAL", "invalid argument")),
            libc::EIO => Some(("EIO", "i/o error")),
            libc::EXDEV => Some(("EXDEV", "cross-device link not permitted")),
            libc::EAGAIN => Some(("EAGAIN", "resource temporarily unavailable")),
            _ => None,
        };
        if let Some((code, desc)) = pair {
            return (code.to_string(), desc.to_string());
        }
    }
    let pair = match err.kind() {
        io::ErrorKind::NotFound => ("ENOENT", "no such file or directory"),
        io::ErrorKind::PermissionDenied => ("EACCES", "permission denied"),
        io::ErrorKind::AlreadyExists => ("EEXIST", "file already exists"),
        io::ErrorKind::IsADirectory => ("EISDIR", "illegal operation on a directory"),
        io::ErrorKind::NotADirectory => ("ENOTDIR", "not a directory"),
        io::ErrorKind::DirectoryNotEmpty => ("ENOTEMPTY", "directory not empty"),
        io::ErrorKind::InvalidInput => ("EINVAL", "invalid argument"),
        _ => ("EIO", "i/o error"),
    };
    (pair.0.to_string(), pair.1.to_string())
}

/// Build the error Node reports for `syscall` on `path` (`CODE: description, syscall 'path'`).
pub fn node_fs_error(err: &io::Error, syscall: &str, path: Option<&str>) -> NodeFsError {
    let (code, desc) = io_error_code(err);
    let message = match path {
        Some(path) => format!("{code}: {desc}, {syscall} '{path}'"),
        None => format!("{code}: {desc}, {syscall}"),
    };
    NodeFsError { code, message }
}

/// `String(error)` / `` `Error code: ${error.code}` `` as the tools format caught errors:
/// errors with a Node `code` give `Error code: CODE`, others `Error: message`.
pub fn describe_error_with_code(err: &anyhow::Error) -> String {
    if let Some(fs_err) = err.downcast_ref::<NodeFsError>() {
        return format!("Error code: {}", fs_err.code);
    }
    format!("Error: {err}")
}

/// Access mode bits for [`access`].
pub const F_OK: i32 = 0;
pub const R_OK: i32 = 4;
pub const W_OK: i32 = 2;

/// `fs.promises.access(path, mode)`.
pub async fn access(path: &str, mode: i32) -> Result<(), NodeFsError> {
    let owned = path.to_string();
    tokio::task::spawn_blocking(move || access_sync(&owned, mode))
        .await
        .unwrap_or_else(|_| {
            Err(NodeFsError {
                code: "EIO".into(),
                message: "EIO: i/o error, access".into(),
            })
        })
}

/// `fs.accessSync(path, mode)`.
pub fn access_sync(path: &str, mode: i32) -> Result<(), NodeFsError> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let c_path = CString::new(path).map_err(|_| NodeFsError {
            code: "ERR_INVALID_ARG_VALUE".into(),
            message: "The argument 'path' must be a string without null bytes".into(),
        })?;
        // SAFETY: c_path is a valid NUL-terminated string.
        let rc = unsafe { libc::access(c_path.as_ptr(), mode) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        Err(node_fs_error(&err, "access", Some(path)))
    }
    #[cfg(not(unix))]
    {
        match std::fs::metadata(path) {
            Ok(meta) => {
                if mode & W_OK != 0 && meta.permissions().readonly() {
                    let err = io::Error::from(io::ErrorKind::PermissionDenied);
                    return Err(node_fs_error(&err, "access", Some(path)));
                }
                Ok(())
            }
            Err(err) => Err(node_fs_error(&err, "access", Some(path))),
        }
    }
}

/// `fs.promises.readFile(path)`.
pub async fn read_file(path: &str) -> Result<Vec<u8>, NodeFsError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(bytes),
        Err(err) => {
            let (code, _) = io_error_code(&err);
            // Opening a directory succeeds on Unix; the read fails without a path.
            if code == "EISDIR" {
                Err(node_fs_error(&err, "read", None))
            } else {
                Err(node_fs_error(&err, "open", Some(path)))
            }
        }
    }
}

/// `fs.promises.writeFile(path, content, "utf-8")`.
pub async fn write_file(path: &str, content: &[u8]) -> Result<(), NodeFsError> {
    tokio::fs::write(path, content)
        .await
        .map_err(|err| node_fs_error(&err, "open", Some(path)))
}

/// `fs.promises.mkdir(path, { recursive: true })`.
pub async fn mkdir_recursive(path: &str) -> Result<(), NodeFsError> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|err| node_fs_error(&err, "mkdir", Some(path)))
}

/// `fs.promises.stat(path)`.
pub async fn stat(path: &str) -> Result<std::fs::Metadata, NodeFsError> {
    tokio::fs::metadata(path)
        .await
        .map_err(|err| node_fs_error(&err, "stat", Some(path)))
}

/// `fs.promises.readdir(path)`: entry names in directory order.
pub async fn readdir(path: &str) -> Result<Vec<String>, NodeFsError> {
    let mut reader = tokio::fs::read_dir(path)
        .await
        .map_err(|err| node_fs_error(&err, "scandir", Some(path)))?;
    let mut names = Vec::new();
    loop {
        match reader.next_entry().await {
            Ok(Some(entry)) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Ok(None) => break,
            Err(err) => return Err(node_fs_error(&err, "scandir", Some(path))),
        }
    }
    Ok(names)
}
