#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod service;
mod editor;
mod printing;
mod print_commands;
mod document_properties;
mod text_geometry;
mod split;
mod combine;
use service::{DocumentInfo, PdfService};
use tauri::{Manager, State};

#[tauri::command]
async fn open_document(app: tauri::AppHandle, service: State<'_, PdfService>) -> Result<Option<service::OpenResult>, String> {
    let window = app.get_webview_window("main").ok_or("Application window is unavailable")?;
    let path = tauri::async_runtime::spawn_blocking(move || rfd::FileDialog::new().set_parent(&window).add_filter("PDF documents", &["pdf"]).pick_file()).await.map_err(|e| e.to_string())?;
    match path { Some(path) => service.begin_open(path).await.map(Some), None => Ok(None) }
}
#[tauri::command]
async fn reopen_document(service: State<'_, PdfService>, path: String) -> Result<service::OpenResult, String> {
    service.begin_open(std::path::PathBuf::from(path)).await
}
#[tauri::command]
async fn unlock_document(service: State<'_, PdfService>, request_id: u64, password: String) -> Result<service::OpenResult, String> { service.unlock(request_id, password).await }
#[tauri::command]
async fn cancel_password_request(service: State<'_, PdfService>, request_id: u64) -> Result<(), String> { service.cancel_password(request_id).await }
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

#[tauri::command]
async fn page_text(service: State<'_, PdfService>, id: u64, page: u16, revision: u64) -> Result<String, String> { service.text(id, page, revision).await }

#[tauri::command]
async fn page_text_geometry(service: State<'_, PdfService>, id: u64, page: u16, revision: u64) -> Result<text_geometry::PageTextGeometry, String> { service.text_geometry(id, page, revision).await }

#[tauri::command]
async fn document_bookmarks(service: State<'_, PdfService>, id: u64, revision: u64) -> Result<service::BookmarkList, String> { service.bookmarks(id, revision).await }

#[tauri::command]
async fn document_properties(service: State<'_, PdfService>, id: u64, revision: u64) -> Result<document_properties::DocumentProperties, String> { service.properties(id, revision).await }

#[tauri::command]
async fn dependency_notices(app: tauri::AppHandle) -> Result<String, String> {
    let path = app.path().resource_dir().map_err(|e| e.to_string())?.join("resources/third-party-licenses/THIRD-PARTY-NOTICES.txt");
    tauri::async_runtime::spawn_blocking(move || std::fs::read_to_string(path).map_err(|e| format!("Could not read bundled notices: {e}"))).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn edit_pages(service: State<'_, PdfService>, id: u64, edit: editor::PageEdit) -> Result<DocumentInfo, String> {
    service.edit(id, edit).await
}
#[tauri::command]
async fn crop_page(service: State<'_, PdfService>, id: u64, page: u16, revision: u64, rect: service::CropRect) -> Result<DocumentInfo, String> {
    service.crop(id, page, revision, rect).await
}
#[tauri::command]
async fn save_copy(app: tauri::AppHandle, service: State<'_, PdfService>, id: u64, pages: Option<Vec<usize>>) -> Result<Option<service::SavedCopy>, String> {
    let window = app.get_webview_window("main").ok_or("Application window is unavailable")?;
    let suggested = if pages.is_some() { "extracted-pages.pdf" } else { "organized-copy.pdf" };
    let path = tauri::async_runtime::spawn_blocking(move || rfd::FileDialog::new().set_parent(&window).add_filter("PDF documents", &["pdf"]).set_file_name(suggested).save_file()).await.map_err(|e| e.to_string())?;
    match path { Some(path) => service.save(id, pages, path).await.map(Some), None => Ok(None) }
}

#[tauri::command]
async fn split_document(app: tauri::AppHandle, service: State<'_, PdfService>, id: u64, revision: u64, pages_per_file: usize) -> Result<Option<split::SplitOutput>, String> {
    let window = app.get_webview_window("main").ok_or("Application window is unavailable")?;
    let folder = tauri::async_runtime::spawn_blocking(move || rfd::FileDialog::new().set_parent(&window).set_title("Choose a new folder for split PDFs").set_file_name("Split PDFs").save_file()).await.map_err(|error| error.to_string())?;
    match folder { Some(folder) => service.split(id, revision, pages_per_file, folder).await.map(Some), None => Ok(None) }
}

#[tauri::command]
async fn combine_documents(app: tauri::AppHandle, service: State<'_, PdfService>, first: service::CombineSource, second: service::CombineSource) -> Result<Option<service::SavedCopy>, String> {
    service.check_combine(first, second).await?;
    let window = app.get_webview_window("main").ok_or("Application window is unavailable")?;
    let path = tauri::async_runtime::spawn_blocking(move || rfd::FileDialog::new().set_parent(&window).set_title("Save combined PDF as a new file").add_filter("PDF documents", &["pdf"]).set_file_name("combined.pdf").save_file()).await.map_err(|error| error.to_string())?;
    match path { Some(path) => service.combine(first, second, path).await.map(Some), None => Ok(None) }
}

#[tauri::command]
async fn insert_pages_copy(app: tauri::AppHandle, service: State<'_, PdfService>, target: service::CombineSource, donor: service::CombineSource, at: usize) -> Result<Option<service::SavedCopy>, String> {
    service.check_insertion(target, donor, at).await?;
    let window = app.get_webview_window("main").ok_or("Application window is unavailable")?;
    let path = tauri::async_runtime::spawn_blocking(move || rfd::FileDialog::new().set_parent(&window).set_title("Save PDF with inserted pages as a new file").add_filter("PDF documents", &["pdf"]).set_file_name("inserted-pages.pdf").save_file()).await.map_err(|error| error.to_string())?;
    match path { Some(path) => service.insert_pages_copy(target, donor, at, path).await.map(Some), None => Ok(None) }
}

#[tauri::command]
async fn replace_pages_copy(app: tauri::AppHandle, service: State<'_, PdfService>, target: service::CombineSource, donor: service::CombineSource, start: usize, count: usize) -> Result<Option<service::SavedCopy>, String> {
    service.check_replacement(target, donor, start, count).await?;
    let window = app.get_webview_window("main").ok_or("Application window is unavailable")?;
    let path = tauri::async_runtime::spawn_blocking(move || rfd::FileDialog::new().set_parent(&window).set_title("Save PDF with replaced pages as a new file").add_filter("PDF documents", &["pdf"]).set_file_name("replaced-pages.pdf").save_file()).await.map_err(|error| error.to_string())?;
    match path { Some(path) => service.replace_pages_copy(target, donor, start, count, path).await.map(Some), None => Ok(None) }
}

fn main() {
    tauri::Builder::default().plugin(tauri_plugin_updater::Builder::new().build()).setup(|app| {
        let library = app.path().resource_dir()?.join("resources/pdfium/bin/pdfium.dll");
        app.manage(PdfService::start(library));
        app.manage(print_commands::PrintJobs::default());
        Ok(())
    }).invoke_handler(tauri::generate_handler![open_document, reopen_document, open_example, render_page, close_document, edit_pages, crop_page, save_copy, split_document, combine_documents, insert_pages_copy, replace_pages_copy, page_text, page_text_geometry, document_bookmarks, document_properties, dependency_notices, unlock_document, cancel_password_request, print_commands::print_document, print_commands::cancel_print])
      .run(tauri::generate_context!()).expect("Desktop application failed");
}
