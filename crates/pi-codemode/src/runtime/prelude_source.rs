//! JavaScript evaluated inside the VM before the script runs (`prelude.js`).
//!
//! The prelude keeps the host bridge in a closure so the script cannot call it directly, and
//! builds `tools`, `ALL_TOOLS`, the output helpers (`text`, `image`, `exit`, `console`), and
//! globals on top of it. Tool arguments and results cross as JSON strings and are parsed on this
//! side.
//!
//! Output helpers: `text(value)` appends a text item (non-strings are JSON-stringified),
//! `image(urlOrItem)` appends an image from a base64 `data:` URL, an `{ image_url }` object, or an
//! image block `{ type: "image", data, mimeType }`, and `exit()` ends the script successfully.
//! `console.*` appends text items marked as console output.
//!
//! `store(key, value)` and `load(key)` are synchronous: they work on a snapshot of JSON text passed
//! in as `storeJson`, and the keys the script wrote are reported with a successful "done".
//!
//! Evaluates to a function `(bridge, toolsJson, globalsJson, storeJson) => { settle, run, stalled,
//! abandon }`. `settle(id, ok, payload, propertiesJson)` settles call `id`; a rejection's `Error`
//! gets the own properties of the `propertiesJson` object, except `name`, `message`, and `stack`.
//! `toolsJson` lists `{ name, jsName, description }`: `tools[jsName]` and `tools[name]` call the
//! tool, and `ALL_TOOLS` lists `{ name: jsName, description }`. `stalled()` reports a script that
//! has not finished while no host call is pending: with no timers or I/O in the VM, nothing can
//! ever resume it. `globalsJson` lists `{ name, spread }`; `a.b` names are grouped into a frozen
//! `a` object. `bridge(kind, a, b, c)` with kind "call" or "global" (id, name, argsJson), "output"
//! ("text" or "console", text) or ("image", data, mimeType), or "done" (ok,
//! valueJsonOrErrorJson, writesJson).
//!
//! Before anything else the prelude freezes the built-ins and makes the built-in globals
//! read-only. The prelude shares them with the script, so a script that patched one (for example
//! `Array.prototype.toJSON`) could otherwise corrupt what the prelude reports to the host.

use std::sync::LazyLock;

/// Characters of JSON one stored value may have.
pub const MAX_STORE_VALUE_CHARS: usize = 256 * 1024;
/// Characters of keys and JSON all stored values may have together.
pub const MAX_STORE_TOTAL_CHARS: usize = 1024 * 1024;
/// Output one script may produce with `text()`, `image()`, and `console.*`: characters of text and
/// base64 image data. The host keeps all output until the script ends, so without a limit a script
/// that prints in a loop grows the host's memory until it crashes.
pub const MAX_OUTPUT_CHARS: usize = 16 * 1024 * 1024;
/// Output items one script may produce; covers loops that print empty strings.
pub const MAX_OUTPUT_ITEMS: usize = 100_000;

const IMAGE_HELPER_EXPECTS: &str =
    "image expects a non-empty image URL string, an object with image_url, or an image block { type: \"image\", data, mimeType }";

/// The prelude's source text.
pub static PRELUDE_SOURCE: LazyLock<String> = LazyLock::new(|| {
    include_str!("prelude.js")
        .replace("__MAX_STORE_VALUE_CHARS__", &MAX_STORE_VALUE_CHARS.to_string())
        .replace("__MAX_STORE_TOTAL_CHARS__", &MAX_STORE_TOTAL_CHARS.to_string())
        .replace("__MAX_OUTPUT_CHARS__", &MAX_OUTPUT_CHARS.to_string())
        .replace("__MAX_OUTPUT_ITEMS__", &MAX_OUTPUT_ITEMS.to_string())
        .replace("__IMAGE_HELPER_EXPECTS__", &crate::js::quote(IMAGE_HELPER_EXPECTS))
});

/// FNV-1a over UTF-16 code units, as 8 hex digits: identifies the prelude in the fingerprint.
pub(crate) fn fnv1a(text: &str) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for unit in text.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

pub(crate) static PRELUDE_HASH: LazyLock<String> = LazyLock::new(|| fnv1a(&PRELUDE_SOURCE));
