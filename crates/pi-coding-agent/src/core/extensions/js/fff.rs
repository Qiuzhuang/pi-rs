//! `@ff-labs/fff-node` host functions: the virtual JS module (`js/fff-node.js`) drives these.
//!
//! `pi-fff` builds its fuzzy file resolution, autocomplete and content grep on the
//! `@ff-labs/fff-node` npm package, which in Node.js loads the native `libfff_c` shared
//! library through the `ffi-rs` N-API addon (a `.node` binary, unsupported by pi-jsrt).
//! Here the addon is replaced by this module: `libfff_c.so`/`.dylib`/`.dll` (the pure engine,
//! shipped in the `@ff-labs/fff-bin-<platform>` npm package next to the extension) is dlopened
//! directly and its C API — `fff-c` v0.9.6's `include/fff.h` — is bound natively.
//!
//! Layout fidelity: the structs mirror `fff-c` 0.9.6's generated header, which matches the
//! installed binary (verified symbol-for-symbol against `@ff-labs/fff-bin-linux-x64-musl`
//! 0.9.6 and its `fff_health_check().version`). The library's version is checked at load and
//! unsupported versions fail with a clear error instead of misreading memory. Note the
//! upstream JS bindings deviate from the header for `FffScore.exact_match` and the
//! `FffGrepMatch` `fuzzy_score`/`is_binary`/`is_definition` tail (they read stale offsets,
//! silently zeroing those fields); these bindings read the header layout, so those fields
//! carry their true values here.
//!
//! Instance handles returned by `fff_create_instance_with` are registered in a process-wide
//! table and exposed to JS as integer ids; [`shutdown_all`] destroys whatever a discarded
//! extension runtime still holds.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use libloading::Library;
use serde_json::{Value, json};

/// `fff-c` series whose struct layouts this module binds (checked against the library's
/// reported `fff_health_check` version before any other call is made).
const BOUND_SERIES: &str = "0.9.";

type VoidPtr = *mut std::ffi::c_void;

// ---------------------------------------------------------------------------
// Raw C structures (`fff-c` 0.9.6, include/fff.h; `#[repr(C)]` reproduces the layout)
// ---------------------------------------------------------------------------

#[repr(C)]
struct FffResultRaw {
    success: bool,
    error: *mut c_char,
    handle: VoidPtr,
    int_value: i64,
}

#[repr(C)]
struct CreateOptions {
    version: u32,
    base_path: *const c_char,
    frecency_db_path: *const c_char,
    history_db_path: *const c_char,
    enable_mmap_cache: bool,
    enable_content_indexing: bool,
    watch: bool,
    ai_mode: bool,
    log_file_path: *const c_char,
    log_level: *const c_char,
    cache_budget_max_files: u64,
    cache_budget_max_bytes: u64,
    cache_budget_max_file_size: u64,
    enable_fs_root_scanning: bool,
    enable_home_dir_scanning: bool,
}

#[repr(C)]
struct FileItemRaw {
    relative_path: *mut c_char,
    file_name: *mut c_char,
    git_status: *mut c_char,
    size: u64,
    modified: u64,
    access_frecency_score: i64,
    modification_frecency_score: i64,
    total_frecency_score: i64,
    is_binary: bool,
}

#[repr(C)]
struct ScoreRaw {
    total: i32,
    base_score: i32,
    filename_bonus: i32,
    special_filename_bonus: i32,
    frecency_boost: i32,
    distance_penalty: i32,
    current_file_penalty: i32,
    combo_match_boost: i32,
    path_alignment_bonus: i32,
    exact_match: bool,
    match_type: *mut c_char,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct LocationRaw {
    tag: u8,
    line: i32,
    col: i32,
    end_line: i32,
    end_col: i32,
}

#[repr(C)]
struct SearchResultRaw {
    items: *const FileItemRaw,
    scores: *const ScoreRaw,
    count: u32,
    total_matched: u32,
    total_files: u32,
    location: LocationRaw,
}

#[repr(C)]
struct MatchRangeRaw {
    start: u32,
    end: u32,
}

#[repr(C)]
struct GrepMatchRaw {
    relative_path: *mut c_char,
    file_name: *mut c_char,
    git_status: *mut c_char,
    line_content: *mut c_char,
    match_ranges: *const MatchRangeRaw,
    context_before: *mut *mut c_char,
    context_after: *mut *mut c_char,
    size: u64,
    modified: u64,
    total_frecency_score: i64,
    access_frecency_score: i64,
    modification_frecency_score: i64,
    line_number: u64,
    byte_offset: u64,
    col: u32,
    match_ranges_count: u32,
    context_before_count: u32,
    context_after_count: u32,
    fuzzy_score: u16,
    has_fuzzy_score: bool,
    is_binary: bool,
    is_definition: bool,
}

#[repr(C)]
struct GrepResultRaw {
    items: *const GrepMatchRaw,
    count: u32,
    total_matched: u32,
    total_files_searched: u32,
    total_files: u32,
    filtered_file_count: u32,
    next_file_offset: u32,
    regex_fallback_error: *mut c_char,
}

#[repr(C)]
struct ScanProgressRaw {
    scanned_files_count: u64,
    is_scanning: bool,
    is_watcher_ready: bool,
    is_warmup_complete: bool,
}

#[repr(C)]
struct DirItemRaw {
    relative_path: *mut c_char,
    dir_name: *mut c_char,
    max_access_frecency: i32,
}

#[repr(C)]
struct DirSearchResultRaw {
    items: *const DirItemRaw,
    scores: *const ScoreRaw,
    count: u32,
    total_matched: u32,
    total_dirs: u32,
}

#[repr(C)]
struct MixedItemRaw {
    item_type: u8,
    relative_path: *mut c_char,
    display_name: *mut c_char,
    git_status: *mut c_char,
    size: u64,
    modified: u64,
    access_frecency_score: i64,
    modification_frecency_score: i64,
    total_frecency_score: i64,
    is_binary: bool,
}

#[repr(C)]
struct MixedSearchResultRaw {
    items: *const MixedItemRaw,
    scores: *const ScoreRaw,
    count: u32,
    total_matched: u32,
    total_files: u32,
    total_dirs: u32,
    location: LocationRaw,
}

// ---------------------------------------------------------------------------
// Loaded library
// ---------------------------------------------------------------------------

struct Native {
    _lib: Library,
    create: extern "C" fn(*const CreateOptions) -> *mut FffResultRaw,
    destroy: extern "C" fn(VoidPtr),
    search: extern "C" fn(VoidPtr, *const c_char, *const c_char, u32, u32, u32, i32, u32) -> *mut FffResultRaw,
    glob: extern "C" fn(VoidPtr, *const c_char, *const c_char, u32, u32, u32) -> *mut FffResultRaw,
    searches_directories: extern "C" fn(VoidPtr, *const c_char, *const c_char, u32, u32, u32) -> *mut FffResultRaw,
    search_mixed: extern "C" fn(VoidPtr, *const c_char, *const c_char, u32, u32, u32, i32, u32) -> *mut FffResultRaw,
    live_grep: extern "C" fn(VoidPtr, *const c_char, u8, u64, u32, bool, u32, u32, u64, u32, u32, bool) -> *mut FffResultRaw,
    multi_grep: extern "C" fn(VoidPtr, *const c_char, *const c_char, u64, u32, bool, u32, u32, u64, u32, u32, bool) -> *mut FffResultRaw,
    scan_files: extern "C" fn(VoidPtr) -> *mut FffResultRaw,
    is_scanning: extern "C" fn(VoidPtr) -> bool,
    get_base_path: extern "C" fn(VoidPtr) -> *mut FffResultRaw,
    get_scan_progress: extern "C" fn(VoidPtr) -> *mut FffResultRaw,
    wait_for_scan: extern "C" fn(VoidPtr, u64) -> *mut FffResultRaw,
    restart_index: extern "C" fn(VoidPtr, *const c_char) -> *mut FffResultRaw,
    refresh_git_status: extern "C" fn(VoidPtr) -> *mut FffResultRaw,
    track_query: extern "C" fn(VoidPtr, *const c_char, *const c_char) -> *mut FffResultRaw,
    get_historical_query: extern "C" fn(VoidPtr, u64) -> *mut FffResultRaw,
    health_check: extern "C" fn(VoidPtr, *const c_char) -> *mut FffResultRaw,
    search_result_get_item: extern "C" fn(*const SearchResultRaw, u32) -> *const FileItemRaw,
    search_result_get_score: extern "C" fn(*const SearchResultRaw, u32) -> *const ScoreRaw,
    dir_search_result_get_item: extern "C" fn(*const DirSearchResultRaw, u32) -> *const DirItemRaw,
    dir_search_result_get_score: extern "C" fn(*const DirSearchResultRaw, u32) -> *const ScoreRaw,
    mixed_search_result_get_item: extern "C" fn(*const MixedSearchResultRaw, u32) -> *const MixedItemRaw,
    mixed_search_result_get_score: extern "C" fn(*const MixedSearchResultRaw, u32) -> *const ScoreRaw,
    grep_result_get_match: extern "C" fn(*const GrepResultRaw, u32) -> *const GrepMatchRaw,
    free_result: extern "C" fn(*mut FffResultRaw),
    free_string: extern "C" fn(*mut c_char),
    free_search_result: extern "C" fn(*mut SearchResultRaw),
    free_grep_result: extern "C" fn(*mut GrepResultRaw),
    free_dir_search_result: extern "C" fn(*mut DirSearchResultRaw),
    free_mixed_search_result: extern "C" fn(*mut MixedSearchResultRaw),
    free_scan_progress: extern "C" fn(*mut ScanProgressRaw),
}

fn sym<T: Copy>(lib: &Library, name: &str) -> Result<T, String> {
    let mut symbol_name = name.as_bytes().to_vec();
    symbol_name.push(0);
    // SAFETY: `T` is an `extern "C" fn` type; the dlsym lookup verifies the symbol exists and
    // the declared signature matches `fff-c`'s generated `fff.h` for the bound version.
    let f: libloading::Symbol<T> =
        unsafe { lib.get(&symbol_name) }.map_err(|e| format!("symbol {name} not found in libfff_c: {e}"))?;
    Ok(*f)
}

/// Platform npm package (`@ff-labs/fff-bin-<name>`) and library filename for this build.
fn platform_binary() -> Option<(&'static str, &'static str)> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "musl"))]
    return Some(("linux-x64-musl", "libfff_c.so"));
    #[cfg(all(target_os = "linux", target_arch = "x86_64", not(target_env = "musl")))]
    return Some(("linux-x64-gnu", "libfff_c.so"));
    #[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "musl"))]
    return Some(("linux-arm64-musl", "libfff_c.so"));
    #[cfg(all(target_os = "linux", target_arch = "aarch64", not(target_env = "musl")))]
    return Some(("linux-arm64-gnu", "libfff_c.so"));
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return Some(("darwin-x64", "libfff_c.dylib"));
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return Some(("darwin-arm64", "libfff_c.dylib"));
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Some(("win32-x64", "fff_c.dll"));
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    return Some(("win32-arm64", "fff_c.dll"));
    #[cfg(not(any(
        all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")),
        all(target_os = "macos", any(target_arch = "x86_64", target_arch = "aarch64")),
        all(target_os = "windows", any(target_arch = "x86_64", target_arch = "aarch64")),
    )))]
    return None;
}

/// Locate `libfff_c` like the npm packages do: `PI_FFF_C_LIB` override, then the platform
/// `@ff-labs/fff-bin-*` package inside the extension npm root and any `node_modules` above cwd.
fn discover_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PI_FFF_C_LIB")
        && !p.is_empty()
        && Path::new(&p).is_file()
    {
        return Some(PathBuf::from(p));
    }
    let (platform, filename) = platform_binary()?;
    let pkg = format!("@ff-labs/fff-bin-{platform}");
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from(crate::config::get_agent_dir()).join("npm/node_modules"),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        for dir in cwd.ancestors() {
            roots.push(dir.join("node_modules"));
        }
    }
    roots.into_iter().map(|r| r.join(&pkg).join(filename)).find(|p| p.is_file())
}

/// Read the `version` field of the static health-check document.
fn library_version(n: &Native) -> String {
    let empty = CString::new("").unwrap_or_default();
    let raw = (n.health_check)(std::ptr::null_mut(), empty.as_ptr());
    if raw.is_null() {
        return String::new();
    }
    // SAFETY: `raw` is a valid `FffResult` returned by the library; every borrowed field is
    // read before the matching free call.
    unsafe {
        if !(*raw).success || (*raw).handle.is_null() {
            String::new()
        } else {
            let doc = CStr::from_ptr((*raw).handle as *const c_char).to_string_lossy().into_owned();
            (n.free_result)(raw);
            serde_json::from_str::<Value>(&doc)
                .ok()
                .and_then(|v| v.get("version").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default()
        }
    }
}

fn bind(lib: Library) -> Result<Native, String> {
    let n = Native {
        create: sym(&lib, "fff_create_instance_with")?,
        destroy: sym(&lib, "fff_destroy")?,
        search: sym(&lib, "fff_search")?,
        glob: sym(&lib, "fff_glob")?,
        searches_directories: sym(&lib, "fff_search_directories")?,
        search_mixed: sym(&lib, "fff_search_mixed")?,
        live_grep: sym(&lib, "fff_live_grep")?,
        multi_grep: sym(&lib, "fff_multi_grep")?,
        scan_files: sym(&lib, "fff_scan_files")?,
        is_scanning: sym(&lib, "fff_is_scanning")?,
        get_base_path: sym(&lib, "fff_get_base_path")?,
        get_scan_progress: sym(&lib, "fff_get_scan_progress")?,
        wait_for_scan: sym(&lib, "fff_wait_for_scan")?,
        restart_index: sym(&lib, "fff_restart_index")?,
        refresh_git_status: sym(&lib, "fff_refresh_git_status")?,
        track_query: sym(&lib, "fff_track_query")?,
        get_historical_query: sym(&lib, "fff_get_historical_query")?,
        health_check: sym(&lib, "fff_health_check")?,
        search_result_get_item: sym(&lib, "fff_search_result_get_item")?,
        search_result_get_score: sym(&lib, "fff_search_result_get_score")?,
        dir_search_result_get_item: sym(&lib, "fff_dir_search_result_get_item")?,
        dir_search_result_get_score: sym(&lib, "fff_dir_search_result_get_score")?,
        mixed_search_result_get_item: sym(&lib, "fff_mixed_search_result_get_item")?,
        mixed_search_result_get_score: sym(&lib, "fff_mixed_search_result_get_score")?,
        grep_result_get_match: sym(&lib, "fff_grep_result_get_match")?,
        free_result: sym(&lib, "fff_free_result")?,
        free_string: sym(&lib, "fff_free_string")?,
        free_search_result: sym(&lib, "fff_free_search_result")?,
        free_grep_result: sym(&lib, "fff_free_grep_result")?,
        free_dir_search_result: sym(&lib, "fff_free_dir_search_result")?,
        free_mixed_search_result: sym(&lib, "fff_free_mixed_search_result")?,
        free_scan_progress: sym(&lib, "fff_free_scan_progress")?,
        _lib: lib,
    };
    // Layout gate: the bindings mirror `fff-c` 0.9.6's generated header. A library reporting a
    // different series may have moved fields, which would silently corrupt results, so refuse.
    let version = library_version(&n);
    if !version.starts_with(BOUND_SERIES) {
        return Err(format!(
            "libfff_c version {version:?} does not match the {BOUND_SERIES}x layout this pi build binds \
             (fff-c 0.9.6): refusing to call a native library with unknown struct layouts. \
             Pin @ff-labs/fff-node to 0.9.x or point PI_FFF_C_LIB at a matching libfff_c.",
        ));
    }
    Ok(n)
}

static NATIVE: OnceLock<Result<Native, String>> = OnceLock::new();

fn native() -> Result<&'static Native, String> {
    NATIVE
        .get_or_init(|| {
            let path = discover_binary().ok_or_else(|| {
                "libfff_c native library not found: install @ff-labs/fff-node's platform binary (npm) or set PI_FFF_C_LIB"
                    .to_string()
            })?;
            // SAFETY: loading the cdylib produced by the fff project; `bind` only uses symbols
            // after they resolved and the version gate passes before any other call.
            let lib = unsafe { Library::new(&path) }.map_err(|e| format!("cannot load {}: {e}", path.display()))?;
            bind(lib)
        })
        .as_ref()
        .map_err(Clone::clone)
}

// ---------------------------------------------------------------------------
// Instance table (JS sees integer ids; the raw pointers never cross into JS)
// ---------------------------------------------------------------------------

// A raw native pointer wrapped so it can live in the process-wide instance table.
// SAFETY: the table is the single owner of each handle; only `dispatch` (single JS thread)
// and `shutdown_all` ever read or release them, never concurrently on different threads.
struct RawPtr(VoidPtr);

unsafe impl Send for RawPtr {}

struct Entry {
    ptr: RawPtr,
    /// `(base_path, frecency_db_path, history_db_path)` for DB-backed finders, `None` for
    /// DB-less ones. The DB environments can only be opened once per process, so requests
    /// for the same triple share one native instance with a reference count: pi-web hosts
    /// many pi sessions in one process, and pi-fff's own shared-finder registry is scoped
    /// to a single JS runtime while this port can host several (dmtrKovalenko/fff#760).
    key: Option<(String, String, String)>,
    refs: usize,
}

static INSTANCES: Mutex<Vec<Option<Entry>>> = Mutex::new(Vec::new());

fn instance(id: f64) -> anyhow::Result<VoidPtr> {
    let idx = id as usize;
    INSTANCES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(idx)
        .and_then(|slot| slot.as_ref().map(|e| e.ptr.0))
        .ok_or_else(|| anyhow::anyhow!("FileFinder instance has been destroyed."))
}

/// Destroy every native finder still registered (an extension runtime is being dropped).
pub(crate) fn shutdown_all() {
    let drained: Vec<VoidPtr> = {
        let mut table = INSTANCES.lock().unwrap_or_else(|e| e.into_inner());
        table.drain(..).flatten().map(|e| e.ptr.0).collect()
    };
    if let Ok(n) = native() {
        for ptr in drained {
            (n.destroy)(ptr);
        }
    }
}

// ---------------------------------------------------------------------------
// JSON helpers (output shapes mirror the `@ff-labs/fff-node` JS layer)
// ---------------------------------------------------------------------------

fn err_json(message: impl Into<String>) -> Value {
    json!({ "ok": false, "error": message.into() })
}

fn ok_value(value: Value) -> Value {
    json!({ "ok": true, "value": value })
}

fn ok_json() -> Value {
    json!({ "ok": true })
}

unsafe fn cstr(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
    }
}

unsafe fn cstr_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() { None } else { Some(unsafe { cstr(ptr) }) }
}

fn u(a: &Value) -> u64 {
    a.as_u64().unwrap_or(0)
}

fn i(a: &Value) -> i64 {
    a.as_i64().unwrap_or(0)
}

fn b(a: &Value) -> bool {
    a.as_bool().unwrap_or(false)
}

fn s(a: &Value) -> String {
    a.as_str().unwrap_or_default().to_string()
}

fn grep_mode(a: &Value) -> u8 {
    match a.as_str() {
        Some("regex") => 1,
        Some("fuzzy") => 2,
        _ => 0,
    }
}

/// Recursively rewrite snake_case object keys to camelCase (the JS API surface is camelCase;
/// `fff_health_check` hands back a snake_case JSON document).
fn snake_to_camel_json(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(snake_to_camel_json).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let mut camel = String::with_capacity(k.len());
                    let mut up = false;
                    for c in k.chars() {
                        if c == '_' {
                            up = true;
                        } else if up {
                            camel.extend(c.to_uppercase());
                            up = false;
                        } else {
                            camel.push(c);
                        }
                    }
                    (camel, snake_to_camel_json(v))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Envelope: on failure returns an error Result, on success yields the payload field and
/// releases the envelope (never the payload itself).
enum Envelope {
    Err(String),
    Handle(VoidPtr),
    Int(i64),
}

/// Read an `FffResult` returned by a raw call, keeping the payload for the caller.
fn take_envelope(n: &Native, raw: *mut FffResultRaw) -> Envelope {
    if raw.is_null() {
        return Envelope::Err("native call returned a null result".to_string());
    }
    // SAFETY: `raw` points at a library-allocated `FffResult`; fields are read before
    // `fff_free_result` releases the envelope (not the payload).
    let (success, error, handle, int_value) = unsafe { ((*raw).success, (*raw).error, (*raw).handle, (*raw).int_value) };
    if !success {
        let message = unsafe { cstr_opt(error) }.unwrap_or_else(|| "Unknown error".to_string());
        (n.free_result)(raw);
        return Envelope::Err(message);
    }
    (n.free_result)(raw);
    // `bool`/`int` return values ride in `int_value`; pointer payloads in `handle`.
    if handle.is_null() {
        Envelope::Int(int_value)
    } else {
        Envelope::Handle(handle)
    }
}

/// `FffResult` whose payload is a C string (base path, historical query, health JSON).
fn string_result(n: &Native, raw: *mut FffResultRaw) -> Value {
    match take_envelope(n, raw) {
        Envelope::Err(message) => err_json(message),
        Envelope::Handle(ptr) => {
            // SAFETY: the string is heap-allocated by the library and released below.
            let text = unsafe { cstr_opt(ptr as *const c_char) };
            (n.free_string)(ptr as *mut c_char);
            match text {
                None => ok_json(),
                Some(text) => ok_value(Value::String(text)),
            }
        }
        _ => ok_json(),
    }
}

fn int_result(n: &Native, raw: *mut FffResultRaw) -> Value {
    match take_envelope(n, raw) {
        Envelope::Err(message) => err_json(message),
        Envelope::Int(v) => ok_value(json!(v)),
        Envelope::Handle(_) => ok_value(json!(0)),
    }
}

fn bool_result(n: &Native, raw: *mut FffResultRaw) -> Value {
    match take_envelope(n, raw) {
        Envelope::Err(message) => err_json(message),
        Envelope::Int(v) => ok_value(json!(v != 0)),
        _ => ok_value(json!(false)),
    }
}

fn void_result(n: &Native, raw: *mut FffResultRaw) -> Value {
    match take_envelope(n, raw) {
        Envelope::Err(message) => err_json(message),
        _ => ok_json(),
    }
}

fn location_json(location: &LocationRaw) -> Option<Value> {
    match location.tag {
        1 => Some(json!({ "type": "line", "line": location.line })),
        2 => Some(json!({ "type": "position", "line": location.line, "col": location.col })),
        3 => Some(json!({
            "type": "range",
            "start": { "line": location.line, "col": location.col },
            "end": { "line": location.end_line, "col": location.end_col },
        })),
        _ => None,
    }
}

fn score_json(raw: &ScoreRaw) -> Value {
    // SAFETY: `raw` borrows a struct owned by a live native result; `cstr` reads
    // NUL-terminated C strings that outlive the result's free call.
    unsafe {
        json!({
            "total": raw.total,
            "baseScore": raw.base_score,
            "filenameBonus": raw.filename_bonus,
            "specialFilenameBonus": raw.special_filename_bonus,
            "frecencyBoost": raw.frecency_boost,
            "distancePenalty": raw.distance_penalty,
            "currentFilePenalty": raw.current_file_penalty,
            "comboMatchBoost": raw.combo_match_boost,
            "exactMatch": raw.exact_match,
            "matchType": cstr(raw.match_type),
        })
    }
}

fn file_item_json(raw: &FileItemRaw) -> Value {
    // SAFETY: as in `score_json` — all strings are owned by the live native result.
    unsafe {
        json!({
            "relativePath": cstr(raw.relative_path),
            "fileName": cstr(raw.file_name),
            "gitStatus": cstr(raw.git_status),
            "size": raw.size,
            "modified": raw.modified,
            "accessFrecencyScore": raw.access_frecency_score,
            "modificationFrecencyScore": raw.modification_frecency_score,
            "totalFrecencyScore": raw.total_frecency_score,
        })
    }
}

fn dir_item_json(raw: &DirItemRaw) -> Value {
    // SAFETY: as in `score_json`.
    unsafe {
        json!({
            "relativePath": cstr(raw.relative_path),
            "dirName": cstr(raw.dir_name),
            "maxAccessFrecency": raw.max_access_frecency,
        })
    }
}

fn mixed_item_json(raw: &MixedItemRaw) -> Value {
    // SAFETY: as in `score_json`.
    unsafe {
        if raw.item_type == 1 {
            json!({
                "type": "directory",
                "item": {
                    "relativePath": cstr(raw.relative_path),
                    "dirName": cstr(raw.display_name),
                    "maxAccessFrecency": raw.access_frecency_score,
                },
            })
        } else {
            json!({
                "type": "file",
                "item": {
                    "relativePath": cstr(raw.relative_path),
                    "fileName": cstr(raw.display_name),
                    "gitStatus": cstr(raw.git_status),
                    "size": raw.size,
                    "modified": raw.modified,
                    "accessFrecencyScore": raw.access_frecency_score,
                    "modificationFrecencyScore": raw.modification_frecency_score,
                    "totalFrecencyScore": raw.total_frecency_score,
                },
            })
        }
    }
}

fn grep_match_json(raw: &GrepMatchRaw) -> Value {
    // SAFETY: as in `score_json`; the range/context arrays have `*_count` elements
    // owned by the live grep result.
    let mut ranges: Vec<Value> = Vec::with_capacity(raw.match_ranges_count as usize);
    for k in 0..raw.match_ranges_count {
        let range = unsafe { &*raw.match_ranges.add(k as usize) };
        ranges.push(json!([range.start, range.end]));
    }
    let mut out = unsafe {
        json!({
            "relativePath": cstr(raw.relative_path),
            "fileName": cstr(raw.file_name),
            "gitStatus": cstr(raw.git_status),
            "lineContent": cstr(raw.line_content),
            "size": raw.size,
            "modified": raw.modified,
            "totalFrecencyScore": raw.total_frecency_score,
            "accessFrecencyScore": raw.access_frecency_score,
            "modificationFrecencyScore": raw.modification_frecency_score,
            "isBinary": raw.is_binary,
            "lineNumber": raw.line_number,
            "col": raw.col,
            "byteOffset": raw.byte_offset,
            "matchRanges": ranges,
        })
    };
    if raw.has_fuzzy_score {
        out["fuzzyScore"] = json!(raw.fuzzy_score);
    }
    if raw.context_before_count > 0 {
        let mut before: Vec<Value> = Vec::with_capacity(raw.context_before_count as usize);
        for k in 0..raw.context_before_count {
            before.push(Value::String(unsafe { cstr((*raw.context_before).add(k as usize)) }));
        }
        out["contextBefore"] = Value::Array(before);
    }
    if raw.context_after_count > 0 {
        let mut after: Vec<Value> = Vec::with_capacity(raw.context_after_count as usize);
        for k in 0..raw.context_after_count {
            after.push(Value::String(unsafe { cstr((*raw.context_after).add(k as usize)) }));
        }
        out["contextAfter"] = Value::Array(after);
    }
    if raw.is_definition {
        out["isDefinition"] = json!(true);
    }
    out
}

/// Parse `FffSearchResult` (payload of `fff_search`/`fff_glob`) and release it.
fn parse_search(n: &Native, result: VoidPtr) -> Value {
    let (count, total_matched, total_files, location) = unsafe {
        let sr = &*(result as *const SearchResultRaw);
        (sr.count, sr.total_matched, sr.total_files, sr.location)
    };
    let mut item_values = Vec::with_capacity(count as usize);
    let mut score_values = Vec::with_capacity(count as usize);
    for k in 0..count {
        // SAFETY: the accessors hand out pointers into the result, alive until the free call.
        let item = (n.search_result_get_item)(result as *const SearchResultRaw, k);
        let score = (n.search_result_get_score)(result as *const SearchResultRaw, k);
        item_values.push(file_item_json(unsafe { &*item }));
        score_values.push(score_json(unsafe { &*score }));
    }
    let mut out = json!({
        "items": item_values,
        "scores": score_values,
        "totalMatched": total_matched,
        "totalFiles": total_files,
    });
    if let Some(location) = location_json(&location) {
        out["location"] = location;
    }
    (n.free_search_result)(result as *mut SearchResultRaw);
    ok_value(out)
}

/// Parse `FffDirSearchResult` (payload of `fff_search_directories`) and release it.
fn parse_dir_search(n: &Native, result: VoidPtr) -> Value {
    let (count, total_matched, total_dirs) = unsafe {
        let sr = &*(result as *const DirSearchResultRaw);
        (sr.count, sr.total_matched, sr.total_dirs)
    };
    let mut item_values = Vec::with_capacity(count as usize);
    let mut score_values = Vec::with_capacity(count as usize);
    for k in 0..count {
        // SAFETY: as in `parse_search`.
        let item = (n.dir_search_result_get_item)(result as *const DirSearchResultRaw, k);
        let score = (n.dir_search_result_get_score)(result as *const DirSearchResultRaw, k);
        item_values.push(dir_item_json(unsafe { &*item }));
        score_values.push(score_json(unsafe { &*score }));
    }
    (n.free_dir_search_result)(result as *mut DirSearchResultRaw);
    ok_value(json!({
        "items": item_values,
        "scores": score_values,
        "totalMatched": total_matched,
        "totalDirs": total_dirs,
    }))
}

/// Parse `FffMixedSearchResult` (payload of `fff_search_mixed`) and release it.
fn parse_mixed_search(n: &Native, result: VoidPtr) -> Value {
    let (count, total_matched, total_files, total_dirs, location) = unsafe {
        let sr = &*(result as *const MixedSearchResultRaw);
        (sr.count, sr.total_matched, sr.total_files, sr.total_dirs, sr.location)
    };
    let mut item_values = Vec::with_capacity(count as usize);
    let mut score_values = Vec::with_capacity(count as usize);
    for k in 0..count {
        // SAFETY: as in `parse_search`.
        let item = (n.mixed_search_result_get_item)(result as *const MixedSearchResultRaw, k);
        let score = (n.mixed_search_result_get_score)(result as *const MixedSearchResultRaw, k);
        item_values.push(mixed_item_json(unsafe { &*item }));
        score_values.push(score_json(unsafe { &*score }));
    }
    let mut out = json!({
        "items": item_values,
        "scores": score_values,
        "totalMatched": total_matched,
        "totalFiles": total_files,
        "totalDirs": total_dirs,
    });
    if let Some(location) = location_json(&location) {
        out["location"] = location;
    }
    (n.free_mixed_search_result)(result as *mut MixedSearchResultRaw);
    ok_value(out)
}

/// Parse `FffGrepResult` (payload of `fff_live_grep`/`fff_multi_grep`) and release it.
fn parse_grep(n: &Native, result: VoidPtr) -> Value {
    let (count, total_matched, total_files_searched, total_files, filtered_file_count, next_file_offset, fallback) =
        unsafe {
            let gr = &*(result as *const GrepResultRaw);
            (
                gr.count,
                gr.total_matched,
                gr.total_files_searched,
                gr.total_files,
                gr.filtered_file_count,
                gr.next_file_offset,
                cstr_opt(gr.regex_fallback_error),
            )
        };
    let mut items = Vec::with_capacity(count as usize);
    for k in 0..count {
        // SAFETY: `fff_grep_result_get_match` hands out pointers into the result, which is
        // kept alive until the free call below.
        let match_ptr = (n.grep_result_get_match)(result as *const GrepResultRaw, k);
        items.push(grep_match_json(unsafe { &*match_ptr }));
    }
    (n.free_grep_result)(result as *mut GrepResultRaw);
    let mut out = json!({
        "items": items,
        "totalMatched": total_matched,
        "totalFilesSearched": total_files_searched,
        "totalFiles": total_files,
        "filteredFileCount": filtered_file_count,
        "nextCursor": if next_file_offset > 0 {
            json!({ "__brand": "GrepCursor", "_offset": next_file_offset })
        } else {
            Value::Null
        },
    });
    if let Some(fallback) = fallback {
        out["regexFallbackError"] = Value::String(fallback);
    }
    ok_value(out)
}

// ---------------------------------------------------------------------------
// Dispatch: `call("fff", { fn, args })` from the `@ff-labs/fff-node` virtual module
// ---------------------------------------------------------------------------

fn args_of(payload: &Value) -> Vec<Value> {
    payload
        .get("args")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn dispatch(payload: &Value) -> anyhow::Result<Value> {
    let f = payload
        .get("fn")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = args_of(payload);
    let a = |idx: usize| args.get(idx).cloned().unwrap_or(Value::Null);
    let n = native().map_err(anyhow::Error::msg)?;

    Ok(match f {
        "libPath" => ok_value(json!(discover_binary().map(|p| p.display().to_string()))),
        "available" => ok_value(json!(native().is_ok())),
        "create" => {
            let opts = a(0);
            let base_s = s(&opts["basePath"]);
            let frecency_s = s(&opts["frecencyDbPath"]);
            let history_s = s(&opts["historyDbPath"]);
            // `CString`s must outlive the call; keep them alive in a Vec for the call's duration.
            let base = CString::new(base_s.clone()).unwrap_or_default();
            let frecency = CString::new(frecency_s.clone()).unwrap_or_default();
            let history = CString::new(history_s.clone()).unwrap_or_default();
            let log_file = CString::new(s(&opts["logFilePath"])).unwrap_or_default();
            let log_level = CString::new(s(&opts["logLevel"])).unwrap_or_default();
            // SAFETY: `FffCreateOptions` is a `#[repr(C)]` mirror of the header's struct; all
            // strings are valid NUL-terminated UTF-8 for the duration of the call.
            let options = CreateOptions {
                version: 1,
                base_path: base.as_ptr(),
                frecency_db_path: frecency.as_ptr(),
                history_db_path: history.as_ptr(),
                enable_mmap_cache: b(&opts["enableMmapCache"]),
                enable_content_indexing: b(&opts["enableContentIndexing"]),
                watch: b(&opts["watch"]),
                ai_mode: b(&opts["aiMode"]),
                log_file_path: log_file.as_ptr(),
                log_level: log_level.as_ptr(),
                cache_budget_max_files: u(&opts["cacheBudgetMaxFiles"]),
                cache_budget_max_bytes: u(&opts["cacheBudgetMaxBytes"]),
                cache_budget_max_file_size: u(&opts["cacheBudgetMaxFileSize"]),
                enable_fs_root_scanning: b(&opts["enableFsRootScanning"]),
                enable_home_dir_scanning: b(&opts["enableHomeDirScanning"]),
            };
            // DB environments can only be opened once per process: share one native finder
            // (refcounted) among all requests for the same (base, frecency, history) triple.
            let key = if frecency_s.is_empty() {
                None
            } else {
                Some((base_s, frecency_s, history_s))
            };
            let mut table = INSTANCES.lock().unwrap_or_else(|e| e.into_inner());
            let shared_idx = key.as_ref().and_then(|key| {
                table
                    .iter()
                    .position(|slot| slot.as_ref().is_some_and(|e| e.key.as_ref() == Some(key)))
            });
            let reused = shared_idx.and_then(|idx| {
                table[idx].as_mut().map(|entry| {
                    entry.refs += 1;
                    idx as u64
                })
            });
            if let Some(id) = reused {
                return Ok(ok_value(json!(id)));
            }
            let raw = (n.create)(&options);
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(ptr) => {
                    table.push(Some(Entry {
                        ptr: RawPtr(ptr),
                        key,
                        refs: 1,
                    }));
                    ok_value(json!((table.len() - 1) as u64))
                }
                _ => err_json("fff_create_instance_with returned a null handle"),
            }
        }
        "destroy" => {
            let id = i(&a(0)) as usize;
            let victim = {
                let mut table = INSTANCES.lock().unwrap_or_else(|e| e.into_inner());
                match table.get_mut(id).and_then(Option::as_mut) {
                    Some(entry) => {
                        entry.refs = entry.refs.saturating_sub(1);
                        if entry.refs == 0 {
                            table[id].take().map(|e| e.ptr.0)
                        } else {
                            None
                        }
                    }
                    None => None,
                }
            };
            if let Some(ptr) = victim {
                (n.destroy)(ptr);
            }
            ok_json()
        }
        "search" => {
            let query = CString::new(s(&a(1))).unwrap_or_default();
            let current = CString::new(s(&a(2))).unwrap_or_default();
            let raw = (n.search)(
                instance(i(&a(0)) as f64)?,
                query.as_ptr(),
                current.as_ptr(),
                u(&a(3)) as u32,
                u(&a(4)) as u32,
                u(&a(5)) as u32,
                i(&a(6)) as i32,
                u(&a(7)) as u32,
            );
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(result) => parse_search(n, result),
                _ => err_json("fff_search returned null search result"),
            }
        }
        "glob" => {
            let pattern = CString::new(s(&a(1))).unwrap_or_default();
            let current = CString::new(s(&a(2))).unwrap_or_default();
            let raw = (n.glob)(
                instance(i(&a(0)) as f64)?,
                pattern.as_ptr(),
                current.as_ptr(),
                u(&a(3)) as u32,
                u(&a(4)) as u32,
                u(&a(5)) as u32,
            );
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(result) => parse_search(n, result),
                _ => err_json("fff_glob returned null search result"),
            }
        }
        "directorySearch" => {
            let query = CString::new(s(&a(1))).unwrap_or_default();
            let current = CString::new(s(&a(2))).unwrap_or_default();
            let raw = (n.searches_directories)(
                instance(i(&a(0)) as f64)?,
                query.as_ptr(),
                current.as_ptr(),
                u(&a(3)) as u32,
                u(&a(4)) as u32,
                u(&a(5)) as u32,
            );
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(result) => parse_dir_search(n, result),
                _ => err_json("fff_search_directories returned null search result"),
            }
        }
        "mixedSearch" => {
            let query = CString::new(s(&a(1))).unwrap_or_default();
            let current = CString::new(s(&a(2))).unwrap_or_default();
            let raw = (n.search_mixed)(
                instance(i(&a(0)) as f64)?,
                query.as_ptr(),
                current.as_ptr(),
                u(&a(3)) as u32,
                u(&a(4)) as u32,
                u(&a(5)) as u32,
                i(&a(6)) as i32,
                u(&a(7)) as u32,
            );
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(result) => parse_mixed_search(n, result),
                _ => err_json("fff_search_mixed returned null search result"),
            }
        }
        "grep" => {
            let query = CString::new(s(&a(1))).unwrap_or_default();
            let raw = (n.live_grep)(
                instance(i(&a(0)) as f64)?,
                query.as_ptr(),
                grep_mode(&a(2)),
                u(&a(3)),
                u(&a(4)) as u32,
                b(&a(5)),
                u(&a(6)) as u32,
                u(&a(7)) as u32,
                u(&a(8)),
                u(&a(9)) as u32,
                u(&a(10)) as u32,
                b(&a(11)),
            );
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(result) => parse_grep(n, result),
                _ => err_json("grep returned null result"),
            }
        }
        "multiGrep" => {
            let patterns = CString::new(s(&a(1))).unwrap_or_default();
            let constraints = CString::new(s(&a(2))).unwrap_or_default();
            let raw = (n.multi_grep)(
                instance(i(&a(0)) as f64)?,
                patterns.as_ptr(),
                constraints.as_ptr(),
                u(&a(3)),
                u(&a(4)) as u32,
                b(&a(5)),
                u(&a(6)) as u32,
                u(&a(7)) as u32,
                u(&a(8)),
                u(&a(9)) as u32,
                u(&a(10)) as u32,
                b(&a(11)),
            );
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(result) => parse_grep(n, result),
                _ => err_json("multi grep returned null result"),
            }
        }
        "scanFiles" => {
            let raw = (n.scan_files)(instance(i(&a(0)) as f64)?);
            void_result(n, raw)
        }
        "isScanning" => ok_value(json!((n.is_scanning)(instance(i(&a(0)) as f64)?))),
        "getBasePath" => {
            let raw = (n.get_base_path)(instance(i(&a(0)) as f64)?);
            string_result(n, raw)
        }
        "getScanProgress" => {
            let raw = (n.get_scan_progress)(instance(i(&a(0)) as f64)?);
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(result) => {
                    // SAFETY: the progress struct is library-allocated and freed below.
                    let p = unsafe { &*(result as *const ScanProgressRaw) };
                    let value = json!({
                        "scannedFilesCount": p.scanned_files_count,
                        "isScanning": p.is_scanning,
                        "isWatcherReady": p.is_watcher_ready,
                        "isWarmupComplete": p.is_warmup_complete,
                    });
                    (n.free_scan_progress)(result as *mut ScanProgressRaw);
                    ok_value(value)
                }
                _ => err_json("scan progress returned null"),
            }
        }
        "waitForScan" => {
            let raw = (n.wait_for_scan)(instance(i(&a(0)) as f64)?, u(&a(1)));
            bool_result(n, raw)
        }
        "restartIndex" => {
            let path = CString::new(s(&a(1))).unwrap_or_default();
            let raw = (n.restart_index)(instance(i(&a(0)) as f64)?, path.as_ptr());
            void_result(n, raw)
        }
        "refreshGitStatus" => {
            let raw = (n.refresh_git_status)(instance(i(&a(0)) as f64)?);
            int_result(n, raw)
        }
        "trackQuery" => {
            let query = CString::new(s(&a(1))).unwrap_or_default();
            let path = CString::new(s(&a(2))).unwrap_or_default();
            let raw = (n.track_query)(instance(i(&a(0)) as f64)?, query.as_ptr(), path.as_ptr());
            bool_result(n, raw)
        }
        "historicalQuery" => {
            let raw = (n.get_historical_query)(instance(i(&a(0)) as f64)?, u(&a(1)));
            string_result(n, raw)
        }
        "healthCheck" => {
            // `null` handle = the limited static check (version + git only), like `healthCheckStatic`.
            let handle: VoidPtr = match a(0).as_u64() {
                Some(id) if id != 0 => instance(id as f64).unwrap_or(std::ptr::null_mut()),
                _ => std::ptr::null_mut(),
            };
            let test_path = CString::new(s(&a(1))).unwrap_or_default();
            let raw = (n.health_check)(handle, test_path.as_ptr());
            match take_envelope(n, raw) {
                Envelope::Err(message) => err_json(message),
                Envelope::Handle(result) => {
                    // SAFETY: the JSON document is a heap-allocated C string, released below.
                    let doc = unsafe { cstr_opt(result as *const c_char) };
                    (n.free_string)(result as *mut c_char);
                    match doc {
                        None => ok_json(),
                        Some(doc) => {
                            match serde_json::from_str::<Value>(&doc) {
                                Ok(json) => ok_value(snake_to_camel_json(&json)),
                                Err(_) => ok_value(Value::String(doc)),
                            }
                        }
                    }
                }
                _ => ok_json(),
            }
        }
        _ => anyhow::bail!("unknown fff host call {f}"),
    })
}
