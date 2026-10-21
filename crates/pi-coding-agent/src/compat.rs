//! Placeholders for types that the dependency crates (`pi-ai`, `pi-tui`, `pi-codemode`) do not
//! export yet.
//!
//! Each item names the TypeScript type it stands for. When the owning crate adds the real type,
//! replace the placeholder here with a `pub use` of it (or switch the users to the real path) and
//! delete the placeholder. Keep this module small: it exists only so the skeleton compiles while
//! the other crates are being ported in parallel.

use std::sync::Arc;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub use pi_tui::tui::ComponentRef;

// ---------------------------------------------------------------------------
// pi-ai
// ---------------------------------------------------------------------------

/// `AuthOperationOptions` (`packages/ai/src/auth/types.ts`): optional cancellation for public
/// auth and credential operations. pi-ai's Rust API takes the signal directly
/// (`Option<&CancellationToken>`); coding-agent APIs keep the TS options shape.
#[derive(Debug, Clone, Default)]
pub struct AuthOperationOptions {
    pub signal: Option<CancellationToken>,
}

/// Extension OAuth compat types (`packages/ai/src/compat/extension-oauth-types.ts`), exported by
/// pi-ai.
pub use pi_ai::auth::extension_oauth_types::{
    OAuthAuthInfo, OAuthDeviceCodeInfo, OAuthLoginCallbacks, OAuthPrompt, OAuthSelectOption,
    OAuthSelectPrompt,
};

// ---------------------------------------------------------------------------
// pi-tui
// ---------------------------------------------------------------------------

/// `MarkdownTheme` (`packages/tui/src/components/markdown.ts`).
pub use pi_tui::components::markdown::MarkdownTheme;

/// `EditorTheme` (`packages/tui/src/components/editor.ts`).
pub type EditorTheme = pi_tui::components::editor::EditorTheme;

/// `SelectListTheme` (`packages/tui/src/components/select-list.ts`).
pub type SelectListTheme = pi_tui::components::select_list::SelectListTheme;

/// `SelectListLayoutOptions` (`packages/tui/src/components/select-list.ts`).
pub type SelectListLayoutOptions = pi_tui::components::select_list::SelectListLayoutOptions;

/// `SettingsListTheme` (`packages/tui/src/components/settings-list.ts`).
#[derive(Clone, Default)]
pub struct SettingsListTheme {
    pub styles: Value,
}

/// `SelectItem` (`packages/tui/src/components/select-list.ts`).
pub type SelectItem = pi_tui::components::select_list::SelectItem;

/// `SelectList` (`packages/tui/src/components/select-list.ts`).
pub type SelectList = pi_tui::components::select_list::SelectList;

/// `SettingsList` (`packages/tui/src/components/settings-list.ts`).
pub struct SettingsList {
    // Placeholder until pi-tui exports the component.
}

/// `Input` (`packages/tui/src/components/input.ts`).
pub struct Input {
    // Placeholder until pi-tui exports the component.
}

/// `Editor` (`packages/tui/src/components/editor.ts`).
pub struct Editor {
    // Placeholder until pi-tui exports the component.
}

/// `Markdown` (`packages/tui/src/components/markdown.ts`).
pub struct Markdown {
    // Placeholder until pi-tui exports the component.
}

/// `Text` (`packages/tui/src/components/text.ts`).
pub struct Text {
    // Placeholder until pi-tui exports the component.
}

/// `Container` (`packages/tui/src/tui.ts`).
pub type Container = pi_tui::tui::Container;

/// `EditorComponent` (`packages/tui/src/editor-component.ts`).
pub use pi_tui::components::editor::EditorComponent;

/// `AutocompleteItem` (`packages/tui/src/autocomplete.ts`).
pub type AutocompleteItem = pi_tui::autocomplete::AutocompleteItem;

/// `AutocompleteProvider` (`packages/tui/src/autocomplete.ts`).
pub use pi_tui::autocomplete::AutocompleteProvider;

/// `WheelScrollLines` (`packages/tui/src/wheel-scroll.ts`): `number | "auto"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WheelScrollLines {
    Lines(u32),
    /// Always `"auto"`.
    Auto(String),
}

/// `RenderCache` (`packages/tui`).
pub struct RenderCache {
    // Placeholder until pi-tui exports it.
}

/// `ImageTranscoder` (`packages/tui/src/terminal-image.ts`).
pub trait ImageTranscoder: Send + Sync {
    fn transcode(
        &self,
        data: &[u8],
        mime_type: &str,
    ) -> BoxFuture<'static, anyhow::Result<Vec<u8>>>;
}

// ---------------------------------------------------------------------------
// pi-codemode
// ---------------------------------------------------------------------------

/// `CodemodeJsonSchema` (`packages/codemode/src/types.ts`): `{ [key: string]: unknown } | boolean`.
pub type CodemodeJsonSchema = Value;

/// Shared closure used by placeholders.
pub type Callback = Arc<dyn Fn() + Send + Sync>;
