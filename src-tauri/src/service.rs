use std::{collections::{HashMap, VecDeque}, io::Cursor, path::PathBuf, sync::{mpsc, OnceLock}};
use pdfium_render::prelude::*;
use serde::Serialize;
use tokio::sync::oneshot;
use crate::editor::{EditSession, PageEdit, write_new_file};

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
type Reply<T> = oneshot::Sender<Result<T, String>>;
enum Request {
    Open(PathBuf, Reply<DocumentInfo>),
    BeginOpen(PathBuf, Reply<OpenResult>),
    Unlock(u64, String, Reply<OpenResult>),
    CancelPassword(u64, Reply<()>),
    Render(u64, u16, i32, Reply<Vec<u8>>),
    Text(u64, u16, u64, Reply<String>),
    Bookmarks(u64, u64, Reply<BookmarkList>),
    Close(u64, Reply<()>),
    Edit(u64, PageEdit, Reply<DocumentInfo>),
    Save(u64, Option<Vec<usize>>, PathBuf, Reply<SavedCopy>),
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
        std::thread::Builder::new().name("pdf-worker".into()).spawn(move || {
            let pdfium = Pdfium::bind_to_library(library).map(Pdfium::new).map_err(|e| format!("PDF engine could not start: {e}"));
            let mut documents = HashMap::new();
            let mut sessions = HashMap::<u64, (EditSession, DocumentInfo)>::new();
            let mut cache = Cache { entries: VecDeque::new(), weight: 0 };
            let mut next_id = 1;
            let mut pending = HashMap::<u64, PathBuf>::new();
            let mut next_request = 1;
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
                            documents.insert(id, document);
                            Ok(OpenResult::Opened { document: info })
                        })();
                        if !matches!(&result, Ok(OpenResult::PasswordRequired { .. })) { pending.remove(&request_id); }
                        if let Err(result) = reply.send(result) {
                            pending.remove(&request_id);
                            if let Ok(OpenResult::Opened { document }) = result { documents.remove(&document.id); sessions.remove(&document.id); }
                        }
                    }
                    Request::Open(path, reply) => {
                        let result = (|| {
                            let engine = pdfium.as_ref().map_err(Clone::clone)?;
                            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                            let document = engine.load_pdf_from_byte_vec(bytes.clone(), None).map_err(|e| format!("Unable to open PDF: {e}"))?;
                            let pages = page_sizes(&document)?;
                            let id = next_id; next_id += 1;
                            let info = DocumentInfo { id, name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path: path.to_string_lossy().into_owned(), pages, revision: 0, dirty: false, can_undo: false, can_redo: false };
                            sessions.insert(id, (EditSession::new(bytes, info.pages.len()), info.clone()));
                            documents.insert(id, document); Ok(info)
                        })();
                        let _ = reply.send(result);
                    },
                    Request::Render(id, page, width, reply) => {
                        if reply.is_closed() { continue; }
                        let width = width.clamp(64, 3000);
                        let key = (id, page, width);
                        let result = if let Some(bytes) = cache.get(key) { Ok(bytes) } else {
                            (|| {
                                let document = documents.get(&id).ok_or("Document is closed")?;
                                let spec = sessions.get(&id).ok_or("Document is closed")?.0.plan.get(page as usize).ok_or("Page is out of range")?;
                                let mut page = document.pages().get(spec.source as i32).map_err(|e| e.to_string())?;
                                let original_rotation = page.rotation().map_err(|e| e.to_string())?;
                                let original_turns = match original_rotation { PdfPageRenderRotation::Degrees90 => 1, PdfPageRenderRotation::Degrees180 => 2, PdfPageRenderRotation::Degrees270 => 3, _ => 0 };
                                let rotation = match (original_turns + spec.turns) % 4 { 1 => PdfPageRenderRotation::Degrees90, 2 => PdfPageRenderRotation::Degrees180, 3 => PdfPageRenderRotation::Degrees270, _ => PdfPageRenderRotation::None };
                                page.set_rotation(rotation);
                                let image_result = (|| {
                                    let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(width).set_maximum_height(5000)).map_err(|e| e.to_string())?;
                                    bitmap.as_image().map_err(|e| e.to_string())
                                })();
                                page.set_rotation(original_rotation);
                                let image = image_result?;
                                let mut bytes = Cursor::new(Vec::new());
                                image.write_to(&mut bytes, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                                let bytes = bytes.into_inner();
                                let weight = bytes.len() + image.width() as usize * image.height() as usize * 4;
                                cache.insert(key, bytes.clone(), weight); Ok(bytes)
                            })()
                        };
                        let _ = reply.send(result);
                    },
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
                    Request::Text(id, page, revision, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (session, _) = sessions.get(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Search again.".into()); }
                            let spec = session.plan.get(page as usize).ok_or("Page is out of range")?;
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            let source = document.pages().get(spec.source as i32).map_err(|e| e.to_string())?;
                            let text = source.text().map_err(|e| e.to_string())?;
                            Ok(text.all())
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Edit(id, edit, reply) => {
                        let result = (|| {
                            let (session, original) = sessions.get_mut(&id).ok_or("Document is closed")?;
                            session.apply(edit)?;
                            cache.close(id);
                            Ok(current_info(session, original))
                        })();
                        let _ = reply.send(result);
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
                            Ok(SavedCopy { path: path.to_string_lossy().into_owned(), document: current_info(session, original) })
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
    pub async fn bookmarks(&self, id: u64, revision: u64) -> Result<BookmarkList, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Bookmarks(id, revision, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn edit(&self, id: u64, edit: PageEdit) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Edit(id, edit, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
    pub async fn save(&self, id: u64, pages: Option<Vec<usize>>, path: PathBuf) -> Result<SavedCopy, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Save(id, pages, path, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?
    }
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

fn current_info(session: &EditSession, original: &DocumentInfo) -> DocumentInfo {
    let mut info = original.clone();
    info.pages = session.plan.iter().map(|page| {
        let size = &original.pages[page.source];
        if page.turns % 2 == 0 { size.clone() } else { PageSize { width: size.height, height: size.width } }
    }).collect();
    info.revision = session.revision; info.dirty = session.dirty(); info.can_undo = session.can_undo(); info.can_redo = session.can_redo(); info
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_open_retries_cancels_and_keeps_edits_blocked() {
        use lopdf::encryption::crypt_filters::{Aes128CryptFilter, Aes256CryptFilter, CryptFilter};
        use std::{collections::BTreeMap, sync::Arc};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let folder = tempfile::tempdir().unwrap();
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
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
            for page in 0..info.pages.len() {
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Render(info.id, page as u16, 100, tx)).unwrap(); assert!(!rx.blocking_recv().unwrap().unwrap().is_empty(), "v{version}-{kind} page {page}");
            }
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
            let output = folder.path().join("must-not-export.pdf");
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Save(info.id, None, output.clone(), tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err()); assert!(!output.exists());
            assert_eq!(std::fs::read(&path).unwrap(), source);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Close(info.id, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
        }
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
