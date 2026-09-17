use std::{collections::{HashMap, VecDeque}, io::Cursor, path::PathBuf, sync::{mpsc, OnceLock}};
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use crate::editor::{CropBox, EditSession, PageEdit, PageSpec, write_new_file};

#[derive(Clone, Copy, Deserialize)]
pub struct CropRect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
#[derive(Clone, Copy, Deserialize)]
pub struct CombineSource { pub id: u64, pub revision: u64 }

#[derive(Clone, Serialize)]
pub struct PageSize { width: f32, height: f32 }
#[derive(Clone, Serialize)]
pub struct DocumentInfo { id: u64, name: String, path: String, pages: Vec<PageSize>, revision: u64, dirty: bool, can_undo: bool, can_redo: bool }
#[derive(Serialize)]
pub struct SavedCopy { path: String, document: DocumentInfo }
#[derive(Serialize)]
pub struct BookmarkInfo { title: String, page: Option<usize>, depth: usize }
#[derive(Serialize)]
pub struct BookmarkList { items: Vec<BookmarkInfo>, truncated: bool }
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OpenResult {
    Opened { document: DocumentInfo },
    PasswordRequired { request_id: u64, name: String, incorrect: bool },
}
pub struct PrintSnapshotInfo { pub token: u64, pub pages: usize, pub name: String, cleanup: mpsc::Sender<Request> }
impl Drop for PrintSnapshotInfo {
    fn drop(&mut self) {
        let (reply, _) = oneshot::channel();
        let _ = self.cleanup.send(Request::EndPrint(self.token, reply));
    }
}
pub struct PrintBitmap { pub width: u32, pub height: u32, pub bgra: Vec<u8> }
type Reply<T> = oneshot::Sender<Result<T, String>>;
enum Request {
    #[cfg(test)]
    OpenDocumentsForPath(PathBuf, Reply<Vec<u64>>),
    Open(PathBuf, Reply<DocumentInfo>),
    BeginOpen(PathBuf, Reply<OpenResult>),
    Unlock(u64, String, Reply<OpenResult>),
    CancelPassword(u64, Reply<()>),
    BeginPrint(u64, u64, Reply<PrintSnapshotInfo>),
    PrintRender(u64, usize, u32, u32, Reply<PrintBitmap>),
    EndPrint(u64, Reply<()>),
    Render(u64, u16, i32, Reply<Vec<u8>>),
    Text(u64, u16, u64, Reply<String>),
    TextGeometry(u64, u16, u64, Reply<crate::text_geometry::PageTextGeometry>),
    Bookmarks(u64, u64, Reply<BookmarkList>),
    Properties(u64, u64, Reply<crate::document_properties::DocumentProperties>),
    Close(u64, Reply<()>),
    Edit(u64, PageEdit, Reply<DocumentInfo>),
    Crop(u64, u16, u64, CropRect, Reply<DocumentInfo>),
    Save(u64, Option<Vec<usize>>, PathBuf, Reply<SavedCopy>),
    Split(u64, u64, usize, PathBuf, Reply<crate::split::SplitOutput>),
    CheckCombine(CombineSource, CombineSource, Reply<()>),
    Combine(CombineSource, CombineSource, PathBuf, Reply<SavedCopy>),
}
#[derive(Clone)]
pub struct PdfService { sender: mpsc::Sender<Request> }
static PDF_SERVICE: OnceLock<PdfService> = OnceLock::new();
type CacheKey = (u64, u16, i32);
struct Cache { entries: VecDeque<(CacheKey, Vec<u8>, usize)>, weight: usize }
impl Cache {
    fn get(&mut self, key: CacheKey) -> Option<Vec<u8>> {
        let i = self.entries.iter().position(|(k, _, _)| *k == key)?;
        let item = self.entries.remove(i)?;
        let bytes = item.1.clone(); self.entries.push_back(item); Some(bytes)
    }
    fn insert(&mut self, key: CacheKey, bytes: Vec<u8>, weight: usize) {
        const BUDGET: usize = 512 * 1024 * 1024;
        while self.weight + weight > BUDGET {
            if let Some((_, _, size)) = self.entries.pop_front() { self.weight -= size; } else { break; }
        }
        if weight <= BUDGET { self.weight += weight; self.entries.push_back((key, bytes, weight)); }
    }
    fn close(&mut self, id: u64) { self.entries.retain(|(key, _, _)| key.0 != id); self.weight = self.entries.iter().map(|(_, _, w)| w).sum(); }
}
impl PdfService {
    pub fn start(library: PathBuf) -> Self {
        PDF_SERVICE.get_or_init(|| Self::start_worker(library)).clone()
    }
    fn start_worker(library: PathBuf) -> Self {
        let (sender, receiver) = mpsc::channel();
        let snapshot_cleanup = sender.clone();
        std::thread::Builder::new().name("pdf-worker".into()).spawn(move || {
            let pdfium = Pdfium::bind_to_library(library).map(Pdfium::new).map_err(|e| format!("PDF engine could not start: {e}"));
            let mut documents = HashMap::new();
            let mut sessions = HashMap::<u64, (EditSession, DocumentInfo)>::new();
            let mut cache = Cache { entries: VecDeque::new(), weight: 0 };
            let mut next_id = 1;
            let mut pending = HashMap::<u64, PathBuf>::new();
            let mut next_request = 1;
            let mut print_snapshots = HashMap::<u64, (std::rc::Rc<PdfDocument<'_>>, Vec<crate::editor::PageSpec>)>::new();
            let mut next_print = 1;
            while let Ok(request) = receiver.recv() {
                let request = match request {
                    Request::BeginOpen(path, reply) => {
                        if reply.is_closed() { continue; }
                        if pending.len() >= 8 { let _ = reply.send(Err("Close an existing password prompt before opening another PDF.".into())); continue; }
                        let request_id = next_request; next_request += 1;
                        pending.insert(request_id, path);
                        Request::Unlock(request_id, String::new(), reply)
                    }
                    other => other,
                };
                match request {
                    #[cfg(test)]
                    Request::OpenDocumentsForPath(path, reply) => {
                        let ids = sessions.iter().filter_map(|(id, (_, info))| (PathBuf::from(&info.path) == path).then_some(*id)).collect();
                        let _ = reply.send(Ok(ids));
                    }
                    Request::BeginPrint(id, revision, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            if print_snapshots.len() >= 4 { return Err("Wait for another print job to finish.".into()); }
                            let (session, info) = sessions.get(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Start printing again.".into()); }
                            let document: &std::rc::Rc<PdfDocument<'_>> = documents.get(&id).ok_or("Document is closed")?;
                            if !matches!(document.permissions().security_handler_revision(), Ok(PdfSecurityHandlerRevision::Unprotected)) { return Err("Printing encrypted or restricted PDFs is not supported in this build.".into()); }
                            let token = next_print; next_print += 1;
                            print_snapshots.insert(token, (document.clone(), session.plan.clone()));
                            Ok(PrintSnapshotInfo { token, pages: session.plan.len(), name: info.name.clone(), cleanup: snapshot_cleanup.clone() })
                        })();
                        if let Err(Ok(snapshot)) = reply.send(result) { print_snapshots.remove(&snapshot.token); }
                    }
                    Request::EndPrint(token, reply) => { print_snapshots.remove(&token); let _ = reply.send(Ok(())); }
                    Request::PrintRender(token, index, max_width, max_height, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (document, plan) = print_snapshots.get(&token).ok_or("Print job has ended")?;
                            let spec = plan.get(index).ok_or("Print page is out of range")?;
                            with_planned_page(document, spec, |page| {
                                let (width, height) = print_dimensions(page.width().value, page.height().value, max_width, max_height)?;
                                let bitmap = page.render_with_config(&PdfRenderConfig::new().set_fixed_size(width as i32, height as i32).set_format(PdfBitmapFormat::BGRA).set_reverse_byte_order(false).clear_before_rendering(true).set_clear_color(PdfColor::WHITE).render_annotations(true).render_form_data(true).use_print_quality(true)).map_err(|error| error.to_string())?;
                                let bgra = bitmap.as_raw_bytes();
                                if bitmap.width() != width as i32 || bitmap.height() != height as i32 || bgra.len() != width as usize * height as usize * 4 { return Err("Unexpected print bitmap layout.".into()); }
                                Ok(PrintBitmap { width, height, bgra })
                            })
                        })();
                        let _ = reply.send(result);
                    }
                    Request::BeginOpen(_, _) => unreachable!(),
                    Request::CancelPassword(id, reply) => { pending.remove(&id); let _ = reply.send(Ok(())); }
                    Request::Unlock(request_id, password, reply) => {
                        if reply.is_closed() { pending.remove(&request_id); continue; }
                        let result = (|| {
                            let path = pending.get(&request_id).ok_or("Password request expired. Open the PDF again.")?.clone();
                            let engine = pdfium.as_ref().map_err(Clone::clone)?;
                            if password.contains('\0') { return Err("Passwords cannot contain a null character.".into()); }
                            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                            let document = match engine.load_pdf_from_byte_vec(bytes.clone(), Some(&password)) {
                                Ok(document) => document,
                                Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => return Ok(OpenResult::PasswordRequired { request_id, name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), incorrect: !password.is_empty() }),
                                Err(error) => return Err(format!("Unable to open PDF: {error}")),
                            };
                            let pages = page_sizes(&document)?;
                            let id = next_id; next_id += 1;
                            let info = DocumentInfo { id, name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path: path.to_string_lossy().into_owned(), pages, revision: 0, dirty: false, can_undo: false, can_redo: false };
                            sessions.insert(id, (EditSession::new(bytes, info.pages.len()), info.clone()));
                            documents.insert(id, std::rc::Rc::new(document));
                            Ok(OpenResult::Opened { document: info })
                        })();
                        if !matches!(&result, Ok(OpenResult::PasswordRequired { .. })) { pending.remove(&request_id); }
                        if let Err(result) = reply.send(result) {
                            pending.remove(&request_id);
                            if let Ok(OpenResult::Opened { document }) = result { documents.remove(&document.id); sessions.remove(&document.id); }
                        }
                    }
                    Request::Open(path, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let engine = pdfium.as_ref().map_err(Clone::clone)?;
                            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                            let document = engine.load_pdf_from_byte_vec(bytes.clone(), None).map_err(|e| format!("Unable to open PDF: {e}"))?;
                            let pages = page_sizes(&document)?;
                            let id = next_id; next_id += 1;
                            let info = DocumentInfo { id, name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path: path.to_string_lossy().into_owned(), pages, revision: 0, dirty: false, can_undo: false, can_redo: false };
                            sessions.insert(id, (EditSession::new(bytes, info.pages.len()), info.clone()));
                            documents.insert(id, std::rc::Rc::new(document)); Ok(info)
                        })();
                        if let Err(Ok(info)) = reply.send(result) { documents.remove(&info.id); sessions.remove(&info.id); }
                    },
                    Request::Render(id, page, width, reply) => {
                        if reply.is_closed() { continue; }
                        let width = width.clamp(64, 3000);
                        let key = (id, page, width);
                        let result = if let Some(bytes) = cache.get(key) { Ok(bytes) } else {
                            (|| {
                                let document = documents.get(&id).ok_or("Document is closed")?;
                                let spec = sessions.get(&id).ok_or("Document is closed")?.0.plan.get(page as usize).ok_or("Page is out of range")?;
                                let image = with_planned_page(document, spec, |page| {
                                    let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(width).set_maximum_height(5000)).map_err(|e| e.to_string())?;
                                    bitmap.as_image().map_err(|e| e.to_string())
                                })?;
                                let mut bytes = Cursor::new(Vec::new());
                                image.write_to(&mut bytes, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                                let bytes = bytes.into_inner();
                                let weight = bytes.len() + image.width() as usize * image.height() as usize * 4;
                                cache.insert(key, bytes.clone(), weight); Ok(bytes)
                            })()
                        };
                        let _ = reply.send(result);
                    },
                    Request::Properties(id, revision, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (session, original) = sessions.get(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Reopen document properties.".into()); }
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            let dimensions = current_info(session, original, document)?.pages.iter().map(|page| (page.width, page.height)).collect::<Vec<_>>();
                            crate::document_properties::inspect(document, session.source.len(), &dimensions)
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Bookmarks(id, revision, reply) => {
                        let result = (|| {
                            let (session, _) = sessions.get(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Reopen bookmarks.".into()); }
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            let mut items = Vec::new();
                            let mut depths = HashMap::new();
                            let mut truncated = false;
                            for bookmark in document.bookmarks().iter().take(1001) {
                                if items.len() == 1000 { truncated = true; break; }
                                let depth = bookmark.parent().and_then(|parent| depths.get(&parent).copied()).map_or(0, |depth: usize| depth + 1);
                                depths.insert(bookmark.clone(), depth);
                                let source = bookmark.destination().and_then(|destination| destination.page_index().ok());
                                let page = source.and_then(|source| session.plan.iter().position(|spec| spec.source == source as usize));
                                items.push(BookmarkInfo { title: bookmark.title().filter(|s| !s.is_empty()).unwrap_or_else(|| "Untitled bookmark".into()), page, depth });
                            }
                            Ok(BookmarkList { items, truncated })
                        })();
                        let _ = reply.send(result);
                    }
                    Request::TextGeometry(id, index, revision, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (session, _) = sessions.get(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Select text again.".into()); }
                            let spec = session.plan.get(index as usize).ok_or("Page is out of range")?;
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            with_planned_page(document, spec, |page| crate::text_geometry::inspect(page, id, index, revision))
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Text(id, page, revision, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (session, _) = sessions.get(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Search again.".into()); }
                            let spec = session.plan.get(page as usize).ok_or("Page is out of range")?;
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            with_planned_page(document, spec, |source| {
                                let text = source.text().map_err(|e| e.to_string())?;
                                let visible = source.boundaries().bounding().map_err(|e| e.to_string())?.bounds;
                                Ok(text.inside_rect(visible))
                            })
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Edit(id, edit, reply) => {
                        let result = (|| {
                            let (session, original) = sessions.get_mut(&id).ok_or("Document is closed")?;
                            session.apply(edit)?;
                            cache.close(id);
                            current_info(session, original, documents.get(&id).ok_or("Document is closed")?)
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Crop(id, index, revision, rect, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (session, original) = sessions.get_mut(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Open the crop tool again.".into()); }
                            let spec = session.plan.get(index as usize).ok_or("Page is out of range")?;
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            let crop = with_planned_page(document, spec, |page| displayed_crop(page, rect))?;
                            let mut proposed = spec.clone(); proposed.crop = Some(crop);
                            with_planned_page(document, &proposed, |page| {
                                let (width, height) = (page.width().value, page.height().value);
                                if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 { return Err("The crop produces invalid page dimensions.".into()); }
                                Ok(())
                            })?;
                            session.apply(PageEdit::Crop { page: index as usize, crop })?;
                            cache.close(id);
                            current_info(session, original, document)
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Split(id, revision, pages_per_file, folder, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (session, _) = sessions.get(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Start splitting again.".into()); }
                            let parent = folder.parent().ok_or("Choose a new folder for the split PDFs.")?;
                            let name = folder.file_name().ok_or("Choose a new folder name for the split PDFs.")?;
                            let engine = pdfium.as_ref().map_err(Clone::clone)?;
                            crate::split::prepare_and_publish(session, parent, name, pages_per_file, |bytes, expected| {
                                if reply.is_closed() { return Err("Split was canceled.".into()); }
                                let document = engine.load_pdf_from_byte_slice(bytes, None).map_err(|error| format!("Output could not be opened: {error}"))?;
                                if document.pages().len() as usize != expected { return Err("Output page count differs from the split plan.".into()); }
                                for index in 0..document.pages().len() {
                                    let page = document.pages().get(index).map_err(|error| format!("Output page {} could not be read: {error}", index + 1))?;
                                    let (width, height) = (page.width().value, page.height().value);
                                    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 { return Err(format!("Output page {} has invalid dimensions.", index + 1)); }
                                    let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(64).set_maximum_height(64)).map_err(|error| format!("Output page {} could not be rendered: {error}", index + 1))?;
                                    if bitmap.width() <= 0 || bitmap.height() <= 0 { return Err(format!("Output page {} has an invalid bitmap.", index + 1)); }
                                }
                                Ok(())
                            })
                        })();
                        let _ = reply.send(result);
                    }
                    Request::CheckCombine(first, second, reply) => {
                        if reply.is_closed() { continue; }
                        let result = combine_sessions(&sessions, first, second).and_then(|(first, second)| crate::combine::validate_sources(first, second).map(|_| ()));
                        let _ = reply.send(result);
                    }
                    Request::Combine(first, second, path, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (first, second) = combine_sessions(&sessions, first, second)?;
                            let engine = pdfium.as_ref().map_err(Clone::clone)?;
                            let mut prepared = None;
                            let bytes = crate::combine::prepare_and_write(first, second, &path, |bytes, expected| {
                                if reply.is_closed() { return Err("Combine was canceled.".into()); }
                                let document = engine.load_pdf_from_byte_vec(bytes.to_vec(), None).map_err(|error| format!("Output could not be opened: {error}"))?;
                                let pages = page_sizes(&document)?;
                                if pages.len() != expected { return Err("Output page count differs from the combine plan.".into()); }
                                for index in 0..document.pages().len() {
                                    let page = document.pages().get(index).map_err(|error| format!("Output page {} could not be read: {error}", index + 1))?;
                                    let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(64).set_maximum_height(64)).map_err(|error| format!("Output page {} could not be rendered: {error}", index + 1))?;
                                    if bitmap.width() <= 0 || bitmap.height() <= 0 { return Err(format!("Output page {} has an invalid bitmap.", index + 1)); }
                                }
                                if reply.is_closed() { return Err("Combine was canceled.".into()); }
                                prepared = Some((document, pages));
                                Ok(())
                            })?;
                            let (document, pages) = prepared.ok_or("Combined output was not validated")?;
                            let id = next_id; next_id += 1;
                            let info = DocumentInfo { id, name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path: path.to_string_lossy().into_owned(), pages, revision: 0, dirty: false, can_undo: false, can_redo: false };
                            sessions.insert(id, (EditSession::new(bytes, info.pages.len()), info.clone()));
                            documents.insert(id, std::rc::Rc::new(document));
                            Ok(SavedCopy { path: path.to_string_lossy().into_owned(), document: info })
                        })();
                        if let Err(Ok(saved)) = reply.send(result) { documents.remove(&saved.document.id); sessions.remove(&saved.document.id); }
                    }
                    Request::Save(id, pages, path, reply) => {
                        let result = (|| {
                            let (session, original) = sessions.get_mut(&id).ok_or("Document is closed")?;
                            if path == PathBuf::from(&original.path) { return Err("Choose a new filename to preserve the source PDF.".into()); }
                            let bytes = session.export(pages.as_deref())?;
                            let engine = pdfium.as_ref().map_err(Clone::clone)?;
                            let check = engine.load_pdf_from_byte_slice(&bytes, None).map_err(|e| format!("Output validation failed: {e}"))?;
                            let expected = pages.as_ref().map_or(session.plan.len(), |p| p.iter().collect::<std::collections::HashSet<_>>().len());
                            if check.pages().len() as usize != expected { return Err("Output page count validation failed.".into()); }
                            write_new_file(&path, &bytes)?;
                            if pages.is_none() { session.mark_saved(); }
                            Ok(SavedCopy { path: path.to_string_lossy().into_owned(), document: current_info(session, original, documents.get(&id).ok_or("Document is closed")?)? })
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Close(id, reply) => { documents.remove(&id); sessions.remove(&id); cache.close(id); let _ = reply.send(Ok(())); }
                }
            }
        }).expect("Could not start PDF worker");
        Self { sender }
    }
    pub async fn open(&self, path: PathBuf) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Open(path, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn begin_open(&self, path: PathBuf) -> Result<OpenResult, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::BeginOpen(path, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn begin_print(&self, id: u64, revision: u64) -> Result<PrintSnapshotInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::BeginPrint(id, revision, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn end_print(&self, token: u64) -> Result<(), String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::EndPrint(token, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub fn print_render_blocking(&self, token: u64, page: usize, max_width: u32, max_height: u32) -> Result<PrintBitmap, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::PrintRender(token, page, max_width, max_height, tx)).map_err(|error| error.to_string())?; rx.blocking_recv().map_err(|error| error.to_string())?
    }
    pub async fn unlock(&self, id: u64, password: String) -> Result<OpenResult, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Unlock(id, password, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn cancel_password(&self, id: u64) -> Result<(), String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::CancelPassword(id, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn render(&self, id: u64, page: u16, width: i32) -> Result<Vec<u8>, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Render(id, page, width, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn close(&self, id: u64) -> Result<(), String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Close(id, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn text(&self, id: u64, page: u16, revision: u64) -> Result<String, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Text(id, page, revision, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn text_geometry(&self, id: u64, page: u16, revision: u64) -> Result<crate::text_geometry::PageTextGeometry, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::TextGeometry(id, page, revision, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn bookmarks(&self, id: u64, revision: u64) -> Result<BookmarkList, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Bookmarks(id, revision, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn properties(&self, id: u64, revision: u64) -> Result<crate::document_properties::DocumentProperties, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Properties(id, revision, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn edit(&self, id: u64, edit: PageEdit) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Edit(id, edit, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn crop(&self, id: u64, page: u16, revision: u64, rect: CropRect) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Crop(id, page, revision, rect, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn save(&self, id: u64, pages: Option<Vec<usize>>, path: PathBuf) -> Result<SavedCopy, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Save(id, pages, path, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn split(&self, id: u64, revision: u64, pages_per_file: usize, folder: PathBuf) -> Result<crate::split::SplitOutput, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Split(id, revision, pages_per_file, folder, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn check_combine(&self, first: CombineSource, second: CombineSource) -> Result<(), String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::CheckCombine(first, second, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn combine(&self, first: CombineSource, second: CombineSource, path: PathBuf) -> Result<SavedCopy, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Combine(first, second, path, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
}

fn combine_sessions(sessions: &HashMap<u64, (EditSession, DocumentInfo)>, first: CombineSource, second: CombineSource) -> Result<(&EditSession, &EditSession), String> {
    if first.id == second.id { return Err("Choose two different open PDFs to combine.".into()); }
    let first_session = &sessions.get(&first.id).ok_or("First document is closed")?.0;
    let second_session = &sessions.get(&second.id).ok_or("Second document is closed")?.0;
    if first_session.revision != first.revision || second_session.revision != second.revision { return Err("A document changed. Open Combine again.".into()); }
    Ok((first_session, second_session))
}

fn print_dimensions(width: f32, height: f32, max_width: u32, max_height: u32) -> Result<(u32, u32), String> {
    if max_width == 0 || max_height == 0 || !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 { return Err("Invalid print dimensions.".into()); }
    let limit_width = max_width.min(4096) as f64;
    let limit_height = max_height.min(4096) as f64;
    let scale = (limit_width / width as f64).min(limit_height / height as f64).min((16_000_000.0 / (width as f64 * height as f64)).sqrt());
    let output_width = (width as f64 * scale).floor().max(1.0) as u32;
    let output_height = (height as f64 * scale).floor().max(1.0) as u32;
    Ok((output_width, output_height))
}

fn page_sizes(document: &PdfDocument<'_>) -> Result<Vec<PageSize>, String> {
    let pages = document.pages();
    if pages.is_empty() { return Err("This PDF has no pages.".into()); }
    if pages.len() > u16::MAX as i32 + 1 { return Err("This PDF exceeds the supported limit of 65,536 pages.".into()); }
    (0..pages.len()).map(|index| {
        let page = pages.get(index).map_err(|error| format!("Unable to read page {}: {error}", index + 1))?;
        let size = PageSize { width: page.width().value, height: page.height().value };
        if !size.width.is_finite() || !size.height.is_finite() || size.width <= 0.0 || size.height <= 0.0 { return Err(format!("Invalid dimensions on page {}.", index + 1)); }
        Ok(size)
    }).collect()
}

fn with_planned_page<T>(document: &PdfDocument<'_>, spec: &PageSpec, action: impl FnOnce(&PdfPage<'_>) -> Result<T, String>) -> Result<T, String> {
    let mut page = document.pages().get(spec.source as i32).map_err(|error| error.to_string())?;
    let original_rotation = page.rotation().map_err(|error| error.to_string())?;
    let original_crop = if spec.crop.is_some() {
        Some(page.boundaries().crop().or_else(|_| page.boundaries().bounding()).map_err(|error| error.to_string())?.bounds)
    } else { None };
    if let Some(crop) = spec.crop { page.boundaries_mut().set_crop(PdfRect::new_from_values(crop.bottom, crop.left, crop.top, crop.right)).map_err(|error| error.to_string())?; }
    let turns = (original_rotation.as_degrees() as i32 / 90 + spec.turns).rem_euclid(4);
    page.set_rotation(match turns { 1 => PdfPageRenderRotation::Degrees90, 2 => PdfPageRenderRotation::Degrees180, 3 => PdfPageRenderRotation::Degrees270, _ => PdfPageRenderRotation::None });
    let result = action(&page);
    let restore = match original_crop { Some(crop) => page.boundaries_mut().set_crop(crop).map_err(|error| error.to_string()), None => Ok(()) };
    page.set_rotation(original_rotation);
    result.and_then(|value| restore.map(|_| value))
}

fn displayed_crop(page: &PdfPage<'_>, rect: CropRect) -> Result<CropBox, String> {
    if ![rect.x, rect.y, rect.width, rect.height].iter().all(|value| value.is_finite()) || rect.x < 0.0 || rect.y < 0.0 || rect.width <= 0.0 || rect.height <= 0.0 || rect.x + rect.width > 1.0 + 1e-12 || rect.y + rect.height > 1.0 + 1e-12 {
        return Err("Choose a nonempty crop rectangle within the current page.".into());
    }
    let bounds = page.boundaries().bounding().map_err(|error| error.to_string())?.bounds;
    let visible = CropBox { left: bounds.left().value, bottom: bounds.bottom().value, right: bounds.right().value, top: bounds.top().value };
    if rect.x == 0.0 && rect.y == 0.0 && rect.width == 1.0 && rect.height == 1.0 { visible.validate_within(visible)?; return Ok(visible); }
    const SIZE: i32 = 1_000_000;
    let config = PdfRenderConfig::new().set_fixed_size(SIZE, SIZE);
    // DeviceToPage takes integer pixels. Interpolate its page corners in f64 so a one-point crop is not rounded to a smaller pixel cell.
    let mut page_corners = [(0.0_f64, 0.0_f64); 4];
    for (index, (x, y)) in [(0, 0), (SIZE, 0), (0, SIZE), (SIZE, SIZE)].into_iter().enumerate() {
        let (x, y) = page.pixels_to_points(x, y, &config).map_err(|error| error.to_string())?;
        if !x.value.is_finite() || !y.value.is_finite() { return Err("The crop could not be mapped to the source page.".into()); }
        page_corners[index] = (f64::from(x.value), f64::from(y.value));
    }
    let mut crop = CropBox { left: f32::INFINITY, bottom: f32::INFINITY, right: f32::NEG_INFINITY, top: f32::NEG_INFINITY };
    for (x, y) in [(rect.x, rect.y), (rect.x + rect.width, rect.y), (rect.x, rect.y + rect.height), (rect.x + rect.width, rect.y + rect.height)] {
        let (x, y) = (x.min(1.0), y.min(1.0));
        let weights = [(1.0 - x) * (1.0 - y), x * (1.0 - y), (1.0 - x) * y, x * y];
        let point = weights.into_iter().zip(page_corners).fold((0.0, 0.0), |(x, y), (weight, (corner_x, corner_y))| (x + weight * corner_x, y + weight * corner_y));
        crop.left = crop.left.min(point.0 as f32); crop.bottom = crop.bottom.min(point.1 as f32); crop.right = crop.right.max(point.0 as f32); crop.top = crop.top.max(point.1 as f32);
    }
    crop.left = crop.left.max(visible.left); crop.bottom = crop.bottom.max(visible.bottom); crop.right = crop.right.min(visible.right); crop.top = crop.top.min(visible.top);
    crop.validate_within(visible)?;
    Ok(crop)
}

fn current_info(session: &EditSession, original: &DocumentInfo, document: &PdfDocument<'_>) -> Result<DocumentInfo, String> {
    let mut info = original.clone();
    info.pages = session.plan.iter().map(|spec| {
        if spec.crop.is_some() { return with_planned_page(document, spec, |page| Ok(PageSize { width: page.width().value, height: page.height().value })); }
        let size = &original.pages[spec.source];
        Ok(if spec.turns % 2 == 0 { size.clone() } else { PageSize { width: size.height, height: size.width } })
    }).collect::<Result<_, String>>()?;
    info.revision = session.revision; info.dirty = session.dirty(); info.can_undo = session.can_undo(); info.can_redo = session.can_redo(); Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Print admission is a process-wide four-job resource; isolate only its tests, not the PDF worker or the full suite.
    static PRINT_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn print_test_lock() -> std::sync::MutexGuard<'static, ()> {
        PRINT_TESTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn call<T>(service: &PdfService, request: impl FnOnce(Reply<T>) -> Request) -> Result<T, String> {
        let (tx, rx) = oneshot::channel(); service.sender.send(request(tx)).unwrap(); rx.blocking_recv().unwrap()
    }
    struct TestPrintSnapshot { service: PdfService, info: PrintSnapshotInfo }
    impl std::ops::Deref for TestPrintSnapshot {
        type Target = PrintSnapshotInfo;
        fn deref(&self) -> &Self::Target { &self.info }
    }
    impl Drop for TestPrintSnapshot {
        fn drop(&mut self) {
            let (tx, rx) = oneshot::channel();
            if self.service.sender.send(Request::EndPrint(self.info.token, tx)).is_ok() { let _ = rx.blocking_recv(); }
        }
    }
    fn print_snapshot(service: &PdfService, id: u64, revision: u64) -> TestPrintSnapshot {
        TestPrintSnapshot { service: service.clone(), info: call(service, |reply| Request::BeginPrint(id, revision, reply)).unwrap() }
    }
    struct UnreadPrintCleanup { service: PdfService, document: u64, tokens: Vec<u64> }
    impl Drop for UnreadPrintCleanup {
        fn drop(&mut self) {
            for token in &self.tokens {
                let (tx, rx) = oneshot::channel();
                if self.service.sender.send(Request::EndPrint(*token, tx)).is_ok() { let _ = rx.blocking_recv(); }
            }
            let (tx, rx) = oneshot::channel();
            if self.service.sender.send(Request::Close(self.document, tx)).is_ok() { let _ = rx.blocking_recv(); }
        }
    }
    #[test]
    fn print_unread_successful_replies_release_snapshot_capacity_when_receivers_are_dropped() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let info = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
        let bootstrap = print_snapshot(&service, info.id, info.revision);
        let first_token = bootstrap.token + 1;
        drop(bootstrap);
        let mut cleanup = UnreadPrintCleanup { service: service.clone(), document: info.id, tokens: Vec::new() };
        for index in 0..4 {
            let (tx, rx) = oneshot::channel();
            service.sender.send(Request::BeginPrint(info.id, info.revision, tx)).unwrap();
            call(&service, |reply| Request::Properties(info.id, info.revision, reply)).unwrap();
            assert!(!rx.is_empty(), "The worker barrier must establish that the print reply was already sent");
            cleanup.tokens.push(first_token + index);
            drop(rx);
            call(&service, |reply| Request::Properties(info.id, info.revision, reply)).unwrap();
        }
        let available = call(&service, |reply| Request::BeginPrint(info.id, info.revision, reply));
        assert!(available.is_ok(), "Dropped unread replies retained print capacity: {}", available.as_ref().err().map(String::as_str).unwrap_or(""));
        let available = TestPrintSnapshot { service: service.clone(), info: available.unwrap() };
        assert!(!service.print_render_blocking(available.token, 0, 100, 100).unwrap().bgra.is_empty());
        drop(available);
    }
    #[test]
    fn open_canceled_before_worker_processing_never_retains_an_unreachable_document() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap(); let path = folder.path().join("canceled-open.pdf");
        let source = std::fs::read(root.join("resources/welcome.pdf")).unwrap(); std::fs::write(&path, &source).unwrap();
        let (tx, rx) = oneshot::channel(); drop(rx);
        service.sender.send(Request::Open(path.clone(), tx)).unwrap();
        // The probe follows Open on the same FIFO and observes only this test's unique path.
        let retained = call(&service, |reply| Request::OpenDocumentsForPath(path.clone(), reply)).unwrap();
        for id in &retained {
            assert_eq!(call(&service, |reply| Request::Properties(*id, 0, reply)).unwrap().page_count, 6);
            call(&service, |reply| Request::Close(*id, reply)).unwrap();
        }
        assert_eq!(std::fs::read(&path).unwrap(), source);
        assert!(retained.is_empty(), "A canceled Open retained {} unreachable document(s)", retained.len());
    }
    #[test]
    fn print_snapshot_lease_survives_thread_handoff_source_close_and_releases_on_completion_or_unwind() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let reusable = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
        for unwind in [false, true] {
            let info = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
            let snapshot = call(&service, |reply| Request::BeginPrint(info.id, info.revision, reply)).unwrap(); let token = snapshot.token;
            let expected = service.print_render_blocking(token, 0, 100, 100).unwrap().bgra;
            let (ready, started) = std::sync::mpsc::channel(); let (finish, wait) = std::sync::mpsc::channel(); let worker = service.clone();
            let thread = std::thread::spawn(move || {
                let snapshot = snapshot;
                ready.send(()).unwrap(); wait.recv().unwrap();
                let rendered = worker.print_render_blocking(snapshot.token, 0, 100, 100).unwrap().bgra;
                if unwind { panic!("Exercise snapshot lease cleanup during spool-thread unwind"); }
                rendered
            });
            started.recv().unwrap();
            call(&service, |reply| Request::Close(info.id, reply)).unwrap();
            assert_eq!(service.print_render_blocking(token, 0, 100, 100).unwrap().bgra, expected);
            let held = (0..3).map(|_| print_snapshot(&service, reusable.id, 0)).collect::<Vec<_>>();
            assert!(call(&service, |reply| Request::BeginPrint(reusable.id, 0, reply)).err().unwrap().contains("another print job"), "The active spool lease must retain its admission slot");
            finish.send(()).unwrap(); let result = thread.join();
            if unwind { assert!(result.is_err()); } else { assert_eq!(result.unwrap(), expected); }
            call(&service, |reply| Request::Properties(reusable.id, 0, reply)).unwrap();
            assert!(service.print_render_blocking(token, 0, 100, 100).err().unwrap().contains("ended"));
            let replacement = print_snapshot(&service, reusable.id, 0);
            assert!(call(&service, |reply| Request::BeginPrint(reusable.id, 0, reply)).err().unwrap().contains("another print job"));
            drop(replacement); drop(held);
        }
        call(&service, |reply| Request::Close(reusable.id, reply)).unwrap();
    }
    #[test]
    fn print_snapshot_admission_limit_and_guard_cleanup_preserve_worker_capacity() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let info = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
        let held = (0..3).map(|_| print_snapshot(&service, info.id, 0)).collect::<Vec<_>>();
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _fourth = print_snapshot(&service, info.id, 0);
            assert!(call(&service, |reply| Request::BeginPrint(info.id, 0, reply)).err().unwrap().contains("another print job"));
            panic!("Exercise snapshot guard cleanup after a failed assertion");
        }));
        assert!(unwind.is_err());
        let replacement = print_snapshot(&service, info.id, 0);
        assert!(call(&service, |reply| Request::BeginPrint(info.id, 0, reply)).err().unwrap().contains("another print job"));
        drop(replacement); drop(held);
        let reusable = (0..4).map(|_| print_snapshot(&service, info.id, 0)).collect::<Vec<_>>();
        assert!(call(&service, |reply| Request::BeginPrint(info.id, 0, reply)).err().unwrap().contains("another print job"));
        drop(reusable);
        call(&service, |reply| Request::Close(info.id, reply)).unwrap();
    }
    fn assert_same_ink(actual: &image::RgbImage, expected: &image::RgbImage) {
        assert!(actual.width().abs_diff(expected.width()) <= 1 && actual.height().abs_diff(expected.height()) <= 1, "Unexpected crop dimensions: {:?} vs {:?}", actual.dimensions(), expected.dimensions());
        let ink = |image: &image::RgbImage| image.enumerate_pixels().filter(|(_, _, pixel)| pixel.0.iter().any(|value| *value < 100)).map(|(x, y, _)| (x, y)).collect::<Vec<_>>();
        let actual_ink = ink(actual); let expected_ink = ink(expected);
        assert!(!actual_ink.is_empty() && !expected_ink.is_empty());
        let compare = |points: &[(u32, u32)], source: &image::RgbImage, target: &image::RgbImage| {
            points.iter().filter(|(x, y)| {
                let x = (f64::from(*x) * f64::from(target.width()) / f64::from(source.width())).round() as i32;
                let y = (f64::from(*y) * f64::from(target.height()) / f64::from(source.height())).round() as i32;
                (-2..=2).any(|dy| (-2..=2).any(|dx| {
                    let (x, y) = (x + dx, y + dy);
                    x >= 0 && y >= 0 && x < target.width() as i32 && y < target.height() as i32 && target.get_pixel(x as u32, y as u32).0.iter().any(|value| *value < 100)
                }))
            }).count()
        };
        assert!(compare(&actual_ink, actual, expected) as f64 / actual_ink.len() as f64 > 0.99, "Crop has unexpected rendered ink");
        assert!(compare(&expected_ink, expected, actual) as f64 / expected_ink.len() as f64 > 0.99, "Crop lost expected rendered ink");
    }
    fn combine_source(info: &DocumentInfo) -> CombineSource { CombineSource { id: info.id, revision: info.revision } }
    #[test]
    fn combine_all_ordered_fixture_pairs_validate_every_output_page_and_preserve_source_history() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        let fixtures = [("resources/welcome.pdf", 6usize), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)];
        for (first_index, (first_fixture, first_count)) in fixtures.iter().enumerate() {
            for (second_index, (second_fixture, second_count)) in fixtures.iter().enumerate() {
                if first_index == second_index { continue; }
                let first_path = root.join(first_fixture); let second_path = root.join(second_fixture);
                let first_bytes = std::fs::read(&first_path).unwrap(); let second_bytes = std::fs::read(&second_path).unwrap();
                let mut first = call(&service, |reply| Request::Open(first_path.clone(), reply)).unwrap();
                let mut second = call(&service, |reply| Request::Open(second_path.clone(), reply)).unwrap();
                for edit in [PageEdit::Move { from: 0, to: 2 }, PageEdit::Rotate { pages: vec![0], clockwise: true }, PageEdit::Delete { pages: vec![1] }] {
                    first = call(&service, |reply| Request::Edit(first.id, edit, reply)).unwrap();
                }
                for edit in [PageEdit::Delete { pages: vec![1, 3] }, PageEdit::Rotate { pages: vec![0], clockwise: false }, PageEdit::Move { from: 0, to: 1 }] {
                    second = call(&service, |reply| Request::Edit(second.id, edit, reply)).unwrap();
                }
                let path = folder.path().join(format!("pair-{first_count}-{second_count}.pdf"));
                call(&service, |reply| Request::CheckCombine(combine_source(&first), combine_source(&second), reply)).unwrap();
                let started = std::time::Instant::now();
                let combined = call(&service, |reply| Request::Combine(combine_source(&first), combine_source(&second), path.clone(), reply)).unwrap().document;
                println!("combine {first_count}+{second_count}: {} current pages, validation/write {:?}", combined.pages.len(), started.elapsed());
                assert_ne!(combined.id, first.id); assert_ne!(combined.id, second.id);
                assert_eq!(combined.pages.len(), first_count + second_count - 3);
                assert_eq!(combined.revision, 0);
                assert!(!combined.dirty && !combined.can_undo && !combined.can_redo);
                let reopened = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
                for position in 0..combined.pages.len() {
                    let (source, local, original_page, count) = if position < first.pages.len() {
                        let original = match position { 0 => 1, 1 => 0, other => other + 1 };
                        (&first, position, original, first_count)
                    } else {
                        let local = position - first.pages.len();
                        let original = match local { 0 => 2, 1 => 0, other => other + 2 };
                        (&second, local, original, second_count)
                    };
                    let text = call(&service, |reply| Request::Text(combined.id, position as u16, 0, reply)).unwrap();
                    assert!(text.contains(&format!("Page {} of {count}", original_page + 1)), "Output source/order differs at page {position} of {first_count}+{second_count}");
                    assert_eq!((combined.pages[position].width, combined.pages[position].height), (source.pages[local].width, source.pages[local].height));
                    let expected = call(&service, |reply| Request::Render(source.id, local as u16, 64, reply)).unwrap();
                    let actual = call(&service, |reply| Request::Render(combined.id, position as u16, 64, reply)).unwrap();
                    let reopened_actual = call(&service, |reply| Request::Render(reopened.id, position as u16, 64, reply)).unwrap();
                    let decode = |bytes: &[u8]| image::load_from_memory(bytes).unwrap().into_rgba8();
                    assert_eq!(decode(&actual), decode(&expected), "Combined output render differs at page {position}");
                    assert_eq!(decode(&reopened_actual), decode(&expected), "Reopened output render differs at page {position}");
                }
                assert_eq!(std::fs::read(&first_path).unwrap(), first_bytes); assert_eq!(std::fs::read(&second_path).unwrap(), second_bytes);
                for source in [&first, &second] {
                    let properties = call(&service, |reply| Request::Properties(source.id, source.revision, reply)).unwrap();
                    assert_eq!(properties.page_count, source.pages.len());
                    for undo in 0..3 {
                        let restored = call(&service, |reply| Request::Edit(source.id, PageEdit::Undo, reply)).unwrap();
                        assert_eq!(restored.revision, source.revision + undo + 1); assert_eq!(restored.dirty, undo != 2); assert!(restored.can_redo);
                    }
                }
                for id in [first.id, second.id, combined.id, reopened.id] { call(&service, |reply| Request::Close(id, reply)).unwrap(); }
            }
        }
    }
    #[test]
    fn combine_cropped_inherited_original_and_edited_rotations_keep_independent_ink_and_visible_tokens() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        for original in [0, 90, 180, 270] {
            for edited in 0..4 {
                let mut sources = Vec::new(); let mut expected = Vec::new();
                for (index, label) in ["First", "Second"].into_iter().enumerate() {
                    let rotation = if index == 0 { original } else { (360 - original) % 360 };
                    let bytes = crate::text_geometry::tests::fixture(rotation, [1.0, 0.0, 0.0, 1.0, 100.0, 300.0], &format!("{label}OutsideTop\n\n{label}Header\n{label}Body\n\n{label}OutsideBottom"));
                    let mut pdf = lopdf::Document::load_mem(&bytes).unwrap(); let page = pdf.get_pages()[&1];
                    pdf.get_dictionary_mut(page).unwrap().set("MediaBox", vec![20.into(), 30.into(), 420.into(), 430.into()]);
                    let parent = pdf.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
                    for key in [b"MediaBox".as_slice(), b"CropBox", b"Rotate", b"Resources"] {
                        let value = pdf.get_dictionary_mut(page).unwrap().remove(key).unwrap(); pdf.get_dictionary_mut(parent).unwrap().set(key, value);
                    }
                    let path = folder.path().join(format!("source-{original}-{edited}-{index}.pdf")); pdf.save(&path).unwrap(); let source_bytes = std::fs::read(&path).unwrap();
                    let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
                    for _ in 0..edited { info = call(&service, |reply| Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: index == 0 }, reply)).unwrap(); }
                    let turns = (rotation / 90 + if index == 0 { edited } else { (4 - edited) % 4 }) % 4;
                    let (rect, region, old_width, new_width) = match turns {
                        0 => (CropRect { x: 0.1, y: 70.0 / 260.0, width: 0.8, height: 110.0 / 260.0 }, (90, 210, 720, 330), 900, 720),
                        1 => (CropRect { x: 80.0 / 260.0, y: 0.1, width: 110.0 / 260.0, height: 0.8 }, (240, 90, 330, 720), 780, 330),
                        2 => (CropRect { x: 0.1, y: 80.0 / 260.0, width: 0.8, height: 110.0 / 260.0 }, (90, 240, 720, 330), 900, 720),
                        _ => (CropRect { x: 70.0 / 260.0, y: 0.1, width: 110.0 / 260.0, height: 0.8 }, (210, 90, 330, 720), 780, 330),
                    };
                    let before = image::load_from_memory(&call(&service, |reply| Request::Render(info.id, 0, old_width, reply)).unwrap()).unwrap().into_rgb8();
                    expected.push((image::imageops::crop_imm(&before, region.0, region.1, region.2, region.3).to_image(), new_width, turns));
                    info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, rect, reply)).unwrap();
                    sources.push((info, path, source_bytes));
                }
                let path = folder.path().join(format!("combined-{original}-{edited}.pdf"));
                let combined = call(&service, |reply| Request::Combine(combine_source(&sources[0].0), combine_source(&sources[1].0), path.clone(), reply)).unwrap().document;
                let reopened = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
                let output = lopdf::Document::load(&path).unwrap();
                for (index, label) in ["First", "Second"].into_iter().enumerate() {
                    let (expected, width, turns) = &expected[index];
                    let dimensions = if turns % 2 == 0 { (240.0, 110.0) } else { (110.0, 240.0) };
                    assert_eq!((combined.pages[index].width, combined.pages[index].height), dimensions);
                    for id in [combined.id, reopened.id] {
                        let actual = image::load_from_memory(&call(&service, |reply| Request::Render(id, index as u16, *width, reply)).unwrap()).unwrap().into_rgb8(); assert_same_ink(&actual, expected);
                        let text = call(&service, |reply| Request::Text(id, index as u16, 0, reply)).unwrap();
                        let geometry = call(&service, |reply| Request::TextGeometry(id, index as u16, 0, reply)).unwrap(); assert_eq!(geometry.status, "ok");
                        let copied = geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>();
                        for token in [format!("{label}Header"), format!("{label}Body")] { assert!(text.contains(&token), "{original}/{edited}: {text}"); assert!(copied.contains(&token), "{original}/{edited}: {copied}"); }
                        for token in [format!("{label}OutsideTop"), format!("{label}OutsideBottom")] { assert!(!text.contains(&token) && !copied.contains(&token)); }
                    }
                    assert!(String::from_utf8_lossy(&output.get_page_content(output.get_pages()[&(index as u32 + 1)])).contains(&format!("{label}OutsideBottom")), "Crop must retain the underlying hidden content");
                    let (source, source_path, source_bytes) = &sources[index]; assert_eq!(std::fs::read(source_path).unwrap(), *source_bytes);
                    let undo = call(&service, |reply| Request::Edit(source.id, PageEdit::Undo, reply)).unwrap(); assert_eq!(undo.revision, source.revision + 1); assert!(undo.can_redo);
                    call(&service, |reply| Request::Close(source.id, reply)).unwrap();
                }
                for id in [combined.id, reopened.id] { call(&service, |reply| Request::Close(id, reply)).unwrap(); }
            }
        }
    }
    #[test]
    fn combine_duplicate_stale_and_closed_after_preflight_never_write_or_mutate_sources() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let first = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
        let second = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
        let path = folder.path().join("must-not-combine.pdf");
        let combine = |first, second| call(&service, |reply| Request::Combine(first, second, path.clone(), reply));
        assert!(combine(combine_source(&first), combine_source(&first)).err().unwrap().contains("different"));
        call(&service, |reply| Request::CheckCombine(combine_source(&first), combine_source(&second), reply)).unwrap();
        let changed = call(&service, |reply| Request::Edit(second.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap();
        assert!(combine(combine_source(&first), combine_source(&second)).err().unwrap().contains("changed")); assert!(!path.exists());
        let restored = call(&service, |reply| Request::Edit(second.id, PageEdit::Undo, reply)).unwrap(); assert!(!restored.dirty && restored.can_redo);
        call(&service, |reply| Request::CheckCombine(combine_source(&first), combine_source(&restored), reply)).unwrap();
        call(&service, |reply| Request::Close(second.id, reply)).unwrap(); assert!(combine(combine_source(&first), combine_source(&restored)).err().unwrap().contains("Second document is closed"));
        call(&service, |reply| Request::Close(first.id, reply)).unwrap(); assert!(combine(combine_source(&first), combine_source(&changed)).err().unwrap().contains("First document is closed"));
        assert!(!path.exists()); assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
    }
    #[test]
    fn crop_rotation_inherited_boxes_preview_text_geometry_print_and_reopened_outputs_agree() {
        let _print_lock = print_test_lock();
        use lopdf::Document;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        let full = CropRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
        for inherited in [false, true] {
            for original_rotation in [0, 90, 180, 270] {
                let bytes = crate::text_geometry::tests::fixture(original_rotation, [1.0, 0.0, 0.0, 1.0, 100.0, 300.0], "OutsideTop\n\nInsideHeader\nInsideBody\n\nOutsideBottom");
                let mut pdf = Document::load_mem(&bytes).unwrap();
                let page = pdf.get_pages()[&1];
                pdf.get_object_mut(page).unwrap().as_dict_mut().unwrap().set("MediaBox", vec![20.into(), 30.into(), 420.into(), 430.into()]);
                if inherited {
                    let parent = pdf.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
                    for key in [b"MediaBox".as_slice(), b"CropBox", b"Rotate"] {
                        let value = pdf.get_object_mut(page).unwrap().as_dict_mut().unwrap().remove(key).unwrap();
                        pdf.get_object_mut(parent).unwrap().as_dict_mut().unwrap().set(key, value);
                    }
                }
                let path = folder.path().join(format!("crop-{inherited}-{original_rotation}.pdf"));
                pdf.save(&path).unwrap(); let source = std::fs::read(&path).unwrap();
                let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
                let old_snapshot = print_snapshot(&service, info.id, info.revision);
                let original_print = service.print_render_blocking(old_snapshot.token, 0, 900, 900).unwrap();
                for edited in 0..4 {
                    let turns = (original_rotation / 90 + edited) % 4;
                    let before_revision = info.revision;
                    let no_op = call(&service, |reply| Request::Crop(info.id, 0, info.revision, full, reply)).unwrap();
                    assert_eq!(no_op.revision, info.revision); assert_eq!(no_op.dirty, info.dirty); assert_eq!(no_op.can_undo, info.can_undo); assert_eq!(no_op.can_redo, info.can_redo);
                    // These fixed pixel regions describe [80,150,320,260] inside the fixture's [50,70,350,330] visible box.
                    let (rect, expected_region, old_width, new_width) = match turns {
                        0 => (CropRect { x: 0.1, y: 70.0 / 260.0, width: 0.8, height: 110.0 / 260.0 }, (90, 210, 720, 330), 900, 720),
                        1 => (CropRect { x: 80.0 / 260.0, y: 0.1, width: 110.0 / 260.0, height: 0.8 }, (240, 90, 330, 720), 780, 330),
                        2 => (CropRect { x: 0.1, y: 80.0 / 260.0, width: 0.8, height: 110.0 / 260.0 }, (90, 240, 720, 330), 900, 720),
                        _ => (CropRect { x: 70.0 / 260.0, y: 0.1, width: 110.0 / 260.0, height: 0.8 }, (210, 90, 330, 720), 780, 330),
                    };
                    let before = image::load_from_memory(&call(&service, |reply| Request::Render(info.id, 0, old_width, reply)).unwrap()).unwrap().into_rgb8();
                    let expected = image::imageops::crop_imm(&before, expected_region.0, expected_region.1, expected_region.2, expected_region.3).to_image();
                    info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, rect, reply)).unwrap();
                    assert_eq!(info.revision, before_revision + 1); assert!(info.dirty && info.can_undo);
                    let (expected_width, expected_height) = if turns % 2 == 0 { (240.0, 110.0) } else { (110.0, 240.0) };
                    assert!((info.pages[0].width - expected_width).abs() < 0.001 && (info.pages[0].height - expected_height).abs() < 0.001);
                    assert!(call(&service, |reply| Request::Crop(info.id, 0, before_revision, rect, reply)).err().unwrap().contains("changed"));
                    assert!(call(&service, |reply| Request::Text(info.id, 0, before_revision, reply)).is_err());
                    assert!(call(&service, |reply| Request::TextGeometry(info.id, 0, before_revision, reply)).is_err());
                    let no_op = call(&service, |reply| Request::Crop(info.id, 0, info.revision, full, reply)).unwrap(); assert_eq!(no_op.revision, info.revision);
                    let preview = image::load_from_memory(&call(&service, |reply| Request::Render(info.id, 0, new_width, reply)).unwrap()).unwrap().into_rgb8();
                    assert_same_ink(&preview, &expected);
                    let geometry = call(&service, |reply| Request::TextGeometry(info.id, 0, info.revision, reply)).unwrap();
                    assert_eq!(geometry.status, "ok", "{:?}", geometry.reason);
                    let positioned = geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>();
                    let text = call(&service, |reply| Request::Text(info.id, 0, info.revision, reply)).unwrap();
                    for words in [&positioned, &text] {
                        for word in ["InsideHeader", "InsideBody"] { assert!(words.contains(word), "Lost {word}: original {original_rotation}, edit {edited}, inherited {inherited}: {words}"); }
                        for word in ["OutsideTop", "OutsideBottom"] { assert!(!words.contains(word), "Copied hidden {word}"); }
                    }
                    let boxes = geometry.characters.iter().filter_map(|character| character.bounds.as_ref()).collect::<Vec<_>>();
                    for (x, y, pixel) in preview.enumerate_pixels().filter(|(_, _, pixel)| pixel.0.iter().any(|value| *value < 100)) {
                        let _ = pixel;
                        assert!(boxes.iter().any(|bounds| x as f32 >= bounds.x * preview.width() as f32 - 2.0 && x as f32 <= (bounds.x + bounds.width) * preview.width() as f32 + 2.0 && y as f32 >= bounds.y * preview.height() as f32 - 2.0 && y as f32 <= (bounds.y + bounds.height) * preview.height() as f32 + 2.0), "Crop geometry missed rendered ink");
                    }
                    let snapshot = print_snapshot(&service, info.id, info.revision);
                    assert!(service.print_render_blocking(snapshot.token, 0, 0, 900).is_err());
                    let printed = service.print_render_blocking(snapshot.token, 0, new_width as u32, expected_region.3).unwrap();
                    let print_rgb = image::RgbImage::from_raw(printed.width, printed.height, printed.bgra.chunks_exact(4).flat_map(|pixel| [pixel[2], pixel[1], pixel[0]]).collect()).unwrap();
                    assert_same_ink(&print_rgb, &expected);
                    let prior_print = service.print_render_blocking(old_snapshot.token, 0, 900, 900).unwrap();
                    assert_eq!(prior_print.bgra, original_print.bgra, "Crop or failure changed an earlier print snapshot");
                    let copy = folder.path().join(format!("copy-{inherited}-{original_rotation}-{edited}.pdf"));
                    let saved = call(&service, |reply| Request::Save(info.id, None, copy.clone(), reply)).unwrap();
                    assert!(!saved.document.dirty); assert_eq!(saved.document.revision, info.revision);
                    let output = Document::load(&copy).unwrap();
                    assert_eq!(output.get_page_content(output.get_pages()[&1]), pdf.get_page_content(page));
                    assert!(output.extract_text(&[1]).unwrap().contains("OutsideBottom"), "Crop must hide content without deleting it");
                    let reopened = call(&service, |reply| Request::Open(copy.clone(), reply)).unwrap();
                    let reopened_image = image::load_from_memory(&call(&service, |reply| Request::Render(reopened.id, 0, new_width, reply)).unwrap()).unwrap().into_rgb8();
                    assert_same_ink(&reopened_image, &expected);
                    let reopened_text = call(&service, |reply| Request::Text(reopened.id, 0, 0, reply)).unwrap(); assert!(reopened_text.contains("InsideHeader") && !reopened_text.contains("OutsideBottom"));
                    call(&service, |reply| Request::Close(reopened.id, reply)).unwrap();
                    let split_folder = folder.path().join(format!("split-{inherited}-{original_rotation}-{edited}"));
                    let split = call(&service, |reply| Request::Split(info.id, info.revision, 1, split_folder, reply)).unwrap(); assert_eq!(split.files.len(), 1);
                    let split_info = call(&service, |reply| Request::Open(PathBuf::from(&split.files[0].path), reply)).unwrap();
                    let split_image = image::load_from_memory(&call(&service, |reply| Request::Render(split_info.id, 0, new_width, reply)).unwrap()).unwrap().into_rgb8(); assert_same_ink(&split_image, &expected);
                    call(&service, |reply| Request::Close(split_info.id, reply)).unwrap();
                    info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap();
                    let restored = image::load_from_memory(&call(&service, |reply| Request::Render(info.id, 0, old_width, reply)).unwrap()).unwrap().into_rgb8(); assert_eq!(restored, before);
                    info = call(&service, |reply| Request::Edit(info.id, PageEdit::Redo, reply)).unwrap();
                    let redone = image::load_from_memory(&call(&service, |reply| Request::Render(info.id, 0, new_width, reply)).unwrap()).unwrap().into_rgb8(); assert_same_ink(&redone, &expected);
                    info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap();
                    let held = service.print_render_blocking(snapshot.token, 0, new_width as u32, expected_region.3).unwrap(); assert_eq!(held.bgra, printed.bgra, "Crop print snapshot changed after undo");
                    call(&service, |reply| Request::EndPrint(snapshot.token, reply)).unwrap();
                    assert_eq!(std::fs::read(&path).unwrap(), source);
                    if edited < 3 { info = call(&service, |reply| Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap(); }
                }
                call(&service, |reply| Request::Close(info.id, reply)).unwrap();
                assert!(call(&service, |reply| Request::Crop(info.id, 0, info.revision, full, reply)).err().unwrap().contains("closed"));
                assert_eq!(service.print_render_blocking(old_snapshot.token, 0, 900, 900).unwrap().bgra, original_print.bgra);
                call(&service, |reply| Request::EndPrint(old_snapshot.token, reply)).unwrap();
                assert_eq!(std::fs::read(&path).unwrap(), source);
                let original = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
                assert_eq!(original.pages[0].width, if original_rotation % 180 == 0 { 300.0 } else { 260.0 });
                call(&service, |reply| Request::Close(original.id, reply)).unwrap();
            }
        }
    }
    #[test]
    fn crop_current_edited_page_all_fixtures_and_repeated_crop_preserve_other_pages_and_source() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        for (fixture, count) in [("resources/welcome.pdf", 6usize), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)] {
            let path = root.join(fixture); let source = std::fs::read(&path).unwrap();
            let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Move { from: 0, to: 2 }, reply)).unwrap();
            let original_text = call(&service, |reply| Request::Text(info.id, 2, info.revision, reply)).unwrap();
            assert!(original_text.contains(&format!("Page 1 of {count}")));
            let other_before = call(&service, |reply| Request::Render(info.id, 0, 201, reply)).unwrap();
            let original_crop_page = call(&service, |reply| Request::Render(info.id, 2, 200, reply)).unwrap();
            let started = std::time::Instant::now();
            info = call(&service, |reply| Request::Crop(info.id, 2, info.revision, CropRect { x: 0.0, y: 0.0, width: 1.0, height: 0.8 }, reply)).unwrap();
            assert_eq!(info.pages.len(), count); assert!(info.dirty && info.can_undo);
            assert!(call(&service, |reply| Request::Text(info.id, 0, info.revision, reply)).unwrap().contains(&format!("Page 2 of {count}")));
            assert!(!call(&service, |reply| Request::Text(info.id, 2, info.revision, reply)).unwrap().contains(&format!("Page 1 of {count}")), "Footer should be outside the crop in {fixture}");
            let geometry = call(&service, |reply| Request::TextGeometry(info.id, 2, info.revision, reply)).unwrap();
            assert_eq!(geometry.status, "ok", "{fixture}: {:?}", geometry.reason);
            assert!(!geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>().contains(&format!("Page 1 of {count}")));
            let first_size = info.pages[2].clone();
            info = call(&service, |reply| Request::Crop(info.id, 2, info.revision, CropRect { x: 0.1, y: 0.1, width: 0.8, height: 0.8 }, reply)).unwrap();
            assert!((info.pages[2].width - first_size.width * 0.8).abs() < 0.001 && (info.pages[2].height - first_size.height * 0.8).abs() < 0.001);
            let plan_revision = info.revision;
            for rect in [
                CropRect { x: f64::NAN, y: 0.0, width: 0.5, height: 0.5 },
                CropRect { x: 0.0, y: 0.0, width: f64::INFINITY, height: 0.5 },
                CropRect { x: -0.1, y: 0.0, width: 0.5, height: 0.5 },
                CropRect { x: 0.0, y: 0.0, width: 0.0, height: 0.5 },
                CropRect { x: 0.2, y: 0.0, width: 0.9, height: 0.5 },
                CropRect { x: 0.0, y: 0.0, width: 0.0001, height: 0.5 },
            ] { assert!(call(&service, |reply| Request::Crop(info.id, 2, info.revision, rect, reply)).is_err()); }
            assert!(call(&service, |reply| Request::Crop(info.id, count as u16, info.revision, CropRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }, reply)).is_err());
            assert_eq!(call(&service, |reply| Request::Properties(info.id, plan_revision, reply)).unwrap().page_count, count);
            assert_eq!(call(&service, |reply| Request::Render(info.id, 0, 201, reply)).unwrap(), other_before, "Crop changed an unrelated current page");
            let preview = image::load_from_memory(&call(&service, |reply| Request::Render(info.id, 2, 200, reply)).unwrap()).unwrap().into_rgb8();
            let copy = folder.path().join(format!("fixture-{count}.pdf"));
            let saved = call(&service, |reply| Request::Save(info.id, None, copy.clone(), reply)).unwrap(); assert!(!saved.document.dirty);
            let reopened = call(&service, |reply| Request::Open(copy, reply)).unwrap();
            assert_eq!(reopened.pages.len(), count);
            let after = image::load_from_memory(&call(&service, |reply| Request::Render(reopened.id, 2, 200, reply)).unwrap()).unwrap().into_rgb8();
            assert_eq!(after, preview);
            call(&service, |reply| Request::Close(reopened.id, reply)).unwrap();
            println!("crop fixture={fixture} pages={count} repeated-crop-export-reopen-ms={:.1}", started.elapsed().as_secs_f64() * 1000.0);
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap();
            assert!((info.pages[2].width - first_size.width).abs() < 0.001 && (info.pages[2].height - first_size.height).abs() < 0.001);
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap();
            assert_eq!(call(&service, |reply| Request::Render(info.id, 2, 200, reply)).unwrap(), original_crop_page);
            assert_eq!(std::fs::read(&path).unwrap(), source);
            call(&service, |reply| Request::Close(info.id, reply)).unwrap();
        }
    }
    #[test]
    fn crop_minimum_point_and_edge_rounding_and_protected_failures_restore_source_page() {
        let _print_lock = print_test_lock();
        use lopdf::{dictionary, Document};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        for kind in ["ordinary", "certified", "signature", "byte_range"] {
            let mut pdf = Document::load(root.join("resources/welcome.pdf")).unwrap();
            match kind {
                "certified" => { pdf.catalog_mut().unwrap().set("Perms", dictionary! {}); }
                "signature" => { pdf.add_object(dictionary! { "Type" => "Sig" }); }
                "byte_range" => { pdf.add_object(dictionary! { "ByteRange" => vec![0.into(), 1.into(), 2.into(), 3.into()] }); }
                _ => {}
            }
            let path = folder.path().join(format!("protected-{kind}.pdf")); pdf.save(&path).unwrap(); let source = std::fs::read(&path).unwrap();
            let info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
            let snapshot = print_snapshot(&service, info.id, 0);
            let before = service.print_render_blocking(snapshot.token, 0, 300, 400).unwrap();
            if kind == "ordinary" {
                let width = f64::from(info.pages[0].width);
                let changed = call(&service, |reply| Request::Crop(info.id, 0, 0, CropRect { x: (width - 1.0) / width, y: 0.0, width: 1.0 / width + 1e-15, height: 1.0 }, reply)).unwrap();
                assert_eq!(changed.pages[0].width, 1.0); assert_eq!(changed.revision, 1);
                call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap();
            } else {
                for rect in [CropRect { x: 0.1, y: 0.1, width: 0.8, height: 0.8 }, CropRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }] {
                    assert!(call(&service, |reply| Request::Crop(info.id, 0, 0, rect, reply)).err().unwrap().contains("Signed or certified"));
                }
                let properties = call(&service, |reply| Request::Properties(info.id, 0, reply)).unwrap(); assert_eq!(properties.page_count, 6);
            }
            assert_eq!(service.print_render_blocking(snapshot.token, 0, 300, 400).unwrap().bgra, before.bgra, "Crop or rejected operation changed source boundaries");
            call(&service, |reply| Request::Close(info.id, reply)).unwrap();
            assert_eq!(service.print_render_blocking(snapshot.token, 0, 300, 400).unwrap().bgra, before.bgra);
            call(&service, |reply| Request::EndPrint(snapshot.token, reply)).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), source);
        }
    }
    #[test]
    fn page_text_and_geometry_keep_visible_words_and_exclude_crop_hidden_words_at_every_rotation() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        let visible = ["VisibleHeader", "VisibleBody", "VisibleMiddle", "VisibleLower", "VisibleBottom"];
        let hidden = ["HiddenAbove", "HiddenBelow"];
        for original_rotation in [0, 90, 180, 270] {
            let path = folder.path().join(format!("cropped-text-{original_rotation}.pdf"));
            let bytes = crate::text_geometry::tests::fixture(original_rotation, [1.0, 0.0, 0.0, 1.0, 100.0, 372.0], "HiddenAbove\n\nVisibleHeader\nVisibleBody\nVisibleMiddle\nVisibleLower\nVisibleBottom\n\n\n\nHiddenBelow");
            std::fs::write(&path, &bytes).unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path.clone(), tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
            for edited_rotation in 0..4 {
                let revision = edited_rotation as u64;
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::TextGeometry(info.id, 0, revision, tx)).unwrap(); let geometry = rx.blocking_recv().unwrap().unwrap();
                assert_eq!(geometry.status, "ok", "{:?}", geometry.reason);
                let geometry_text = geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>();
                for word in visible { assert!(geometry_text.contains(word), "Geometry lost {word}: original {original_rotation}, edited {edited_rotation}"); }
                for word in hidden { assert!(!geometry_text.contains(word), "Geometry copied hidden {word}: original {original_rotation}, edited {edited_rotation}"); }
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Text(info.id, 0, revision, tx)).unwrap(); let text = rx.blocking_recv().unwrap().unwrap();
                for word in visible { assert!(text.contains(word), "Page text lost {word}: original {original_rotation}, edited {edited_rotation}"); }
                for word in hidden { assert!(!text.contains(word), "Page text copied hidden {word}: original {original_rotation}, edited {edited_rotation}"); }
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 260, tx)).unwrap(); let preview = image::load_from_memory(&rx.blocking_recv().unwrap().unwrap()).unwrap();
                let turns = (original_rotation / 90 + edited_rotation) % 4;
                assert_eq!(preview.width() < preview.height(), turns % 2 == 1, "Text inspection changed page rotation");
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
                if edited_rotation < 3 {
                    let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
                    let (tx, rx) = oneshot::channel(); service.sender.send(Request::Text(info.id, 0, revision, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
                }
            }
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Text(info.id, 0, 3, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
        }
    }
    #[test]
    fn split_validates_every_fixture_page_and_preserves_source_revision_and_undo() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let output = tempfile::tempdir().unwrap();
        for (fixture, count, group) in [("resources/welcome.pdf", 6usize, 2usize), ("../test-corpus/synthetic-scan-98.pdf", 98, 40), ("../test-corpus/synthetic-text-1500.pdf", 1500, 700)] {
            let source_path = root.join(fixture); let source = std::fs::read(&source_path).unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(source_path.clone(), tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
            for edit in [PageEdit::Move { from: 0, to: 2 }, PageEdit::Rotate { pages: vec![0], clockwise: true }, PageEdit::Delete { pages: vec![1] }] {
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, edit, tx)).unwrap(); let changed = rx.blocking_recv().unwrap().unwrap(); assert!(changed.dirty);
            }
            let folder = output.path().join(format!("fixture-{count}"));
            let split = |revision, pages_per_file, folder: PathBuf| {
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Split(info.id, revision, pages_per_file, folder, tx)).unwrap(); rx.blocking_recv().unwrap()
            };
            assert!(split(0, group, folder.clone()).unwrap_err().contains("changed")); assert!(!folder.exists());
            assert!(split(3, 0, folder.clone()).is_err()); assert!(!folder.exists());
            if count > 64 { assert!(split(3, 1, folder.clone()).unwrap_err().contains("64")); assert!(!folder.exists()); }
            let started = std::time::Instant::now();
            let result = split(3, group, folder.clone()).unwrap();
            println!("split fixture {count}: {} outputs, {} pages, {:?}", result.files.len(), count - 1, started.elapsed());
            assert_eq!(result.files.len(), 3); assert_eq!(result.folder, folder.canonicalize().unwrap());
            let mut emitted = 0;
            for file in result.files {
                assert_eq!(file.first_page, emitted + 1); assert_eq!(file.last_page, (emitted + group).min(count - 1)); assert_eq!(file.page_count, file.last_page - file.first_page + 1);
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(file.path.clone(), tx)).unwrap(); let part = rx.blocking_recv().unwrap().unwrap(); assert_eq!(part.pages.len(), file.page_count);
                for local in 0..part.pages.len() {
                    let position = emitted + local;
                    let source_page = match position { 0 => 1, 1 => 0, other => other + 1 };
                    let (tx, rx) = oneshot::channel(); service.sender.send(Request::Text(part.id, local as u16, 0, tx)).unwrap(); let text = rx.blocking_recv().unwrap().unwrap();
                    assert!(text.contains(&format!("Page {} of {count}", source_page + 1)), "Incorrect split order: {fixture}, position {position}");
                }
                if emitted == 0 {
                    assert_eq!(part.pages[0].width, info.pages[1].height); assert_eq!(part.pages[0].height, info.pages[1].width);
                    let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 260, tx)).unwrap(); let current = rx.blocking_recv().unwrap().unwrap();
                    let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(part.id, 0, 260, tx)).unwrap(); let copied = rx.blocking_recv().unwrap().unwrap();
                    assert_eq!(image::load_from_memory(&current).unwrap().into_rgba8(), image::load_from_memory(&copied).unwrap().into_rgba8(), "Split rotation/render differs from current edited page: {fixture}");
                }
                emitted += part.pages.len();
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(part.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            }
            assert_eq!(emitted, count - 1); assert_eq!(std::fs::read(&source_path).unwrap(), source);
            assert!(split(3, group, folder.clone()).unwrap_err().contains("already exists"));
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Text(info.id, 0, 3, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().unwrap().contains(&format!("Page 2 of {count}")));
            for undo in 0..3 {
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Undo, tx)).unwrap(); let changed = rx.blocking_recv().unwrap().unwrap();
                assert_eq!(changed.revision, 4 + undo); assert_eq!(changed.pages.len(), count); assert_eq!(changed.dirty, undo != 2); assert!(changed.can_redo); assert_eq!(changed.can_undo, undo != 2);
            }
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            let closed = output.path().join(format!("closed-{count}")); assert!(split(6, group, closed.clone()).unwrap_err().contains("closed")); assert!(!closed.exists());
        }
    }
    #[test]
    fn text_geometry_preserves_spaces_generated_newlines_and_unicode() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("whitespace-unicode.pdf");
        std::fs::write(&path, crate::text_geometry::tests::fixture(0, [1.0, 0.0, 0.0, 1.0, 100.0, 200.0], "First café\nSecond line")).unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::TextGeometry(info.id, 0, 0, tx)).unwrap(); let geometry = rx.blocking_recv().unwrap().unwrap();
        assert_eq!(geometry.status, "ok", "{:?}", geometry.reason);
        let copied = geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>();
        assert!(copied.contains("First café")); assert!(copied.contains("Second line")); assert!(copied.contains('\n'));
        assert!(geometry.characters.iter().any(|character| character.text == "é" && character.bounds.is_some()));
        assert!(geometry.characters.iter().filter(|character| character.text == "\r" || character.text == "\n").all(|character| character.bounds.is_none()));
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Text(info.id, 0, 0, tx)).unwrap(); let source = rx.blocking_recv().unwrap().unwrap();
        assert_eq!(copied, source);
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
    }
    #[test]
    fn text_geometry_cropped_rotated_glyphs_match_actual_rendered_ink() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        for rotation in [0, 90, 180, 270] {
            let path = folder.path().join(format!("ink-{rotation}.pdf"));
            std::fs::write(&path, crate::text_geometry::tests::fixture(rotation, [1.0, 0.0, 0.0, 1.0, 100.0, 200.0], "MMMM")).unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
            for edited in 0..4 {
                let total = (rotation / 90 + edited) % 4;
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 900, tx)).unwrap(); let before = rx.blocking_recv().unwrap().unwrap();
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::TextGeometry(info.id, 0, edited as u64, tx)).unwrap(); let geometry = rx.blocking_recv().unwrap().unwrap();
                assert_eq!(geometry.status, "ok", "{:?}", geometry.reason);
                assert_eq!(geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>(), "MMMM");
                assert!(geometry.characters.iter().all(|character| character.angle as i64 == total * 90));
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 901, tx)).unwrap(); let after = rx.blocking_recv().unwrap().unwrap();
                let image = image::load_from_memory(&before).unwrap().into_rgb8();
                let after = image::load_from_memory(&after).unwrap().into_rgb8();
                assert_eq!(image.width() < image.height(), after.width() < after.height());
                let ink: Vec<_> = image.enumerate_pixels().filter(|(_, _, pixel)| pixel.0.iter().any(|value| *value < 100)).map(|(x, y, _)| (x as f32, y as f32)).collect();
                assert!(!ink.is_empty());
                let boxes: Vec<_> = geometry.characters.iter().map(|character| {
                    let bounds = character.bounds.as_ref().unwrap();
                    (bounds.x * image.width() as f32, bounds.y * image.height() as f32, (bounds.x + bounds.width) * image.width() as f32, (bounds.y + bounds.height) * image.height() as f32)
                }).collect();
                let inside = |(x, y): (f32, f32), (left, top, right, bottom): (f32, f32, f32, f32)| x >= left - 2.0 && x <= right + 2.0 && y >= top - 2.0 && y <= bottom + 2.0;
                for bounds in &boxes { assert!(ink.iter().any(|point| inside(*point, *bounds)), "No rendered ink in mapped character: rotation {rotation}, edit {edited}"); }
                assert!(ink.iter().all(|point| boxes.iter().any(|bounds| inside(*point, *bounds))), "Rendered ink falls outside mapped characters: rotation {rotation}, edit {edited}");
                let after_boxes: Vec<_> = geometry.characters.iter().map(|character| {
                    let bounds = character.bounds.as_ref().unwrap();
                    (bounds.x * after.width() as f32, bounds.y * after.height() as f32, (bounds.x + bounds.width) * after.width() as f32, (bounds.y + bounds.height) * after.height() as f32)
                }).collect();
                assert!(after.enumerate_pixels().filter(|(_, _, pixel)| pixel.0.iter().any(|value| *value < 100)).all(|(x, y, _)| after_boxes.iter().any(|bounds| inside((x as f32, y as f32), *bounds))), "Geometry changed source rotation before a fresh render: rotation {rotation}, edit {edited}");
                if edited < 3 { let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap(); }
            }
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        }
    }
    #[test]
    fn text_geometry_unsupported_is_all_or_nothing_and_cap_is_explicit() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        for (index, matrix) in [[1.0, 0.0, 0.2, 1.0, 100.0, 200.0], [0.707, 0.707, -0.707, 0.707, 100.0, 200.0], [-1.0, 0.0, 0.0, 1.0, 200.0, 200.0]].into_iter().enumerate() {
            let path = folder.path().join(format!("unsupported-{index}.pdf"));
            std::fs::write(&path, crate::text_geometry::tests::fixture(90, matrix, "MMMM")).unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::TextGeometry(info.id, 0, 1, tx)).unwrap(); let geometry = rx.blocking_recv().unwrap().unwrap();
            assert_eq!(geometry.status, "unsupported"); assert!(geometry.characters.is_empty()); assert!(geometry.reason.is_some());
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 900, tx)).unwrap(); let image = image::load_from_memory(&rx.blocking_recv().unwrap().unwrap()).unwrap();
            assert!(image.width() > image.height(), "Unsupported geometry must preserve original /Rotate90 plus editRotate90");
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        }
        for count in [20_000, 20_001] {
            let path = folder.path().join(format!("cap-{count}.pdf"));
            std::fs::write(&path, crate::text_geometry::tests::fixture(0, [1.0, 0.0, 0.0, 1.0, 100.0, 200.0], &"M".repeat(count))).unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::TextGeometry(info.id, 0, 0, tx)).unwrap(); let geometry = rx.blocking_recv().unwrap().unwrap();
            assert_eq!(geometry.status, "ok", "{:?}", geometry.reason); assert_eq!(geometry.truncated, count > 20_000); assert!(geometry.characters.len() <= 20_000); assert!(serde_json::to_vec(&geometry).unwrap().len() < 4_000_000);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        }
    }
    #[test]
    fn geometry_follows_edits_and_rejects_stale_invalid_and_closed_requests() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        for (fixture, count) in [("resources/welcome.pdf", 6), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)] {
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(root.join(fixture), tx)).unwrap();
            let info = rx.blocking_recv().unwrap().unwrap();
            let read = |page, revision| {
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::TextGeometry(info.id, page, revision, tx)).unwrap(); rx.blocking_recv().unwrap()
            };
            for page in [0, count / 2, count - 1] {
                let geometry = read(page, 0).unwrap();
                assert_eq!(geometry.status, "ok", "{fixture}: {:?}", geometry.reason);
                assert!(geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>().contains(&format!("Page {} of {count}", page + 1)));
            }
            assert!(read(count, 0).is_err());
            for edit in [PageEdit::Move { from: 0, to: 1 }, PageEdit::Rotate { pages: vec![0], clockwise: true }, PageEdit::Delete { pages: vec![1] }] {
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, edit, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            }
            assert!(read(0, 0).is_err());
            let geometry = read(0, 3).unwrap();
            assert!(geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>().contains(&format!("Page 2 of {count}")));
            assert!(geometry.characters.iter().filter(|character| character.bounds.is_some()).all(|character| character.angle == 90));
            assert!(read(count - 1, 3).is_err());
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            assert!(read(0, 3).is_err());
        }
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("rotated.pdf");
        std::fs::write(&path, crate::text_geometry::tests::fixture(90, [1.0, 0.0, 0.0, 1.0, 100.0, 200.0], "MMMM")).unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::TextGeometry(info.id, 0, 1, tx)).unwrap(); let geometry = rx.blocking_recv().unwrap().unwrap();
        assert!(geometry.characters.iter().all(|character| character.angle == 180));
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
    }
    #[test]
    fn properties_follow_current_pages_without_modifying_source_metadata() {
        use lopdf::{dictionary, Object};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let folder = tempfile::tempdir().unwrap();
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let mut pdf = lopdf::Document::load(root.join("resources/welcome.pdf")).unwrap();
        let metadata = pdf.add_object(dictionary! { "Title" => Object::string_literal("<script>plain PDF title</script>"), "Author" => Object::string_literal("Fixture author") });
        pdf.trailer.set("Info", metadata);
        let path = folder.path().join("properties.pdf"); pdf.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path.clone(), tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Properties(info.id, 0, tx)).unwrap(); let original = rx.blocking_recv().unwrap().unwrap();
        assert_eq!(original.page_count, 6); assert_eq!(original.source_size_bytes, bytes.len());
        assert_eq!(original.security.encrypted, Some(false)); assert_eq!(original.signature_validation, "not_performed");
        assert_eq!(original.metadata.iter().find(|entry| entry.name == "title").unwrap().value, "<script>plain PDF title</script>");
        for edit in [PageEdit::Rotate { pages: vec![0], clockwise: true }, PageEdit::Delete { pages: vec![1] }] {
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, edit, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        }
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Properties(info.id, 0, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Properties(info.id, 2, tx)).unwrap(); let edited = rx.blocking_recv().unwrap().unwrap();
        assert_eq!(edited.page_count, 5); assert_eq!(edited.source_size_bytes, bytes.len());
        assert_eq!(edited.page_dimensions.iter().map(|size| size.count).sum::<usize>(), 5);
        assert_eq!(edited.page_dimensions[0].width_points, info.pages[0].height);
        assert_eq!(edited.page_dimensions[0].height_points, info.pages[0].width);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Properties(info.id, 2, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
    }
    #[test]
    fn print_dimensions_bound_pixels_and_preserve_aspect() {
        for (width, height) in [(612.0, 792.0), (792.0, 612.0), (1.0, 1.0), (1.0, 1_000_000.0), (1_000_000.0, 1.0)] {
            for (max_width, max_height) in [(300, 400), (1, 1), (u32::MAX, u32::MAX)] {
                let (output_width, output_height) = print_dimensions(width, height, max_width, max_height).unwrap();
                assert!(output_width > 0 && output_height > 0);
                assert!(output_width <= max_width.min(4096) && output_height <= max_height.min(4096));
                assert!(output_width as u64 * output_height as u64 <= 16_000_000);
            }
        }
        assert_eq!(print_dimensions(600.0, 800.0, 300, 300).unwrap(), (225, 300));
        assert_eq!(print_dimensions(800.0, 600.0, 300, 300).unwrap(), (300, 225));
        for (width, height, max_width, max_height) in [(0.0, 1.0, 10, 10), (f32::NAN, 1.0, 10, 10), (1.0, f32::INFINITY, 10, 10), (1.0, 1.0, 0, 10)] { assert!(print_dimensions(width, height, max_width, max_height).is_err()); }
    }
    #[test]
    fn print_bitmap_is_bgra_with_opaque_white_background() {
        let _print_lock = print_test_lock();
        use lopdf::{dictionary, Object, Stream};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let folder = tempfile::tempdir().unwrap();
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"1 0 0 rg 50 50 100 100 re f".to_vec()));
        let page = pdf.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()], "Contents" => content });
        pdf.objects.insert(pages, dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }.into());
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages }); pdf.trailer.set("Root", catalog);
        let path = folder.path().join("print-colors.pdf"); pdf.save(&path).unwrap();
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap(); let info = rx.blocking_recv().unwrap().unwrap();
        let snapshot = print_snapshot(&service, info.id, 0);
        let bitmap = service.print_render_blocking(snapshot.token, 0, 100, 100).unwrap();
        assert_eq!(&bitmap.bgra[..4], &[255, 255, 255, 255]);
        let center = (50 * bitmap.width as usize + 50) * 4; assert_eq!(&bitmap.bgra[center..center + 4], &[0, 0, 255, 255]);
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::EndPrint(snapshot.token, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
    }
    #[test]
    fn print_snapshot_survives_edits_and_close_but_not_release() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(root.join("resources/welcome.pdf"), tx)).unwrap();
        let info = rx.blocking_recv().unwrap().unwrap();
        let before = print_snapshot(&service, info.id, 0); assert_eq!(before.pages, 6);
        let original_first = service.print_render_blocking(before.token, 0, 300, 400).unwrap();
        let original_second = service.print_render_blocking(before.token, 1, 300, 400).unwrap();
        assert_eq!(original_first.bgra.len(), original_first.width as usize * original_first.height as usize * 4);
        assert!(original_first.bgra.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(original_first.bgra.chunks_exact(4).any(|pixel| pixel == [255, 255, 255, 255]));
        assert!(service.print_render_blocking(before.token, 6, 300, 400).is_err());
        assert!(service.print_render_blocking(before.token, 0, 0, 400).is_err());
        for edit in [PageEdit::Rotate { pages: vec![0], clockwise: true }, PageEdit::Move { from: 0, to: 2 }] {
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, edit, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        }
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::BeginPrint(info.id, 0, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
        let after = print_snapshot(&service, info.id, 2);
        let reordered = service.print_render_blocking(after.token, 0, 300, 400).unwrap(); assert_eq!(reordered.bgra, original_second.bgra);
        let rotated = service.print_render_blocking(after.token, 2, 300, 400).unwrap(); assert!(rotated.width > rotated.height);
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        let pinned = service.print_render_blocking(before.token, 0, 300, 400).unwrap(); assert_eq!(pinned.bgra, original_first.bgra);
        let pinned_rotated = service.print_render_blocking(after.token, 2, 300, 400).unwrap(); assert_eq!(pinned_rotated.bgra, rotated.bgra);
        for token in [before.token, after.token] {
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::EndPrint(token, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            assert!(service.print_render_blocking(token, 0, 300, 400).is_err());
        }
    }
    #[test]
    fn encrypted_open_retries_cancels_and_keeps_edits_blocked() {
        let _print_lock = print_test_lock();
        use lopdf::encryption::crypt_filters::{Aes128CryptFilter, Aes256CryptFilter, CryptFilter};
        use std::{collections::BTreeMap, sync::Arc};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let folder = tempfile::tempdir().unwrap();
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let ordinary = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
        for (version, user_password) in [2, 4, 5].into_iter().flat_map(|version| ["test password", ""].map(|password| (version, password))) {
            let mut pdf = lopdf::Document::load(root.join("resources/welcome.pdf")).unwrap();
            pdf.trailer.set("ID", vec![lopdf::Object::string_literal("password-test-id"), lopdf::Object::string_literal("password-test-id")]);
            let permissions = lopdf::Permissions::all();
            let fixture_key = [0x42u8; 32];
            let encryption = match version {
                2 => lopdf::EncryptionVersion::V2 { document: &pdf, owner_password: "owner password", user_password, key_length: 128, permissions },
                4 => {
                    let filter: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
                    lopdf::EncryptionVersion::V4 { document: &pdf, encrypt_metadata: true, crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]), stream_filter: b"StdCF".to_vec(), string_filter: b"StdCF".to_vec(), owner_password: "owner password", user_password, permissions }
                }
                5 => {
                    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
                    lopdf::EncryptionVersion::V5 { encrypt_metadata: true, crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]), file_encryption_key: &fixture_key, stream_filter: b"StdCF".to_vec(), string_filter: b"StdCF".to_vec(), owner_password: "owner password", user_password, permissions }
                }
                _ => unreachable!(),
            };
            let state = lopdf::EncryptionState::try_from(encryption).unwrap();
            pdf.encrypt(&state).unwrap();
            let kind = if user_password.is_empty() { "empty" } else { "locked" };
            let path = folder.path().join(format!("v{version}-{kind}.pdf"));
            pdf.save(&path).unwrap();
            let source = std::fs::read(&path).unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::BeginOpen(path.clone(), tx)).unwrap();
            let result = rx.blocking_recv().unwrap().unwrap();
            let info = if user_password.is_empty() {
                match result { OpenResult::Opened { document } => document, _ => panic!("Empty password should open") }
            } else {
                let request_id = match result { OpenResult::PasswordRequired { request_id, incorrect, .. } => { assert!(!incorrect); request_id }, _ => panic!("Expected password challenge") };
                for wrong in ["wrong", ""] {
                    let (tx, rx) = oneshot::channel(); service.sender.send(Request::Unlock(request_id, wrong.into(), tx)).unwrap();
                    assert!(matches!(rx.blocking_recv().unwrap().unwrap(), OpenResult::PasswordRequired { .. }));
                }
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Unlock(request_id, user_password.into(), tx)).unwrap();
                let opened = match rx.blocking_recv().unwrap().unwrap() { OpenResult::Opened { document } => document, _ => panic!("Correct password failed") };
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Unlock(request_id, user_password.into(), tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::BeginOpen(path.clone(), tx)).unwrap();
                let cancelled = match rx.blocking_recv().unwrap().unwrap() { OpenResult::PasswordRequired { request_id, .. } => request_id, _ => panic!("Expected challenge") };
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::CancelPassword(cancelled, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Unlock(cancelled, user_password.into(), tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
                opened
            };
            assert_eq!(info.pages.len(), 6);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Properties(info.id, 0, tx)).unwrap(); let properties = rx.blocking_recv().unwrap().unwrap();
            assert_ne!(properties.security.encrypted, Some(false), "Encrypted v{version}-{kind} must never be reported unencrypted");
            assert_eq!(properties.page_count, 6); assert_eq!(properties.source_size_bytes, source.len());
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::BeginPrint(info.id, 0, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err(), "Encrypted v{version}-{kind} must not print");
            for page in 0..info.pages.len() {
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, page as u16, 100, tx)).unwrap(); assert!(!rx.blocking_recv().unwrap().unwrap().is_empty(), "v{version}-{kind} page {page}");
            }
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Crop(info.id, 0, 0, CropRect { x: 0.1, y: 0.1, width: 0.8, height: 0.8 }, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err(), "Encrypted v{version}-{kind} must not crop");
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 101, tx)).unwrap(); assert!(!rx.blocking_recv().unwrap().unwrap().is_empty(), "Rejected crop must preserve a fresh encrypted preview");
            let output = folder.path().join("must-not-export.pdf");
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Save(info.id, None, output.clone(), tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err()); assert!(!output.exists());
            assert!(call(&service, |reply| Request::CheckCombine(combine_source(&ordinary), combine_source(&info), reply)).is_err(), "Encrypted v{version}-{kind} must not combine");
            assert!(call(&service, |reply| Request::Combine(combine_source(&info), combine_source(&ordinary), output.clone(), reply)).is_err()); assert!(!output.exists());
            assert_eq!(std::fs::read(&path).unwrap(), source);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        }
        call(&service, |reply| Request::Close(ordinary.id, reply)).unwrap();
    }
    #[test]
    fn malformed_middle_page_is_rejected_instead_of_silently_truncated() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let folder = tempfile::tempdir().unwrap();
        let mut pdf = lopdf::Document::load(root.join("resources/welcome.pdf")).unwrap();
        let middle = pdf.get_pages()[&2];
        pdf.objects.insert(middle, lopdf::Object::Null);
        let path = folder.path().join("broken-page.pdf"); pdf.save(&path).unwrap();
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::BeginOpen(path.clone(), tx)).unwrap();
        assert!(rx.blocking_recv().unwrap().err().unwrap().contains("page 2"));
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap();
        assert!(rx.blocking_recv().unwrap().err().unwrap().contains("page 2"));
    }
    #[test]
    fn bookmarks_include_nested_destinations_and_disable_external_actions() {
        use lopdf::{dictionary, Object};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut pdf = lopdf::Document::load(root.join("resources/welcome.pdf")).unwrap();
        let pages = pdf.get_pages();
        let outlines = pdf.new_object_id();
        let first = pdf.new_object_id();
        let child = pdf.new_object_id();
        let external = pdf.new_object_id();
        pdf.objects.insert(outlines, dictionary! { "Type" => "Outlines", "First" => first, "Last" => external, "Count" => 3 }.into());
        pdf.objects.insert(first, dictionary! { "Title" => Object::string_literal("Start"), "Parent" => outlines, "Next" => external, "First" => child, "Last" => child, "Count" => 1, "Dest" => vec![Object::Reference(pages[&1]), Object::Name(b"Fit".to_vec())] }.into());
        pdf.objects.insert(child, dictionary! { "Title" => Object::string_literal("Second page"), "Parent" => first, "A" => dictionary! { "S" => "GoTo", "D" => vec![Object::Reference(pages[&2]), Object::Name(b"Fit".to_vec())] } }.into());
        pdf.objects.insert(external, dictionary! { "Title" => Object::string_literal("External"), "Parent" => outlines, "Prev" => first, "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.com") } }.into());
        pdf.catalog_mut().unwrap().set("Outlines", outlines);
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("bookmarks.pdf"); pdf.save(&path).unwrap();
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap();
        let info = rx.blocking_recv().unwrap().unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Bookmarks(info.id, 0, tx)).unwrap();
        let result = rx.blocking_recv().unwrap().unwrap();
        assert!(!result.truncated); assert_eq!(result.items.len(), 3);
        assert_eq!(result.items[0].title, "Start"); assert_eq!(result.items[0].page, Some(0));
        assert_eq!(result.items[1].depth, 1); assert_eq!(result.items[1].page, Some(1));
        assert_eq!(result.items[2].page, None);
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Bookmarks(info.id, 99, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
    }
    #[test]
    fn bookmark_cycles_and_large_outlines_are_bounded() {
        use lopdf::{dictionary, Object};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let folder = tempfile::tempdir().unwrap();
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        for (count, cycle) in [(0, false), (3, true), (1005, false)] {
            let mut pdf = lopdf::Document::load(root.join("resources/welcome.pdf")).unwrap();
            if count > 0 {
                let page = pdf.get_pages()[&1];
                let outlines = pdf.new_object_id();
                let ids: Vec<_> = (0..count).map(|_| pdf.new_object_id()).collect();
                pdf.objects.insert(outlines, dictionary! { "Type" => "Outlines", "First" => ids[0], "Last" => ids[count - 1], "Count" => count as i64 }.into());
                for (index, id) in ids.iter().enumerate() {
                    let mut entry = dictionary! { "Title" => Object::string_literal(format!("Bookmark {index}")), "Parent" => outlines, "Dest" => vec![Object::Reference(page), Object::Name(b"Fit".to_vec())] };
                    if index > 0 { entry.set("Prev", ids[index - 1]); }
                    if index + 1 < count { entry.set("Next", ids[index + 1]); }
                    else if cycle { entry.set("Next", ids[0]); }
                    pdf.objects.insert(*id, entry.into());
                }
                pdf.catalog_mut().unwrap().set("Outlines", outlines);
            }
            let path = folder.path().join(format!("outline-{count}.pdf")); pdf.save(&path).unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap();
            let info = rx.blocking_recv().unwrap().unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Bookmarks(info.id, 0, tx)).unwrap();
            let result = rx.blocking_recv().unwrap().unwrap();
            assert_eq!(result.items.len(), count.min(1000));
            assert_eq!(result.truncated, count > 1000);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 64, tx)).unwrap();
            assert!(rx.blocking_recv().unwrap().is_ok());
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Bookmarks(info.id, 0, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
        }
    }
    #[test]
    fn text_follows_page_edits_and_rejects_stale_or_closed_requests() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        for (fixture, count) in [("resources/welcome.pdf", 6), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)] {
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(root.join(fixture), tx)).unwrap();
            let info = rx.blocking_recv().unwrap().unwrap();
            let read = |page, revision| {
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Text(info.id, page, revision, tx)).unwrap();
                rx.blocking_recv().unwrap()
            };
            for page in 0..count {
                assert!(read(page, 0).unwrap().contains(&format!("Page {} of {count}", page + 1)));
            }
            assert!(read(count, 0).is_err());
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Move { from: 0, to: 1 }, tx)).unwrap();
            let changed = rx.blocking_recv().unwrap().unwrap();
            assert!(read(0, 0).is_err());
            assert!(read(0, changed.revision).unwrap().contains(&format!("Page 2 of {count}")));
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
            assert!(read(0, changed.revision).is_err());
        }
    }
    #[test]
    fn cache_evicts_and_clears_closed_documents() {
        let mut cache = Cache { entries: VecDeque::new(), weight: 0 };
        cache.insert((1, 0, 800), vec![1], 300 * 1024 * 1024);
        cache.insert((2, 0, 800), vec![2], 300 * 1024 * 1024);
        assert!(cache.get((1, 0, 800)).is_none()); assert_eq!(cache.get((2, 0, 800)), Some(vec![2]));
        cache.close(2); assert_eq!(cache.weight, 0);
    }
    #[test]
    fn renders_scan_corpus_and_rejects_invalid_page() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let (tx, rx) = oneshot::channel();
        service.sender.send(Request::Open(root.join("../test-corpus/synthetic-scan-98.pdf"), tx)).unwrap();
        let info = rx.blocking_recv().unwrap().unwrap(); assert_eq!(info.pages.len(), 98);
        for page in 0..98 {
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, page, 816, tx)).unwrap();
            let bytes = rx.blocking_recv().unwrap().unwrap(); assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 98, 816, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 816, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
        for (path, count) in [("resources/welcome.pdf", 6), ("../test-corpus/synthetic-text-1500.pdf", 1500)] {
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(root.join(path), tx)).unwrap();
            let info = rx.blocking_recv().unwrap().unwrap(); assert_eq!(info.pages.len(), count);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, (count - 1) as u16, 816, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_ok());
        }
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(root.join("../test-corpus/invalid.pdf"), tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
    }

    #[test]
    fn edited_preview_matches_reopened_copy_for_every_valid_fixture() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let output = tempfile::tempdir().unwrap();
        for (fixture, count) in [("resources/welcome.pdf", 6), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)] {
            let source_path = root.join(fixture);
            let source_bytes = std::fs::read(&source_path).unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(source_path.clone(), tx)).unwrap();
            let info = rx.blocking_recv().unwrap().unwrap();
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, tx)).unwrap();
            let changed = rx.blocking_recv().unwrap().unwrap();
            assert!(changed.dirty); assert!(changed.can_undo); assert_eq!(changed.pages[0].width, 792.0);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, 0, 260, tx)).unwrap();
            let preview = image::load_from_memory(&rx.blocking_recv().unwrap().unwrap()).unwrap().into_rgba8();
            let path = output.path().join(format!("organized-{count}.pdf"));
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Save(info.id, None, path.clone(), tx)).unwrap();
            assert!(!rx.blocking_recv().unwrap().unwrap().document.dirty);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Open(path, tx)).unwrap();
            let reopened = rx.blocking_recv().unwrap().unwrap(); assert_eq!(reopened.pages.len(), count);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(reopened.id, 0, 260, tx)).unwrap();
            let saved = image::load_from_memory(&rx.blocking_recv().unwrap().unwrap()).unwrap().into_rgba8();
            assert_eq!(preview.dimensions(), saved.dimensions());
            assert!(preview.as_raw() == saved.as_raw(), "Preview differs from saved copy for {fixture}");
            assert_eq!(std::fs::read(source_path).unwrap(), source_bytes);
        }
    }
}
