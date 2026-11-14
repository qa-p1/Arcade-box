//! Saved pipelines: list, save, delete and run, with the one-time effects confirmation.

use crate::*;

// Called only on pipeline workers. GTK's callback keeps the main loop free;
// the portal dialog backend would require an unrelated Zenity executable.
// The dialog layout follows the kit's confirm dialog: the question, what the
// pipeline can do as a list, then Cancel and the primary "Run pipeline".
pub(crate) fn confirm_pipeline_effects(
    app: &tauri::AppHandle,
    question: String,
    detail: String,
) -> bool {
    #[cfg(target_os = "linux")]
    {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        if app
            .run_on_main_thread(move || {
                use gtk::prelude::*;
                let dialog = gtk::MessageDialog::new(
                    None::<&gtk::Window>,
                    gtk::DialogFlags::MODAL,
                    gtk::MessageType::Question,
                    gtk::ButtonsType::None,
                    &question,
                );
                dialog.set_title("Run pipeline — Arcade Box");
                dialog.set_secondary_text(Some(&detail));
                dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                if let Some(run) = dialog
                    .add_button("Run pipeline", gtk::ResponseType::Ok)
                    .downcast_ref::<gtk::Button>()
                {
                    run.style_context().add_class("suggested-action");
                }
                dialog.set_default_response(gtk::ResponseType::Ok);
                dialog.connect_response(move |dialog, response| {
                    let _ = send.send(response == gtk::ResponseType::Ok);
                    dialog.close();
                });
                dialog.show_all();
                dialog.present();
            })
            .is_err()
        {
            return false;
        }
        receive.recv().unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_dialog::MessageDialogButtons;
        app.dialog()
            .message(format!("{question}\n\n{detail}"))
            .title("Run pipeline — Arcade Box")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Run pipeline".into(),
                "Cancel".into(),
            ))
            .blocking_show()
    }
}

/// What a pipeline's effects mean, one per line, for the confirm dialog.
pub(crate) fn effects_detail(effects: &[String]) -> String {
    let lines: Vec<String> = effects
        .iter()
        .map(|effect| {
            let text = match effect.as_str() {
                "sends-to-device" => "Send content to your devices",
                "network" => "Use the network",
                "executes-commands" => "Run commands",
                "writes-files" => "Create files",
                "opens-ui" => "Open app windows",
                "clipboard" => "Change the clipboard",
                "uploads-content" => "Upload content",
                "persists" => "Save data",
                "launch-apps" => "Start apps",
                other => other,
            };
            format!("• {text}")
        })
        .collect();
    format!(
        "It can:\n{}\n\nYou're asked once for this version of the pipeline.",
        lines.join("\n")
    )
}

#[tauri::command(async)]
pub(crate) fn list_pipelines(
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<Pipeline>, String> {
    runtime.list_pipelines().map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub(crate) fn save_pipeline(
    pipeline: Pipeline,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Pipeline, String> {
    let saved = runtime
        .save_pipeline(pipeline)
        .map_err(|error| error.to_string())?;
    link::refresh(&app);
    Ok(saved)
}

#[tauri::command(async)]
pub(crate) fn delete_pipeline(
    id: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    runtime
        .delete_pipeline(&id)
        .map_err(|error| error.to_string())?;
    link::refresh(&app);
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn run_pipeline(
    request_id: String,
    id: String,
    inputs: Vec<ToolValue>,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<std::collections::HashMap<String, Vec<ToolValue>>, String> {
    let active = link::OutboundRequest::begin(request_id)?;
    runtime
        .run_saved_pipeline(&id, inputs, &active.cancelled)
        .map_err(|error| error.to_string())
}
