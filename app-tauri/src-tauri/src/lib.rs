mod commands;

#[tauri::command]
fn get_version() -> String {
    "0.1.0".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_version,
            commands::spikes::spike_legacy_config_dir,
            commands::spikes::spike_list_ports,
            commands::spikes::spike_probe_hello
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
