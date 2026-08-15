//! OPC Agent 入口:多 AI 员工协调的智能工作台(社区版,开源)。

mod commands;
mod db;
mod llm;
mod orchestrator;
mod voice;

#[cfg(feature = "vosk-stt")]
mod stt;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("无法获取应用数据目录");
            std::fs::create_dir_all(&data_dir).expect("无法创建应用数据目录");
            let db_path = data_dir.join(db::DB_FILE);
            db::open(db_path.clone()).expect("数据库初始化失败");
            app.manage(commands::AppState { db_path });
            #[cfg(feature = "vosk-stt")]
            app.manage(stt::SttState::new(data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_employees,
            commands::create_employee,
            commands::update_employee,
            commands::delete_employee,
            commands::list_conversations,
            commands::get_messages,
            commands::delete_conversation,
            commands::get_dispatch_logs,
            commands::get_settings,
            commands::save_settings,
            commands::speak,
            commands::get_edition,
            commands::send_boss_message,
            #[cfg(feature = "vosk-stt")]
            stt::stt_status,
            #[cfg(feature = "vosk-stt")]
            stt::stt_transcribe_pcm,
            #[cfg(feature = "vosk-stt")]
            stt::stt_download_model,
        ])
        .run(tauri::generate_context!())
        .expect("OPC Agent 启动失败");
}
