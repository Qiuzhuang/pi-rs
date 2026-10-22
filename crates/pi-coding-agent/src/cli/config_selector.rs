//! Port of `packages/coding-agent/src/cli/config-selector.ts`.
//!
//! TUI config selector for the `pi config` command.

use std::sync::Arc;

use parking_lot::Mutex;
use pi_tui::terminal::ProcessTerminal;
use pi_tui::tui::{ComponentRef, Tui, TuiOptions, TuiStopOptions};
use tokio::sync::oneshot;

use crate::core::settings_manager::SettingsManager;
use crate::core::settings_manager::SettingsScope;
use crate::modes::interactive::components::config_selector::{
    ConfigSelectorComponent, ConfigWriteScope, ScopedResolvedPaths,
};
use crate::modes::interactive::theme::theme::{init_theme, stop_theme_watcher};
use crate::utils::process_exit::exit;

#[derive(Clone)]
pub struct ConfigSelectorOptions {
    pub resolved_paths: ScopedResolvedPaths,
    pub settings_manager: Arc<SettingsManager>,
    pub cwd: String,
    pub agent_dir: String,
    pub write_scope: SettingsScope,
    pub project_mode_available: bool,
}

/// Show TUI config selector and return when closed
pub async fn select_config(options: &ConfigSelectorOptions) -> anyhow::Result<()> {
    // Initialize theme before showing TUI
    init_theme(options.settings_manager.get_theme().as_deref(), Some(true));

    let ui = Tui::main_screen_with(
        Arc::new(ProcessTerminal::new()),
        TuiOptions {
            show_hardware_cursor: Some(options.settings_manager.get_show_hardware_cursor()),
            log_directory: Some(options.agent_dir.clone().into()),
            event_loop: None,
        },
    );
    ui.set_clear_on_shrink(options.settings_manager.get_clear_on_shrink());

    let (tx, rx) = oneshot::channel::<()>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let write_scope = match options.write_scope {
        SettingsScope::Global => ConfigWriteScope::Global,
        SettingsScope::Project => ConfigWriteScope::Project,
    };
    let selector = Arc::new(ConfigSelectorComponent::new(
        &options.resolved_paths,
        options.settings_manager.clone(),
        &options.cwd,
        &options.agent_dir,
        {
            let weak = Arc::downgrade(&ui);
            Arc::new(move || {
                if let Some(tx) = tx.lock().take() {
                    if let Some(ui) = weak.upgrade() {
                        ui.stop(TuiStopOptions::default());
                    }
                    stop_theme_watcher();
                    let _ = tx.send(());
                }
            })
        },
        {
            let weak = Arc::downgrade(&ui);
            Arc::new(move || {
                if let Some(ui) = weak.upgrade() {
                    ui.stop(TuiStopOptions::default());
                }
                stop_theme_watcher();
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
        Some(ui.terminal().rows() as i64),
        Some(&write_scope),
        Some(options.project_mode_available),
    ));

    let component: ComponentRef = selector.clone();
    ui.add_child(component);
    let list: ComponentRef = selector.get_resource_list();
    ui.set_focus(Some(list));
    ui.start();
    let _ = rx.await;
    Ok(())
}
