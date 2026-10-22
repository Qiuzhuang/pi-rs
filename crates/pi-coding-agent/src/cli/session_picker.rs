//! Port of `packages/coding-agent/src/cli/session-picker.ts`.
//!
//! TUI session selector for the --resume flag.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_tui::tui::{ComponentRef, TuiStopOptions};
use tokio::sync::oneshot;

use crate::cli::startup_ui::{create_startup_tui, start_startup_tui};
use crate::core::keybindings::KeybindingsManager;
use crate::core::settings_manager::SettingsManager;
use crate::modes::interactive::components::session_selector::{
    SessionSelectorComponent, SessionSelectorComponentOptions,
};
use crate::utils::process_exit::exit;

pub use crate::modes::interactive::components::session_selector::SessionsLoader;

/// Show TUI session selector and return selected session path or null if cancelled
pub async fn select_session(
    current_sessions_loader: &SessionsLoader,
    all_sessions_loader: &SessionsLoader,
    settings_manager: Arc<SettingsManager>,
) -> anyhow::Result<Option<String>> {
    let ui = create_startup_tui(settings_manager.clone()).await?;
    let keybindings = KeybindingsManager::create(None);
    pi_tui::keybindings::set_keybindings(keybindings.tui_manager());

    let (tx, rx) = oneshot::channel::<Option<String>>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let resolve = {
        let tx = tx.clone();
        let weak = Arc::downgrade(&ui);
        Arc::new(move |value: Option<String>| {
            let tx_taken = tx.lock().take();
            if let Some(tx) = tx_taken {
                if let Some(ui) = weak.upgrade() {
                    ui.stop(TuiStopOptions::default());
                }
                let _ = tx.send(value);
            }
        })
    };

    let selector = Arc::new(SessionSelectorComponent::new(
        current_sessions_loader,
        all_sessions_loader,
        {
            let resolve = resolve.clone();
            Arc::new(move |path: String| resolve(Some(path)))
        },
        {
            let resolve = resolve.clone();
            Arc::new(move || resolve(None))
        },
        {
            let weak = Arc::downgrade(&ui);
            Arc::new(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.stop(TuiStopOptions::default());
                }
                exit(0);
            })
        },
        {
            let weak = Arc::downgrade(&ui);
            Arc::new(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.request_render(false);
                }
            })
        },
        Some(&SessionSelectorComponentOptions {
            rename_session: None,
            show_rename_hint: Some(false),
            keybindings: Some(keybindings),
        }),
        None,
    ));

    let component: ComponentRef = selector.clone();
    ui.add_child(component);
    let list: ComponentRef = selector.get_session_list();
    ui.set_focus(Some(list));
    start_startup_tui(ui, settings_manager);
    Ok(rx.await.unwrap_or(None))
}
