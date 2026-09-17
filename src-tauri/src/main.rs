#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod service;
use service::{DocumentInfo, PdfService};
use tauri::{Manager, State};

#[tauri::command]
async fn open_document(service: State<'_, PdfService>) -> Result<Option<DocumentInfo>, String> {
    let path = tauri::async_runtime::spawn_blocking(|| rfd::FileDialog::new().add_filter("PDF documents", &["pdf"]).pick_file()).await.map_err(|e| e.to_string())?;
    match path { Some(path) => service.open(path).await.map(Some), None => Ok(None) }
}
#[tauri::command]
async fn open_example(app: tauri::AppHandle, service: State<'_, PdfService>) -> Result<DocumentInfo, String> {
    service.open(app.path().resource_dir().map_err(|e| e.to_string())?.join("resources/welcome.pdf")).await
}
#[tauri::command]
async fn render_page(service: State<'_, PdfService>, id: u64, page: u16, width: i32) -> Result<tauri::ipc::Response, String> {
    service.render(id, page, width).await.map(tauri::ipc::Response::new)
}
#[tauri::command]
async fn close_document(service: State<'_, PdfService>, id: u64) -> Result<(), String> { service.close(id).await }

fn main() {
    tauri::Builder::default().setup(|app| {
        let library = app.path().resource_dir()?.join("resources/pdfium/bin/pdfium.dll");
        app.manage(PdfService::start(library));
        Ok(())
    }).invoke_handler(tauri::generate_handler![open_document, open_example, render_page, close_document])
      .run(tauri::generate_context!()).expect("Desktop application failed");
}
