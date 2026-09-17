use std::{collections::{HashMap, VecDeque}, io::Cursor, path::PathBuf, sync::{mpsc, OnceLock}};
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use crate::editor::{CropBox, EditSession, PageEdit, PageSpec, write_new_file};
use crate::combine::CopyOperation;

#[derive(Clone, Copy, Deserialize)]
pub struct CropRect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
#[derive(Clone, Copy, Deserialize)]
pub struct CombineSource { pub id: u64, pub revision: u64 }
enum CommentMutation { Create(u16, CropRect, String), Update(String, String), Delete(String), CreateHighlight(u16, CropRect, Option<String>), UpdateHighlight(String, Option<String>), DeleteHighlight(String) }

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
#[cfg(test)]
#[derive(Default, Debug)]
struct RenderWork { dequeued: usize, skipped_closed: usize, rendered: usize }
enum UnclaimedReply { Document(u64), Password(u64) }
struct ReplyLease<T> { value: Option<T>, cleanup: Option<UnclaimedReply>, sender: mpsc::Sender<Request> }
impl<T> ReplyLease<T> {
    fn new(value: T, cleanup: UnclaimedReply, sender: mpsc::Sender<Request>) -> Self { Self { value: Some(value), cleanup: Some(cleanup), sender } }
    fn accept(mut self) -> T { self.cleanup = None; self.value.take().expect("A reply lease owns its value until acceptance") }
}
impl<T> std::ops::Deref for ReplyLease<T> {
    type Target = T;
    fn deref(&self) -> &T { self.value.as_ref().expect("An unaccepted reply lease owns its value") }
}
impl<T> Drop for ReplyLease<T> {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            let (reply, _) = oneshot::channel();
            let request = match cleanup { UnclaimedReply::Document(id) => Request::Close(id, reply), UnclaimedReply::Password(id) => Request::CancelPassword(id, reply) };
            let _ = self.sender.send(request);
        }
    }
}
enum Request {
    #[cfg(test)]
    BacklogGate(mpsc::Receiver<Vec<Request>>, Reply<()>),
    #[cfg(test)]
    RenderWorkForDocument(u64, Reply<RenderWork>),
    #[cfg(test)]
    OpenDocumentsForPath(PathBuf, Reply<Vec<u64>>),
    #[cfg(test)]
    PasswordRequestsForPath(PathBuf, Reply<Vec<u64>>),
    Open(PathBuf, Reply<ReplyLease<DocumentInfo>>),
    BeginOpen(PathBuf, Reply<ReplyLease<OpenResult>>),
    Unlock(u64, String, Reply<ReplyLease<OpenResult>>),
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
    Comments(u64, u64, Reply<crate::comments::CommentList>),
    Annotations(u64, u64, Reply<crate::comments::AnnotationList>),
    Comment(u64, u64, CommentMutation, Reply<DocumentInfo>),
    Save(u64, Option<Vec<usize>>, PathBuf, Reply<SavedCopy>),
    Split(u64, u64, usize, PathBuf, Reply<crate::split::SplitOutput>),
    CheckCombine(CombineSource, CombineSource, Reply<()>),
    Combine(CombineSource, CombineSource, PathBuf, Reply<ReplyLease<SavedCopy>>),
    CheckInsertion(CombineSource, CombineSource, usize, Reply<()>),
    InsertPages(CombineSource, CombineSource, usize, PathBuf, Reply<ReplyLease<SavedCopy>>),
    CheckReplacement(CombineSource, CombineSource, usize, usize, Reply<()>),
    ReplacePages(CombineSource, CombineSource, usize, usize, PathBuf, Reply<ReplyLease<SavedCopy>>),
    CreateCopy(CombineSource, CombineSource, CopyOperation, PathBuf, Reply<ReplyLease<SavedCopy>>),
}
#[derive(Default)]
struct RequestQueue { deferred: VecDeque<Request>, cleanup_overtakes: usize }
impl RequestQueue {
    fn next(&mut self, receiver: &mpsc::Receiver<Request>) -> Result<Request, mpsc::RecvError> {
        // Only viewer renders may be overtaken. Every other command preserves its FIFO boundary.
        // Two cleanup overtakes allow Close + EndPrint together while guaranteeing render progress.
        const WINDOW: usize = 32;
        const MAX_CLEANUP_OVERTAKES: usize = 2;
        if self.deferred.is_empty() { self.deferred.push_back(receiver.recv()?); }
        if matches!(self.deferred.front(), Some(Request::Render(..))) && self.cleanup_overtakes < MAX_CLEANUP_OVERTAKES {
            let mut index = 1;
            while index < WINDOW {
                if index == self.deferred.len() {
                    match receiver.try_recv() {
                        Ok(request) => self.deferred.push_back(request),
                        Err(_) => break,
                    }
                }
                match &self.deferred[index] {
                    Request::Render(..) => index += 1,
                    Request::Close(..) | Request::EndPrint(..) => {
                        self.cleanup_overtakes += 1;
                        return Ok(self.deferred.remove(index).expect("The cleanup index is buffered"));
                    }
                    _ => break,
                }
            }
        }
        self.cleanup_overtakes = 0;
        Ok(self.deferred.pop_front().expect("A request was received before dispatch"))
    }
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
        let resource_cleanup = sender.clone();
        std::thread::Builder::new().name("pdf-worker".into()).spawn(move || {
            let pdfium = Pdfium::bind_to_library(library).map(Pdfium::new).map_err(|e| format!("PDF engine could not start: {e}"));
            let mut documents = HashMap::new();
            let mut sessions = HashMap::<u64, (EditSession, DocumentInfo)>::new();
            let mut note_documents = HashMap::new();
            let mut cache = Cache { entries: VecDeque::new(), weight: 0 };
            let mut next_id = 1;
            let mut pending = HashMap::<u64, PathBuf>::new();
            let mut next_request = 1;
            let mut print_snapshots = HashMap::<u64, (std::rc::Rc<PdfDocument<'_>>, Vec<crate::editor::PageSpec>)>::new();
            let mut next_print = 1;
            #[cfg(test)]
            let mut render_work = HashMap::<u64, RenderWork>::new();
            let mut requests = RequestQueue::default();
            while let Ok(request) = requests.next(&receiver) {
                let request = match request {
                    Request::Combine(first, second, path, reply) => Request::CreateCopy(first, second, CopyOperation::Combine, path, reply),
                    Request::InsertPages(target, donor, at, path, reply) => Request::CreateCopy(target, donor, CopyOperation::Insert { at }, path, reply),
                    Request::ReplacePages(target, donor, start, count, path, reply) => Request::CreateCopy(target, donor, CopyOperation::Replace { start, count }, path, reply),
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
                    Request::BacklogGate(release, reply) => {
                        let _ = reply.send(Ok(()));
                        if let Ok(batch) = release.recv_timeout(std::time::Duration::from_secs(5)) {
                            for request in batch.into_iter().rev() { requests.deferred.push_front(request); }
                        }
                    }
                    #[cfg(test)]
                    Request::RenderWorkForDocument(id, reply) => { let _ = reply.send(Ok(render_work.remove(&id).unwrap_or_default())); }
                    #[cfg(test)]
                    Request::OpenDocumentsForPath(path, reply) => {
                        let ids = sessions.iter().filter_map(|(id, (_, info))| (PathBuf::from(&info.path) == path).then_some(*id)).collect();
                        let _ = reply.send(Ok(ids));
                    }
                    #[cfg(test)]
                    Request::PasswordRequestsForPath(path, reply) => {
                        let ids = pending.iter().filter_map(|(id, source)| (*source == path).then_some(*id)).collect();
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
                            let document = note_document(pdfium.as_ref().map_err(Clone::clone)?, document, session, &mut note_documents, id)?;
                            print_snapshots.insert(token, (document, session.plan.clone()));
                            Ok(PrintSnapshotInfo { token, pages: session.plan.len(), name: info.name.clone(), cleanup: resource_cleanup.clone() })
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
                        let result = result.map(|value| {
                            let cleanup = match &value { OpenResult::Opened { document } => UnclaimedReply::Document(document.id), OpenResult::PasswordRequired { request_id, .. } => UnclaimedReply::Password(*request_id) };
                            ReplyLease::new(value, cleanup, resource_cleanup.clone())
                        });
                        if let Err(result) = reply.send(result) {
                            pending.remove(&request_id);
                            if let Ok(value) = &result { if let OpenResult::Opened { document } = &**value { documents.remove(&document.id); sessions.remove(&document.id); } }
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
                        let result = result.map(|info| { let cleanup = UnclaimedReply::Document(info.id); ReplyLease::new(info, cleanup, resource_cleanup.clone()) });
                        if let Err(Ok(info)) = reply.send(result) { documents.remove(&info.id); sessions.remove(&info.id); }
                    },
                    Request::Render(id, page, width, reply) => {
                        #[cfg(test)]
                        { render_work.entry(id).or_default().dequeued += 1; }
                        if reply.is_closed() {
                            #[cfg(test)]
                            { render_work.entry(id).or_default().skipped_closed += 1; }
                            continue;
                        }
                        let width = width.clamp(64, 3000);
                        let key = (id, page, width);
                        let result = if let Some(bytes) = cache.get(key) { Ok(bytes) } else {
                            (|| {
                                let original = documents.get(&id).ok_or("Document is closed")?;
                                let session = &sessions.get(&id).ok_or("Document is closed")?.0;
                                let spec = session.plan.get(page as usize).ok_or("Page is out of range")?;
                                let document = note_document(pdfium.as_ref().map_err(Clone::clone)?, original, session, &mut note_documents, id)?;
                                let image = with_planned_page(&document, spec, |page| {
                                    #[cfg(test)]
                                    { render_work.entry(id).or_default().rendered += 1; }
                                    let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(width).set_maximum_height(5000).render_annotations(true)).map_err(|e| e.to_string())?;
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
                            note_documents.remove(&id);
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
                            note_documents.remove(&id);
                            cache.close(id);
                            current_info(session, original, document)
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Comments(id, revision, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let session = &sessions.get(&id).ok_or("Document is closed")?.0;
                            if session.revision != revision { return Err("Document changed. Refresh comments.".into()); }
                            let mut result = crate::comments::CommentList { document_id: id, revision, status: "supported", reason: None, notes: Vec::new() };
                            if let Some(reason) = session.comments_reason() { result.status = "unsupported"; result.reason = Some(reason.to_owned()); return Ok(result); }
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            for (index, spec) in session.plan.iter().enumerate() {
                                for note in spec.notes.iter().filter(|note| note.kind == crate::comments::AnnotationKind::Note) {
                                    let rect = with_planned_page(document, spec, |page| displayed_note_rect(page, note.rect))?;
                                    result.notes.push(crate::comments::CommentInfo { id: note.id.clone(), page: index, rect, contents: note.contents.clone() });
                                }
                            }
                            Ok(result)
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Annotations(id, revision, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let session = &sessions.get(&id).ok_or("Document is closed")?.0;
                            if session.revision != revision { return Err("Document changed. Refresh annotations.".into()); }
                            let mut result = crate::comments::AnnotationList { document_id: id, revision, status: "supported", reason: None, annotations: Vec::new() };
                            if let Some(reason) = session.comments_reason() { result.status = "unsupported"; result.reason = Some(reason.to_owned()); return Ok(result); }
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            for (index, spec) in session.plan.iter().enumerate() {
                                for note in &spec.notes {
                                    let rect = with_planned_page(document, spec, |page| displayed_note_rect(page, note.rect))?;
                                    result.annotations.push(crate::comments::AnnotationInfo { id: note.id.clone(), kind: note.kind, page: index, rect, contents: (!note.contents.is_empty()).then(|| note.contents.clone()) });
                                }
                            }
                            Ok(result)
                        })();
                        let _ = reply.send(result);
                    }
                    Request::Comment(id, revision, mutation, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (session, original) = sessions.get_mut(&id).ok_or("Document is closed")?;
                            if session.revision != revision { return Err("Document changed. Refresh comments.".into()); }
                            if let Some(reason) = session.comments_reason() { return Err(reason.to_owned()); }
                            let document = documents.get(&id).ok_or("Document is closed")?;
                            let next = match mutation {
                                CommentMutation::Create(page, rect, contents) => {
                                    let spec = session.plan.get(page as usize).ok_or("Page is out of range")?;
                                    let rect = with_planned_page(document, spec, |page| displayed_crop(page, rect))?;
                                    session.proposed_comment(Some((page as usize, rect)), None, Some(&contents))?
                                }
                                CommentMutation::Update(note_id, contents) => session.proposed_comment(None, Some(&note_id), Some(&contents))?,
                                CommentMutation::Delete(note_id) => session.proposed_comment(None, Some(&note_id), None)?,
                                CommentMutation::CreateHighlight(page, rect, contents) => {
                                    let spec = session.plan.get(page as usize).ok_or("Page is out of range")?;
                                    let rect = with_planned_page(document, spec, |page| displayed_crop(page, rect))?;
                                    session.proposed_highlight(Some((page as usize, rect)), None, contents.as_deref(), false)?
                                }
                                CommentMutation::UpdateHighlight(id, contents) => session.proposed_highlight(None, Some(&id), contents.as_deref(), false)?,
                                CommentMutation::DeleteHighlight(id) => session.proposed_highlight(None, Some(&id), None, true)?,
                            };
                            if next == session.plan { return current_info(session, original, document); }
                            let rendered = load_note_document(pdfium.as_ref().map_err(Clone::clone)?, document, session, &next)?;
                            session.commit_comments(next);
                            note_documents.insert(id, (session.revision, rendered)); cache.close(id);
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
                    Request::CheckInsertion(target, donor, at, reply) => {
                        if reply.is_closed() { continue; }
                        let result = insertion_sessions(&sessions, target, donor).and_then(|(target, donor)| crate::combine::validate_insertion(target, donor, at).map(|_| ()));
                        let _ = reply.send(result);
                    }
                    Request::CheckReplacement(target, donor, start, count, reply) => {
                        if reply.is_closed() { continue; }
                        let result = replacement_sessions(&sessions, target, donor).and_then(|(target, donor)| crate::combine::validate_replacement(target, donor, start, count).map(|_| ()));
                        let _ = reply.send(result);
                    }
                    Request::Combine(..) | Request::InsertPages(..) | Request::ReplacePages(..) => unreachable!(),
                    Request::CreateCopy(first, second, copy, path, reply) => {
                        if reply.is_closed() { continue; }
                        let result = (|| {
                            let (first, second) = match copy {
                                CopyOperation::Combine => combine_sessions(&sessions, first, second)?,
                                CopyOperation::Insert { .. } => insertion_sessions(&sessions, first, second)?,
                                CopyOperation::Replace { .. } => replacement_sessions(&sessions, first, second)?,
                            };
                            let operation = copy.name();
                            let engine = pdfium.as_ref().map_err(Clone::clone)?;
                            let mut prepared = None;
                            let validate = |bytes: &[u8], expected| {
                                if reply.is_closed() { return Err(format!("{operation} was canceled.")); }
                                let document = engine.load_pdf_from_byte_vec(bytes.to_vec(), None).map_err(|error| format!("Output could not be opened: {error}"))?;
                                let pages = page_sizes(&document)?;
                                if pages.len() != expected { return Err(format!("Output page count differs from the {operation} plan.")); }
                                for index in 0..document.pages().len() {
                                    let page = document.pages().get(index).map_err(|error| format!("Output page {} could not be read: {error}", index + 1))?;
                                    let bitmap = page.render_with_config(&PdfRenderConfig::new().set_target_width(64).set_maximum_height(64)).map_err(|error| format!("Output page {} could not be rendered: {error}", index + 1))?;
                                    if bitmap.width() <= 0 || bitmap.height() <= 0 { return Err(format!("Output page {} has an invalid bitmap.", index + 1)); }
                                }
                                if reply.is_closed() { return Err(format!("{operation} was canceled.")); }
                                prepared = Some((document, pages));
                                Ok(())
                            };
                            let bytes = match copy {
                                CopyOperation::Insert { at } => crate::combine::prepare_insertion_and_write(first, second, at, &path, validate)?,
                                CopyOperation::Replace { start, count } => crate::combine::prepare_replacement_and_write(first, second, start, count, &path, validate)?,
                                CopyOperation::Combine => crate::combine::prepare_and_write(first, second, &path, validate)?,
                            };
                            let (document, pages) = prepared.ok_or("New copy output was not validated")?;
                            let id = next_id; next_id += 1;
                            let info = DocumentInfo { id, name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path: path.to_string_lossy().into_owned(), pages, revision: 0, dirty: false, can_undo: false, can_redo: false };
                            sessions.insert(id, (EditSession::new(bytes, info.pages.len()), info.clone()));
                            documents.insert(id, std::rc::Rc::new(document));
                            Ok(SavedCopy { path: path.to_string_lossy().into_owned(), document: info })
                        })();
                        let result = result.map(|saved| { let cleanup = UnclaimedReply::Document(saved.document.id); ReplyLease::new(saved, cleanup, resource_cleanup.clone()) });
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
                    Request::Close(id, reply) => { documents.remove(&id); sessions.remove(&id); note_documents.remove(&id); cache.close(id); let _ = reply.send(Ok(())); }
                }
            }
        }).expect("Could not start PDF worker");
        Self { sender }
    }
    pub async fn open(&self, path: PathBuf) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Open(path, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?.map(ReplyLease::accept)
    }
    pub async fn begin_open(&self, path: PathBuf) -> Result<OpenResult, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::BeginOpen(path, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?.map(ReplyLease::accept)
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
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Unlock(id, password, tx)).map_err(|e| e.to_string())?; rx.await.map_err(|e| e.to_string())?.map(ReplyLease::accept)
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
    pub async fn comments(&self, id: u64, revision: u64) -> Result<crate::comments::CommentList, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Comments(id, revision, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn annotations(&self, id: u64, revision: u64) -> Result<crate::comments::AnnotationList, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Annotations(id, revision, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn create_highlight(&self, id: u64, revision: u64, page: u16, rect: CropRect, contents: Option<String>) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Comment(id, revision, CommentMutation::CreateHighlight(page, rect, contents), tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn update_highlight(&self, id: u64, revision: u64, annotation_id: String, contents: Option<String>) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Comment(id, revision, CommentMutation::UpdateHighlight(annotation_id, contents), tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn delete_highlight(&self, id: u64, revision: u64, annotation_id: String) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Comment(id, revision, CommentMutation::DeleteHighlight(annotation_id), tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn create_comment(&self, id: u64, revision: u64, page: u16, rect: CropRect, contents: String) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Comment(id, revision, CommentMutation::Create(page, rect, contents), tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn update_comment(&self, id: u64, revision: u64, note_id: String, contents: String) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Comment(id, revision, CommentMutation::Update(note_id, contents), tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn delete_comment(&self, id: u64, revision: u64, note_id: String) -> Result<DocumentInfo, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Comment(id, revision, CommentMutation::Delete(note_id), tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
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
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::Combine(first, second, path, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?.map(ReplyLease::accept)
    }
    pub async fn check_insertion(&self, target: CombineSource, donor: CombineSource, at: usize) -> Result<(), String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::CheckInsertion(target, donor, at, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn insert_pages_copy(&self, target: CombineSource, donor: CombineSource, at: usize, path: PathBuf) -> Result<SavedCopy, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::InsertPages(target, donor, at, path, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?.map(ReplyLease::accept)
    }
    pub async fn check_replacement(&self, target: CombineSource, donor: CombineSource, start: usize, count: usize) -> Result<(), String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::CheckReplacement(target, donor, start, count, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?
    }
    pub async fn replace_pages_copy(&self, target: CombineSource, donor: CombineSource, start: usize, count: usize, path: PathBuf) -> Result<SavedCopy, String> {
        let (tx, rx) = oneshot::channel(); self.sender.send(Request::ReplacePages(target, donor, start, count, path, tx)).map_err(|error| error.to_string())?; rx.await.map_err(|error| error.to_string())?.map(ReplyLease::accept)
    }
}

fn load_note_document<'a>(engine: &'a Pdfium, original: &std::rc::Rc<PdfDocument<'a>>, session: &EditSession, plan: &[PageSpec]) -> Result<std::rc::Rc<PdfDocument<'a>>, String> {
    match session.note_source(plan)? {
        None => Ok(original.clone()),
        Some(bytes) => {
            let document = engine.load_pdf_from_byte_vec(bytes, None).map_err(|error| error.to_string())?;
            if page_sizes(&document)?.len() != original.pages().len() as usize { return Err("Comment rendering changed the source page count. The edit was rejected.".into()); }
            Ok(std::rc::Rc::new(document))
        }
    }
}
fn note_document<'a>(engine: &'a Pdfium, original: &std::rc::Rc<PdfDocument<'a>>, session: &EditSession, documents: &mut HashMap<u64, (u64, std::rc::Rc<PdfDocument<'a>>)>, id: u64) -> Result<std::rc::Rc<PdfDocument<'a>>, String> {
    if let Some((revision, document)) = documents.get(&id) { if *revision == session.revision { return Ok(document.clone()); } }
    let document = load_note_document(engine, original, session, &session.plan)?;
    documents.insert(id, (session.revision, document.clone())); Ok(document)
}
fn displayed_note_rect(page: &PdfPage<'_>, rect: CropBox) -> Result<Option<crate::comments::DisplayRect>, String> {
    const SIZE: i32 = 1_000_000;
    let config = PdfRenderConfig::new().set_fixed_size(SIZE, SIZE); let mut points = Vec::with_capacity(4);
    for (x, y) in [(rect.left, rect.bottom), (rect.left, rect.top), (rect.right, rect.bottom), (rect.right, rect.top)] {
        points.push(page.points_to_pixels(PdfPoints::new(x), PdfPoints::new(y), &config).map_err(|error| error.to_string())?);
    }
    let x = (points.iter().map(|point| point.0).min().unwrap() as f64 / SIZE as f64).max(0.0);
    let y = (points.iter().map(|point| point.1).min().unwrap() as f64 / SIZE as f64).max(0.0);
    let right = (points.iter().map(|point| point.0).max().unwrap() as f64 / SIZE as f64).min(1.0);
    let bottom = (points.iter().map(|point| point.1).max().unwrap() as f64 / SIZE as f64).min(1.0);
    Ok((right > x && bottom > y).then_some(crate::comments::DisplayRect { x, y, width: right - x, height: bottom - y }))
}

fn combine_sessions(sessions: &HashMap<u64, (EditSession, DocumentInfo)>, first: CombineSource, second: CombineSource) -> Result<(&EditSession, &EditSession), String> {
    if first.id == second.id { return Err("Choose two different open PDFs to combine.".into()); }
    let first_session = &sessions.get(&first.id).ok_or("First document is closed")?.0;
    let second_session = &sessions.get(&second.id).ok_or("Second document is closed")?.0;
    if first_session.revision != first.revision || second_session.revision != second.revision { return Err("A document changed. Open Combine again.".into()); }
    Ok((first_session, second_session))
}

fn insertion_sessions(sessions: &HashMap<u64, (EditSession, DocumentInfo)>, target: CombineSource, donor: CombineSource) -> Result<(&EditSession, &EditSession), String> {
    if target.id == donor.id { return Err("Choose a different open PDF to insert into the target copy.".into()); }
    let target_session = &sessions.get(&target.id).ok_or("Target document is closed")?.0;
    let donor_session = &sessions.get(&donor.id).ok_or("Donor document is closed")?.0;
    if target_session.revision != target.revision || donor_session.revision != donor.revision { return Err("A document changed. Open Insert Pages again.".into()); }
    Ok((target_session, donor_session))
}

fn replacement_sessions(sessions: &HashMap<u64, (EditSession, DocumentInfo)>, target: CombineSource, donor: CombineSource) -> Result<(&EditSession, &EditSession), String> {
    if target.id == donor.id { return Err("Choose a different open PDF to replace pages in the target copy.".into()); }
    let target_session = &sessions.get(&target.id).ok_or("Target document is closed")?.0;
    let donor_session = &sessions.get(&donor.id).ok_or("Donor document is closed")?.0;
    if target_session.revision != target.revision || donor_session.revision != donor.revision { return Err("A document changed. Open Replace Pages again.".into()); }
    Ok((target_session, donor_session))
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
    static PASSWORD_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn print_test_lock() -> std::sync::MutexGuard<'static, ()> {
        PRINT_TESTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn password_test_lock() -> std::sync::MutexGuard<'static, ()> {
        PASSWORD_TESTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    trait TestReplyValue { type Accepted; fn accept(self) -> Self::Accepted; }
    impl<T> TestReplyValue for ReplyLease<T> { type Accepted = T; fn accept(self) -> T { ReplyLease::accept(self) } }
    macro_rules! accepted_reply_values {
        ($($value:ty),* $(,)?) => { $(impl TestReplyValue for $value { type Accepted = Self; fn accept(self) -> Self { self } })* };
    }
    accepted_reply_values!((), Vec<u8>, Vec<u64>, String, DocumentInfo, SavedCopy, OpenResult, PrintSnapshotInfo, PrintBitmap, BookmarkList, RenderWork, crate::document_properties::DocumentProperties, crate::text_geometry::PageTextGeometry, crate::split::SplitOutput, crate::comments::CommentList, crate::comments::AnnotationList);
    fn call<T: TestReplyValue>(service: &PdfService, request: impl FnOnce(Reply<T>) -> Request) -> Result<T::Accepted, String> {
        let (tx, rx) = oneshot::channel(); service.sender.send(request(tx)).unwrap(); rx.blocking_recv().unwrap().map(TestReplyValue::accept)
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
    fn comments_png(service: &PdfService, id: u64, page: u16, width: i32) -> image::RgbImage {
        image::load_from_memory(&call(service, |reply| Request::Render(id, page, width, reply)).unwrap()).unwrap().into_rgb8()
    }
    fn comments_yellow_bounds(image: &image::RgbImage) -> Option<(u32, u32, u32, u32)> {
        let mut pixels = image.enumerate_pixels().filter(|(_, _, pixel)| pixel[0] > 220 && pixel[1] > 150 && pixel[1] < 240 && pixel[2] < 60);
        let (x, y, _) = pixels.next()?; let mut bounds = (x, y, x, y);
        for (x, y, _) in pixels { bounds.0 = bounds.0.min(x); bounds.1 = bounds.1.min(y); bounds.2 = bounds.2.max(x); bounds.3 = bounds.3.max(y); }
        Some(bounds)
    }
    fn comments_assert_box(image: &image::RgbImage, rect: CropRect) {
        let bounds = comments_yellow_bounds(image).expect("The actual PDF bitmap must contain the note marker");
        for (actual, expected) in [(bounds.0 as f64, rect.x * image.width() as f64), (bounds.1 as f64, rect.y * image.height() as f64), ((bounds.2 + 1) as f64, (rect.x + rect.width) * image.width() as f64), ((bounds.3 + 1) as f64, (rect.y + rect.height) * image.height() as f64)] {
            assert!((actual - expected).abs() <= 2.0, "Actual yellow pixel boundary {actual}, independent expected boundary {expected}");
        }
    }
    fn comments_print_rgb(bitmap: &PrintBitmap) -> image::RgbImage {
        image::RgbImage::from_fn(bitmap.width, bitmap.height, |x, y| { let index = (y as usize * bitmap.width as usize + x as usize) * 4; image::Rgb([bitmap.bgra[index + 2], bitmap.bgra[index + 1], bitmap.bgra[index]]) })
    }
    fn highlights_assert_pixels(before: &image::RgbImage, after: &image::RgbImage, rect: CropRect, ink_threshold: u8) {
        assert_eq!(before.dimensions(), after.dimensions()); let mut tinted = 0; let mut dark = 0;
        for (x, y, pixel) in after.enumerate_pixels() {
            let old = before.get_pixel(x, y);
            let inside = x as f64 > rect.x * after.width() as f64 + 2.0 && (x as f64) < (rect.x + rect.width) * after.width() as f64 - 2.0 && y as f64 > rect.y * after.height() as f64 + 2.0 && (y as f64) < (rect.y + rect.height) * after.height() as f64 - 2.0;
            let outside = (x as f64) < rect.x * after.width() as f64 - 2.0 || (x as f64) > (rect.x + rect.width) * after.width() as f64 + 2.0 || (y as f64) < rect.y * after.height() as f64 - 2.0 || (y as f64) > (rect.y + rect.height) * after.height() as f64 + 2.0;
            if outside { assert_eq!(pixel, old, "An area highlight must not change unrelated pixels"); }
            if inside {
                assert!((pixel[0] as i16 - old[0] as i16).abs() <= 1 && (pixel[1] as i16 - old[1] as i16).abs() <= 1);
                assert!((pixel[2] as f64 - old[2] as f64 * 0.75).abs() <= 2.0, "Expected one yellow multiply fill at 25% opacity: old {old:?}, actual {pixel:?}");
                if old[0] > 240 && old[1] > 240 && old[2] > 240 { tinted += 1; }
                if old.0.iter().all(|value| *value < ink_threshold) { assert!(pixel.0.iter().all(|value| *value <= ink_threshold), "Highlight must keep fixture ink visible"); dark += 1; }
            }
        }
        assert!(tinted > 100, "The actual bitmap must show the area highlight"); assert!(dark > 0, "Fixture must independently place ink below {ink_threshold} inside the highlight");
    }
    fn highlights_assert_extent(image: &image::RgbImage, rect: CropRect) {
        let mut pixels = image.enumerate_pixels().filter(|(_, _, pixel)| pixel[0] > 240 && pixel[1] > 240 && (180..205).contains(&pixel[2])); let (x, y, _) = pixels.next().expect("Expected actual translucent highlight pixels"); let mut bounds = (x, y, x, y);
        for (x, y, _) in pixels { bounds.0 = bounds.0.min(x); bounds.1 = bounds.1.min(y); bounds.2 = bounds.2.max(x); bounds.3 = bounds.3.max(y); }
        for (actual, expected) in [(bounds.0 as f64, rect.x * image.width() as f64), (bounds.1 as f64, rect.y * image.height() as f64), ((bounds.2 + 1) as f64, (rect.x + rect.width) * image.width() as f64), ((bounds.3 + 1) as f64, (rect.y + rect.height) * image.height() as f64)] { assert!((actual - expected).abs() <= 2.0, "Actual highlight pixel extent {actual}, independently expected {expected}"); }
    }
    #[test]
    fn highlights_all_rotations_inherited_crop_multiply_pixels_unicode_reopen_and_print_isolation() {
        let _print_lock = print_test_lock(); let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let marker = CropRect { x: 0.05, y: 0.05, width: 0.9, height: 0.9 };
        for original in [0, 90, 180, 270] { for edited in 0..4 {
            let bytes = crate::text_geometry::tests::fixture(original, [1.0, 0.0, 0.0, 1.0, 100.0, 250.0], "VisibleHeader\nVisibleBody"); let mut pdf = lopdf::Document::load_mem(&bytes).unwrap(); let page = pdf.get_pages()[&1];
            pdf.get_dictionary_mut(page).unwrap().set("MediaBox", vec![20.into(), 30.into(), 420.into(), 430.into()]); let parent = pdf.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
            for key in [b"MediaBox".as_slice(), b"CropBox", b"Rotate", b"Resources"] { let value = pdf.get_dictionary_mut(page).unwrap().remove(key).unwrap(); pdf.get_dictionary_mut(parent).unwrap().set(key, value); }
            let path = folder.path().join(format!("highlight-{original}-{edited}.pdf")); pdf.save(&path).unwrap(); let source = std::fs::read(&path).unwrap(); let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
            for _ in 0..edited { info = call(&service, |reply| Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap(); }
            let before = comments_png(&service, info.id, 0, 400); let text = call(&service, |reply| Request::Text(info.id, 0, info.revision, reply)).unwrap(); let geometry = serde_json::to_value(call(&service, |reply| Request::TextGeometry(info.id, 0, info.revision, reply)).unwrap().characters).unwrap();
            let old = print_snapshot(&service, info.id, info.revision); let old_pixels = service.print_render_blocking(old.token, 0, 400, 400).unwrap(); let dimensions = (info.pages[0].width, info.pages[0].height); let body = "  Area-only é 漢字 😀\n ";
            info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::CreateHighlight(0, marker, Some(body.into())), reply)).unwrap(); assert_eq!((info.pages[0].width, info.pages[0].height), dimensions);
            assert_eq!(call(&service, |reply| Request::Text(info.id, 0, info.revision, reply)).unwrap(), text); assert_eq!(serde_json::to_value(call(&service, |reply| Request::TextGeometry(info.id, 0, info.revision, reply)).unwrap().characters).unwrap(), geometry); assert!(call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes.is_empty());
            let annotations = call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap(); assert_eq!(annotations.annotations.len(), 1); let highlight = &annotations.annotations[0]; assert_eq!(highlight.kind, crate::comments::AnnotationKind::Highlight); assert_eq!(highlight.contents.as_deref(), Some(body)); let id = highlight.id.clone(); let displayed = highlight.rect.as_ref().unwrap();
            for (actual, expected) in [(displayed.x, marker.x), (displayed.y, marker.y), (displayed.width, marker.width), (displayed.height, marker.height)] { assert!((actual - expected).abs() < 0.00001); }
            let after = comments_png(&service, info.id, 0, 400); highlights_assert_pixels(&before, &after, marker, 40); highlights_assert_extent(&after, marker); assert_eq!(comments_png(&service, info.id, 0, 400), after); assert_eq!(service.print_render_blocking(old.token, 0, 400, 400).unwrap().bgra, old_pixels.bgra);
            let snapshot = print_snapshot(&service, info.id, info.revision); let snapshot_pixels = service.print_render_blocking(snapshot.token, 0, 400, 400).unwrap(); highlights_assert_pixels(&comments_print_rgb(&old_pixels), &comments_print_rgb(&snapshot_pixels), marker, 40);
            let copy = folder.path().join(format!("highlight-copy-{original}-{edited}.pdf")); info = call(&service, |reply| Request::Save(info.id, None, copy.clone(), reply)).unwrap().document; let reopened = call(&service, |reply| Request::Open(copy, reply)).unwrap(); assert_eq!(comments_png(&service, reopened.id, 0, 400), after); let imported = call(&service, |reply| Request::Annotations(reopened.id, 0, reply)).unwrap(); assert_eq!(imported.annotations[0].id, id); assert_eq!(imported.annotations[0].contents.as_deref(), Some(body)); assert_eq!(call(&service, |reply| Request::Text(reopened.id, 0, 0, reply)).unwrap(), text); call(&service, |reply| Request::Close(reopened.id, reply)).unwrap();
            info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, CropRect { x: 0.2, y: 0.0, width: 0.8, height: 1.0 }, reply)).unwrap(); let clipped = call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap(); let clipped = clipped.annotations[0].rect.as_ref().unwrap(); assert!(clipped.x.abs() < 0.00001 && (clipped.width - 0.9375).abs() < 0.00001 && (clipped.y - 0.05).abs() < 0.00001);
            highlights_assert_extent(&comments_png(&service, info.id, 0, 320), CropRect { x: 0.0, y: 0.05, width: 0.9375, height: 0.9 });
            info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, CropRect { x: 0.99, y: 0.0, width: 0.01, height: 1.0 }, reply)).unwrap(); let hidden = call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap(); assert!(hidden.annotations[0].rect.is_none()); assert_eq!(hidden.annotations[0].contents.as_deref(), Some(body));
            assert!(!comments_png(&service, info.id, 0, 2).pixels().any(|pixel| pixel[0] > 240 && pixel[1] > 240 && (180..205).contains(&pixel[2])), "Off-crop markup must not remain visible");
            let hidden_path = folder.path().join(format!("highlight-hidden-{original}-{edited}.pdf")); call(&service, |reply| Request::Save(info.id, None, hidden_path.clone(), reply)).unwrap(); let hidden_open = call(&service, |reply| Request::Open(hidden_path, reply)).unwrap(); assert!(call(&service, |reply| Request::Annotations(hidden_open.id, 0, reply)).unwrap().annotations[0].rect.is_none()); call(&service, |reply| Request::Close(hidden_open.id, reply)).unwrap();
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap(); info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap(); assert_eq!(comments_png(&service, info.id, 0, 400), after); info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::DeleteHighlight(id), reply)).unwrap(); assert_eq!(comments_png(&service, info.id, 0, 400), before);
            call(&service, |reply| Request::Close(info.id, reply)).unwrap(); assert_eq!(service.print_render_blocking(snapshot.token, 0, 400, 400).unwrap().bgra, snapshot_pixels.bgra); assert_eq!(std::fs::read(path).unwrap(), source);
        } }
    }
    #[test]
    fn highlights_mixed_corpus_reopen_move_delete_extract_split_and_source_preservation() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap(); let area = CropRect { x: 0.15, y: 0.15, width: 0.7, height: 0.7 };
        for (fixture, count) in [("resources/welcome.pdf", 6usize), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)] {
            let path = root.join(fixture); let source = std::fs::read(&path).unwrap(); let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); let before = comments_png(&service, info.id, 0, 400);
            info = call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::CreateHighlight(0, area, None), reply)).unwrap(); highlights_assert_pixels(&before, &comments_png(&service, info.id, 0, 400), area, 160);
            let first = call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap(); assert!(first.annotations[0].contents.is_none()); let highlight = first.annotations[0].id.clone();
            info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Create((count - 1) as u16, CropRect { x: 0.03, y: 0.03, width: 0.06, height: 0.06 }, "Retained legacy-compatible note é 😀".into()), reply)).unwrap();
            let notes = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap(); assert_eq!(notes.notes.len(), 1); let note = notes.notes[0].id.clone(); assert_ne!(note, highlight);
            info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::UpdateHighlight(highlight.clone(), Some("  Corpus highlight 漢字 😀\n ".into())), reply)).unwrap();
            info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, CropRect { x: 0.0, y: 0.0, width: 0.9, height: 0.9 }, reply)).unwrap(); info = call(&service, |reply| Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap(); info = call(&service, |reply| Request::Edit(info.id, PageEdit::Move { from: 0, to: count - 1 }, reply)).unwrap(); info = call(&service, |reply| Request::Edit(info.id, PageEdit::Delete { pages: vec![1] }, reply)).unwrap();
            let expected = call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap(); assert_eq!(expected.annotations.len(), 2); assert_eq!(expected.annotations.iter().find(|value| value.id == highlight).unwrap().page, count - 2); assert_eq!(expected.annotations.iter().find(|value| value.id == note).unwrap().page, count - 3);
            let copy = folder.path().join(format!("mixed-corpus-{count}.pdf")); info = call(&service, |reply| Request::Save(info.id, None, copy.clone(), reply)).unwrap().document; assert!(!info.dirty); let reopened = call(&service, |reply| Request::Open(copy, reply)).unwrap(); assert_eq!(reopened.pages.len(), count - 1); assert_eq!(serde_json::to_value(call(&service, |reply| Request::Annotations(reopened.id, 0, reply)).unwrap().annotations).unwrap(), serde_json::to_value(&expected.annotations).unwrap());
            for page in [count - 3, count - 2] { assert_eq!(comments_png(&service, info.id, page as u16, 400), comments_png(&service, reopened.id, page as u16, 400)); }
            let extracted = folder.path().join(format!("highlight-extracted-{count}.pdf")); call(&service, |reply| Request::Save(info.id, Some(vec![0, count - 2]), extracted.clone(), reply)).unwrap(); let extracted = call(&service, |reply| Request::Open(extracted, reply)).unwrap(); let annotations = call(&service, |reply| Request::Annotations(extracted.id, 0, reply)).unwrap(); assert_eq!(annotations.annotations.len(), 1); assert_eq!(annotations.annotations[0].kind, crate::comments::AnnotationKind::Highlight); assert_eq!(annotations.annotations[0].id, highlight); assert_eq!(annotations.annotations[0].page, 1); assert_eq!(comments_png(&service, extracted.id, 1, 400), comments_png(&service, info.id, (count - 2) as u16, 400));
            let split = call(&service, |reply| Request::Split(info.id, info.revision, count.div_ceil(3), folder.path().join(format!("mixed-split-{count}")), reply)).unwrap(); let mut ids = Vec::new();
            for file in split.files { let part = call(&service, |reply| Request::Open(file.path.clone(), reply)).unwrap(); for annotation in call(&service, |reply| Request::Annotations(part.id, 0, reply)).unwrap().annotations { ids.push(annotation.id); } call(&service, |reply| Request::Close(part.id, reply)).unwrap(); }
            assert_eq!(ids.len(), 2); assert!(ids.contains(&highlight) && ids.contains(&note)); assert!(!call(&service, |reply| Request::Edit(info.id, PageEdit::Move { from: 0, to: 0 }, reply)).unwrap().dirty, "Split/extract must not change the saved baseline");
            let donor = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap(); assert!(call(&service, |reply| Request::CheckCombine(combine_source(&info), combine_source(&donor), reply)).unwrap_err().contains("Annots")); assert!(call(&service, |reply| Request::CheckInsertion(combine_source(&info), combine_source(&donor), 0, reply)).unwrap_err().contains("Annots")); assert!(call(&service, |reply| Request::CheckReplacement(combine_source(&info), combine_source(&donor), 0, 1, reply)).unwrap_err().contains("Annots"));
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Delete { pages: vec![count - 2] }, reply)).unwrap(); assert!(!call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap().annotations.iter().any(|value| value.id == highlight)); info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap(); assert!(call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap().annotations.iter().any(|value| value.id == highlight));
            for id in [info.id, reopened.id, extracted.id, donor.id] { call(&service, |reply| Request::Close(id, reply)).unwrap(); } assert_eq!(std::fs::read(path).unwrap(), source);
        }
    }
    #[test]
    fn highlights_invalid_stale_closed_preclosed_noops_and_branching_are_guarded() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let path = root.join("resources/welcome.pdf"); let source = std::fs::read(&path).unwrap(); let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); let before = comments_png(&service, info.id, 0, 300); let rect = CropRect { x: 0.1, y: 0.1, width: 0.2, height: 0.2 };
        for (page, bounds, body) in [(6, rect, None), (0, CropRect { x: f64::NAN, ..rect }, None), (0, CropRect { width: 0.00001, ..rect }, None), (0, CropRect { x: 0.9, ..rect }, None), (0, rect, Some("\0".into())), (0, rect, Some("x".repeat(8193)))] {
            assert!(call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::CreateHighlight(page, bounds, body), reply)).is_err()); assert!(call(&service, |reply| Request::Annotations(info.id, 0, reply)).unwrap().annotations.is_empty()); assert_eq!(comments_png(&service, info.id, 0, 300), before);
        }
        let (reply, canceled) = oneshot::channel(); drop(canceled); service.sender.send(Request::Comment(info.id, 0, CommentMutation::CreateHighlight(0, rect, None), reply)).unwrap(); assert!(call(&service, |reply| Request::Annotations(info.id, 0, reply)).unwrap().annotations.is_empty()); assert!(!call(&service, |reply| Request::Edit(info.id, PageEdit::Move { from: 0, to: 0 }, reply)).unwrap().can_undo);
        info = call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::CreateHighlight(0, rect, Some(" \n\t".into())), reply)).unwrap(); let list = call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap(); let first = list.annotations[0].id.clone(); assert!(list.annotations[0].contents.is_none());
        assert!(call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::UpdateHighlight(first.clone(), Some("Stale".into())), reply)).is_err()); assert!(call(&service, |reply| Request::Annotations(info.id, 0, reply)).is_err()); assert!(call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Delete(first.clone()), reply)).is_err());
        let unchanged = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::UpdateHighlight(first.clone(), None), reply)).unwrap(); assert_eq!(unchanged.revision, info.revision); info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap(); info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Create(0, rect, "Branch note".into()), reply)).unwrap(); let note = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes[0].id.clone(); assert!(crate::comments::number(&note).unwrap() > crate::comments::number(&first).unwrap()); assert!(!info.can_redo); assert!(call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::DeleteHighlight(note), reply)).is_err());
        info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::CreateHighlight(0, rect, None), reply)).unwrap(); let highlight = call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap().annotations.into_iter().find(|value| value.kind == crate::comments::AnnotationKind::Highlight).unwrap().id; info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::DeleteHighlight(highlight), reply)).unwrap(); assert_eq!(call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).unwrap().annotations.len(), 1);
        call(&service, |reply| Request::Close(info.id, reply)).unwrap(); assert!(call(&service, |reply| Request::Annotations(info.id, info.revision, reply)).is_err()); assert!(call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::CreateHighlight(0, rect, None), reply)).is_err()); assert_eq!(std::fs::read(path).unwrap(), source);
    }
    #[test]
    fn comments_rotations_inherited_crop_pixels_unicode_reopen_and_print_snapshots_agree() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let marker = CropRect { x: 0.1, y: 0.1, width: 0.2, height: 0.2 };
        for original in [0, 90, 180, 270] { for edited in 0..4 {
            let bytes = crate::text_geometry::tests::fixture(original, [1.0, 0.0, 0.0, 1.0, 100.0, 250.0], "VisibleHeader\nVisibleBody");
            let mut pdf = lopdf::Document::load_mem(&bytes).unwrap(); let page = pdf.get_pages()[&1];
            pdf.get_dictionary_mut(page).unwrap().set("MediaBox", vec![20.into(), 30.into(), 420.into(), 430.into()]);
            let parent = pdf.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
            for key in [b"MediaBox".as_slice(), b"CropBox", b"Rotate", b"Resources"] { let value = pdf.get_dictionary_mut(page).unwrap().remove(key).unwrap(); pdf.get_dictionary_mut(parent).unwrap().set(key, value); }
            let path = folder.path().join(format!("notes-{original}-{edited}.pdf")); pdf.save(&path).unwrap(); let source = std::fs::read(&path).unwrap();
            let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
            for _ in 0..edited { info = call(&service, |reply| Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap(); }
            let baseline = comments_png(&service, info.id, 0, 600); assert!(comments_yellow_bounds(&baseline).is_none());
            let text = call(&service, |reply| Request::Text(info.id, 0, info.revision, reply)).unwrap();
            let geometry = call(&service, |reply| Request::TextGeometry(info.id, 0, info.revision, reply)).unwrap(); let chars = serde_json::to_value(&geometry.characters).unwrap();
            let old_snapshot = print_snapshot(&service, info.id, info.revision); let before_print = service.print_render_blocking(old_snapshot.token, 0, 600, 600).unwrap();
            let dimensions = (info.pages[0].width, info.pages[0].height); let note_body = "Note-only é 漢字 😀\nSecond line";
            info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Create(0, marker, note_body.into()), reply)).unwrap();
            assert_eq!((info.pages[0].width, info.pages[0].height), dimensions);
            assert_eq!(call(&service, |reply| Request::Text(info.id, 0, info.revision, reply)).unwrap(), text);
            assert_eq!(serde_json::to_value(call(&service, |reply| Request::TextGeometry(info.id, 0, info.revision, reply)).unwrap().characters).unwrap(), chars);
            let list = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap(); assert_eq!(list.status, "supported"); assert_eq!(list.notes.len(), 1); assert_eq!(list.notes[0].contents, note_body);
            let note_id = list.notes[0].id.clone(); let rect = list.notes[0].rect.as_ref().unwrap();
            for (actual, expected) in [(rect.x, marker.x), (rect.y, marker.y), (rect.width, marker.width), (rect.height, marker.height)] { assert!((actual - expected).abs() < 0.00001); }
            let preview = comments_png(&service, info.id, 0, 600); comments_assert_box(&preview, marker); assert_eq!(comments_png(&service, info.id, 0, 600), preview);
            assert_eq!(service.print_render_blocking(old_snapshot.token, 0, 600, 600).unwrap().bgra, before_print.bgra, "An earlier snapshot must not acquire the new note");
            let snapshot = print_snapshot(&service, info.id, info.revision); comments_assert_box(&comments_print_rgb(&service.print_render_blocking(snapshot.token, 0, 600, 600).unwrap()), marker);
            let output_path = folder.path().join(format!("notes-copy-{original}-{edited}.pdf")); let saved = call(&service, |reply| Request::Save(info.id, None, output_path.clone(), reply)).unwrap(); info = saved.document;
            let output = lopdf::Document::load(&output_path).unwrap(); let output_page = output.get_pages()[&1]; assert_eq!(output.get_dictionary(output_page).unwrap().get(b"Annots").unwrap().as_array().unwrap().len(), 1);
            let reopened = call(&service, |reply| Request::Open(output_path, reply)).unwrap(); assert_eq!(call(&service, |reply| Request::Comments(reopened.id, 0, reply)).unwrap().notes[0].contents, note_body); comments_assert_box(&comments_png(&service, reopened.id, 0, 600), marker);
            assert_eq!(call(&service, |reply| Request::Text(reopened.id, 0, 0, reply)).unwrap(), text); call(&service, |reply| Request::Close(reopened.id, reply)).unwrap();
            info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, CropRect { x: 0.2, y: 0.0, width: 0.8, height: 1.0 }, reply)).unwrap();
            let clipped = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap(); let clipped = clipped.notes[0].rect.as_ref().unwrap(); assert!(clipped.x.abs() < 0.00001 && (clipped.width - 0.125).abs() < 0.00001);
            comments_assert_box(&comments_png(&service, info.id, 0, 480), CropRect { x: 0.0, y: 0.1, width: 0.125, height: 0.2 });
            info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, CropRect { x: 0.5, y: 0.0, width: 0.5, height: 1.0 }, reply)).unwrap();
            let hidden = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap(); assert!(hidden.notes[0].rect.is_none()); assert_eq!(hidden.notes[0].contents, note_body); assert!(comments_yellow_bounds(&comments_png(&service, info.id, 0, 240)).is_none());
            let hidden_path = folder.path().join(format!("hidden-{original}-{edited}.pdf")); let hidden_output = call(&service, |reply| Request::Save(info.id, None, hidden_path.clone(), reply)).unwrap(); assert_eq!(hidden_output.document.revision, info.revision);
            let hidden_reopen = call(&service, |reply| Request::Open(hidden_path, reply)).unwrap(); let hidden = call(&service, |reply| Request::Comments(hidden_reopen.id, 0, reply)).unwrap(); assert!(hidden.notes[0].rect.is_none()); assert_eq!(hidden.notes[0].id, note_id); call(&service, |reply| Request::Close(hidden_reopen.id, reply)).unwrap();
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap(); info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap(); comments_assert_box(&comments_png(&service, info.id, 0, 600), marker);
            info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Delete(note_id), reply)).unwrap(); assert!(call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes.is_empty()); assert_eq!(comments_png(&service, info.id, 0, 600), baseline);
            call(&service, |reply| Request::Close(info.id, reply)).unwrap(); comments_assert_box(&comments_print_rgb(&service.print_render_blocking(snapshot.token, 0, 600, 600).unwrap()), marker);
            assert_eq!(std::fs::read(path).unwrap(), source);
        } }
    }
    #[test]
    fn comments_corpus_copy_reopen_move_delete_extract_and_split_preserve_notes_and_sources() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        for (fixture, count) in [("resources/welcome.pdf", 6usize), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)] {
            let path = root.join(fixture); let source = std::fs::read(&path).unwrap(); let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
            let marker = CropRect { x: 0.04, y: 0.04, width: 0.08, height: 0.08 };
            for page in [0, count - 1] {
                let before = comments_png(&service, info.id, page as u16, 400);
                info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Create(page as u16, marker, format!("Note on source page {page} é 😀")), reply)).unwrap();
                let after = comments_png(&service, info.id, page as u16, 400); assert_ne!(after, before, "A corpus marker must appear in actual rendered pixels");
                for (x, y, pixel) in after.enumerate_pixels() {
                    let outside = (x as f64) < marker.x * after.width() as f64 - 2.0 || (x as f64) > (marker.x + marker.width) * after.width() as f64 + 2.0 || (y as f64) < marker.y * after.height() as f64 - 2.0 || (y as f64) > (marker.y + marker.height) * after.height() as f64 + 2.0;
                    if outside { assert_eq!(*pixel, *before.get_pixel(x, y), "Notes must not change unrelated corpus pixels"); }
                }
            }
            let first_id = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes[0].id.clone();
            info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Update(first_id.clone(), "Edited Unicode 漢字 😀".into()), reply)).unwrap();
            info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, CropRect { x: 0.0, y: 0.0, width: 0.9, height: 0.9 }, reply)).unwrap();
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap();
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Move { from: 0, to: count - 1 }, reply)).unwrap();
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Delete { pages: vec![1] }, reply)).unwrap();
            let list = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap(); assert_eq!(list.notes.len(), 2); assert_eq!(list.notes.iter().find(|note| note.id == first_id).unwrap().page, count - 2);
            let output_path = folder.path().join(format!("corpus-notes-{count}.pdf")); let saved = call(&service, |reply| Request::Save(info.id, None, output_path.clone(), reply)).unwrap(); info = saved.document; assert!(!info.dirty);
            let reopened = call(&service, |reply| Request::Open(output_path, reply)).unwrap(); assert_eq!(reopened.pages.len(), count - 1);
            let reopened_list = call(&service, |reply| Request::Comments(reopened.id, 0, reply)).unwrap(); assert_eq!(serde_json::to_value(list.notes).unwrap(), serde_json::to_value(reopened_list.notes).unwrap());
            for page in [count - 3, count - 2] { assert_eq!(comments_png(&service, info.id, page as u16, 400), comments_png(&service, reopened.id, page as u16, 400)); }
            let extraction = folder.path().join(format!("extracted-notes-{count}.pdf")); call(&service, |reply| Request::Save(info.id, Some(vec![0, count - 2]), extraction.clone(), reply)).unwrap();
            let extracted = call(&service, |reply| Request::Open(extraction, reply)).unwrap(); let notes = call(&service, |reply| Request::Comments(extracted.id, 0, reply)).unwrap(); assert_eq!(notes.notes.len(), 1); assert_eq!(notes.notes[0].id, first_id); assert_eq!(notes.notes[0].page, 1); assert_eq!(notes.notes[0].contents, "Edited Unicode 漢字 😀");
            if count == 6 {
                let split = call(&service, |reply| Request::Split(info.id, info.revision, 2, folder.path().join("notes-split"), reply)).unwrap(); let mut ids = Vec::new();
                for file in split.files { let part = call(&service, |reply| Request::Open(file.path.clone(), reply)).unwrap(); ids.extend(call(&service, |reply| Request::Comments(part.id, 0, reply)).unwrap().notes.into_iter().map(|note| note.id)); call(&service, |reply| Request::Close(part.id, reply)).unwrap(); }
                assert_eq!(ids.len(), 2); assert!(ids.contains(&first_id));
            }
            let donor = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
            assert!(call(&service, |reply| Request::CheckCombine(combine_source(&info), combine_source(&donor), reply)).unwrap_err().contains("Annots"));
            assert!(call(&service, |reply| Request::CheckInsertion(combine_source(&info), combine_source(&donor), 0, reply)).unwrap_err().contains("Annots"));
            assert!(call(&service, |reply| Request::CheckReplacement(combine_source(&info), combine_source(&donor), 0, 1, reply)).unwrap_err().contains("Annots"));
            let deleted_page = count - 2; info = call(&service, |reply| Request::Edit(info.id, PageEdit::Delete { pages: vec![deleted_page] }, reply)).unwrap(); assert!(!call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes.iter().any(|note| note.id == first_id));
            info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap(); assert!(call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes.iter().any(|note| note.id == first_id));
            for id in [info.id, reopened.id, extracted.id, donor.id] { call(&service, |reply| Request::Close(id, reply)).unwrap(); }
            assert_eq!(std::fs::read(path).unwrap(), source);
        }
    }
    #[test]
    fn comments_invalid_stale_closed_preclosed_and_branch_ids_preserve_session_state() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let mut info = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
        let marker = CropRect { x: 0.1, y: 0.1, width: 0.05, height: 0.05 }; let baseline = comments_png(&service, info.id, 0, 400);
        for (page, rect, contents) in [(99, marker, "text".into()), (0, CropRect { x: f64::NAN, ..marker }, "text".into()), (0, CropRect { x: -0.1, ..marker }, "text".into()), (0, CropRect { width: 0.000001, ..marker }, "text".into()), (0, marker, " \n".into()), (0, marker, "x\0y".into()), (0, marker, "x".repeat(8193))] {
            assert!(call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::Create(page, rect, contents), reply)).is_err());
            assert!(call(&service, |reply| Request::Comments(info.id, 0, reply)).unwrap().notes.is_empty());
        }
        let (reply, unread) = oneshot::channel(); drop(unread); service.sender.send(Request::Comment(info.id, 0, CommentMutation::Create(0, marker, "Canceled".into()), reply)).unwrap();
        assert!(call(&service, |reply| Request::Comments(info.id, 0, reply)).unwrap().notes.is_empty()); assert_eq!(comments_png(&service, info.id, 0, 400), baseline);
        info = call(&service, |reply| Request::Edit(info.id, PageEdit::Move { from: 0, to: 0 }, reply)).unwrap(); assert_eq!(info.revision, 0); assert!(!info.dirty && !info.can_undo);
        info = call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::Create(0, marker, "First".into()), reply)).unwrap(); let id = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes[0].id.clone();
        assert!(call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::Update(id.clone(), "Stale".into()), reply)).err().unwrap().contains("changed")); assert!(call(&service, |reply| Request::Comments(info.id, 0, reply)).is_err());
        let noop = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Update(id.clone(), "First".into()), reply)).unwrap(); assert_eq!(noop.revision, info.revision);
        info = call(&service, |reply| Request::Edit(info.id, PageEdit::Undo, reply)).unwrap(); assert!(call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes.is_empty());
        info = call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Create(0, marker, "Branch".into()), reply)).unwrap(); let next_id = call(&service, |reply| Request::Comments(info.id, info.revision, reply)).unwrap().notes[0].id.clone(); assert_ne!(next_id, id); assert!(!info.can_redo);
        assert!(call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Delete(id), reply)).is_err());
        call(&service, |reply| Request::Close(info.id, reply)).unwrap(); assert!(call(&service, |reply| Request::Comments(info.id, info.revision, reply)).is_err()); assert!(call(&service, |reply| Request::Comment(info.id, info.revision, CommentMutation::Delete(next_id), reply)).is_err());
    }
    #[test]
    fn comments_signed_forms_tagging_foreign_annotations_and_altered_owned_schema_refuse() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        for feature in ["Perms", "AcroForm", "StructTreeRoot", "Foreign", "Altered"] {
            let mut pdf = lopdf::Document::load(root.join("resources/welcome.pdf")).unwrap();
            match feature {
                "Foreign" => { let page = pdf.get_pages()[&1]; let annotation = pdf.add_object(lopdf::dictionary! { "Type" => "Annot", "Subtype" => "Text", "Rect" => vec![40.into(), 60.into(), 70.into(), 90.into()] }); pdf.get_dictionary_mut(page).unwrap().set("Annots", vec![lopdf::Object::Reference(annotation)]); },
                "Altered" => { let mut source = Vec::new(); pdf.save_to(&mut source).unwrap(); let mut session = EditSession::new(source, 6); let plan = session.proposed_comment(Some((0, CropBox { left: 40.0, bottom: 60.0, right: 70.0, top: 90.0 })), None, Some("Existing owned note")).unwrap(); session.commit_comments(plan); pdf = lopdf::Document::load_mem(&session.export(None).unwrap()).unwrap(); let page = pdf.get_pages()[&1]; let annotation = pdf.get_dictionary(page).unwrap().get(b"Annots").unwrap().as_array().unwrap()[0].as_reference().unwrap(); pdf.get_dictionary_mut(annotation).unwrap().set("Popup", lopdf::Object::Null); },
                feature => { pdf.catalog_mut().unwrap().set(feature, lopdf::dictionary! {}); },
            }
            let path = folder.path().join(format!("protected-notes-{feature}.pdf")); pdf.save(&path).unwrap(); let source = std::fs::read(&path).unwrap(); let info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
            let list = call(&service, |reply| Request::Comments(info.id, 0, reply)).unwrap(); assert_eq!(list.status, "unsupported"); assert!(list.reason.is_some()); assert!(list.notes.is_empty());
            assert!(call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::Create(0, CropRect { x: 0.1, y: 0.1, width: 0.1, height: 0.1 }, "Blocked".into()), reply)).is_err());
            assert_eq!(call(&service, |reply| Request::Annotations(info.id, 0, reply)).unwrap().status, "unsupported"); assert!(call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::CreateHighlight(0, CropRect { x: 0.1, y: 0.1, width: 0.1, height: 0.1 }, None), reply)).is_err());
            call(&service, |reply| Request::Close(info.id, reply)).unwrap(); assert_eq!(std::fs::read(path).unwrap(), source);
        }
    }
    fn scheduler_render(id: u64) -> Request { let (reply, _) = oneshot::channel(); Request::Render(id, 0, 64, reply) }
    fn scheduler_cleanup(id: u64, print: bool) -> Request {
        let (reply, _) = oneshot::channel();
        if print { Request::EndPrint(id, reply) } else { Request::Close(id, reply) }
    }
    #[test]
    fn worker_scheduler_preserves_every_nonrender_barrier_and_cleanup_order() {
        let first = CombineSource { id: 1, revision: 0 }; let second = CombineSource { id: 2, revision: 0 };
        let (tx, _) = oneshot::channel();
        let (begin_tx, _) = oneshot::channel(); let (print_tx, _) = oneshot::channel();
        let (save_tx, _) = oneshot::channel(); let (combine_tx, _) = oneshot::channel();
        let (text_tx, _) = oneshot::channel(); let (open_tx, _) = oneshot::channel();
        let (crop_tx, _) = oneshot::channel(); let (split_tx, _) = oneshot::channel();
        for barrier in [Request::Edit(1, PageEdit::Undo, tx), Request::BeginPrint(1, 0, begin_tx),
            Request::PrintRender(1, 0, 64, 64, print_tx), Request::Save(1, None, PathBuf::new(), save_tx),
            Request::Combine(first, second, PathBuf::new(), combine_tx), Request::Text(1, 0, 0, text_tx),
            Request::Open(PathBuf::new(), open_tx), Request::Crop(1, 0, 0, CropRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }, crop_tx),
            Request::Split(1, 0, 1, PathBuf::new(), split_tx)] {
            let (sender, receiver) = mpsc::channel();
            sender.send(scheduler_render(10)).unwrap(); sender.send(barrier).unwrap();
            sender.send(scheduler_cleanup(20, false)).unwrap(); sender.send(scheduler_cleanup(21, true)).unwrap();
            let mut queue = RequestQueue::default();
            assert!(matches!(queue.next(&receiver).unwrap(), Request::Render(10, ..)));
            assert!(!matches!(queue.next(&receiver).unwrap(), Request::Render(..) | Request::Close(..) | Request::EndPrint(..)));
            assert!(matches!(queue.next(&receiver).unwrap(), Request::Close(20, ..)));
            assert!(matches!(queue.next(&receiver).unwrap(), Request::EndPrint(21, ..)));
        }
    }
    #[test]
    fn worker_scheduler_window_is_bounded_and_render_stream_cannot_starve() {
        let (sender, receiver) = mpsc::channel();
        for id in 0..32 { sender.send(scheduler_render(id)).unwrap(); }
        sender.send(scheduler_cleanup(99, false)).unwrap();
        let mut queue = RequestQueue::default();
        assert!(matches!(queue.next(&receiver).unwrap(), Request::Render(0, ..)));
        assert_eq!(queue.deferred.len(), 31);
        assert!(matches!(queue.next(&receiver).unwrap(), Request::Close(99, ..)));
        assert_eq!(queue.deferred.len(), 31);
        for id in 100..110 { sender.send(scheduler_cleanup(id, false)).unwrap(); }
        assert!(matches!(queue.next(&receiver).unwrap(), Request::Close(100, ..)));
        assert!(matches!(queue.next(&receiver).unwrap(), Request::Render(1, ..)), "Two cleanup overtakes must be followed by ordinary render progress");
        let (sender, receiver) = mpsc::channel();
        for id in 0..100 { sender.send(scheduler_render(id)).unwrap(); }
        drop(sender);
        let mut queue = RequestQueue::default();
        for expected in 0..100 {
            assert!(matches!(queue.next(&receiver).unwrap(), Request::Render(id, ..) if id == expected));
            assert!(queue.deferred.len() < 32);
        }
        assert!(queue.next(&receiver).is_err());
    }
    #[test]
    fn worker_scheduler_fifo_control_discriminates_the_cleanup_regression() {
        let (sender, receiver) = mpsc::channel();
        let mut fifo = VecDeque::new();
        for id in 0..8 { fifo.push_back(scheduler_render(id)); }
        fifo.push_back(scheduler_cleanup(99, false)); fifo.push_back(scheduler_cleanup(100, true));
        let before_close = fifo.iter().take_while(|request| !matches!(request, Request::Close(..))).filter(|request| matches!(request, Request::Render(..))).count();
        assert_eq!(before_close, 8, "FIFO control must reproduce eight render dispatches before Close");
        let mut queue = RequestQueue { deferred: fifo, cleanup_overtakes: 0 };
        assert!(matches!(queue.next(&receiver).unwrap(), Request::Close(99, ..)));
        assert!(matches!(queue.next(&receiver).unwrap(), Request::EndPrint(100, ..)));
        drop(sender);
    }
    fn wait_probe_marker(receiver: &mut oneshot::Receiver<Result<(), String>>, deadline: std::time::Instant) -> Option<std::time::Instant> {
        loop {
            match receiver.try_recv() {
                Ok(result) => { result.unwrap(); return Some(std::time::Instant::now()); }
                Err(oneshot::error::TryRecvError::Closed) => panic!("The worker stopped before its backlog marker reply"),
                Err(oneshot::error::TryRecvError::Empty) => {
                    if std::time::Instant::now() >= deadline { return None; }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
    }
    #[test]
    fn worker_cleanup_passes_queued_live_viewer_renders_without_rendering_closed_document() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let info = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap();
        let snapshot = print_snapshot(&service, info.id, info.revision);
        let _cleanup = UnreadPrintCleanup { service: service.clone(), document: info.id, tokens: vec![snapshot.token] };
        let (release, gate) = mpsc::channel();
        call(&service, |reply| Request::BacklogGate(gate, reply)).unwrap();
        let mut responses = Vec::new();
        let mut batch = Vec::new();
        for index in 0..8 {
            let (tx, rx) = oneshot::channel();
            responses.push(rx);
            batch.push(Request::Render(info.id, 0, 64 + index * 64, tx));
        }
        let (close_tx, close_rx) = oneshot::channel();
        batch.push(Request::Close(info.id, close_tx));
        let (end_tx, end_rx) = oneshot::channel();
        batch.push(Request::EndPrint(snapshot.token, end_tx));
        release.send(batch).unwrap();
        close_rx.blocking_recv().unwrap().unwrap();
        end_rx.blocking_recv().unwrap().unwrap();
        let results: Vec<_> = responses.into_iter().map(|rx| rx.blocking_recv().unwrap()).collect();
        let work = call(&service, |reply| Request::RenderWorkForDocument(info.id, reply)).unwrap();
        assert_eq!(work.dequeued, 8);
        assert_eq!(work.skipped_closed, 0, "Viewer replies remain live; cleanup must prevent obsolete engine work");
        assert_eq!(work.rendered, 0, "Queued live viewer renders delayed cleanup and rendered a document being closed");
        assert!(results.iter().all(|result| result.as_ref().err().is_some_and(|error| error.contains("closed"))));
        assert!(service.print_render_blocking(snapshot.token, 0, 64, 64).err().unwrap().contains("ended"));
        assert!(call(&service, |reply| Request::Properties(info.id, info.revision, reply)).err().unwrap().contains("closed"));
    }
    #[test]
    #[ignore = "Manual bounded backlog measurement; no timing regression assertion"]
    fn worker_backlog_probe_measures_close_release_and_live_reply_retention() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let path = root.join("../test-corpus/synthetic-scan-98.pdf");
        let source = std::fs::read(&path).unwrap(); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        for (mode, count, preclosed) in [("baseline", 0, false), ("live_unread", 8, false), ("preclosed_receivers", 8, true)] {
            let experiment_start = std::time::Instant::now(); let deadline = experiment_start + std::time::Duration::from_secs(30);
            let info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
            let snapshot = call(&service, |reply| Request::BeginPrint(info.id, info.revision, reply)).unwrap();
            let _cleanup = UnreadPrintCleanup { service: service.clone(), document: info.id, tokens: vec![snapshot.token] };
            let mut responses = Vec::new();
            for index in 0..count {
                let (tx, rx) = oneshot::channel();
                if preclosed { drop(rx); } else { responses.push(rx); }
                service.sender.send(Request::Render(info.id, 0, 2048 + index * 64, tx)).unwrap();
            }
            let close_enqueued = std::time::Instant::now(); let (close_tx, mut close_rx) = oneshot::channel();
            service.sender.send(Request::Close(info.id, close_tx)).unwrap();
            let release_enqueued = std::time::Instant::now(); let (release_tx, mut release_rx) = oneshot::channel();
            service.sender.send(Request::EndPrint(snapshot.token, release_tx)).unwrap();
            let closed = wait_probe_marker(&mut close_rx, deadline); let released = wait_probe_marker(&mut release_rx, deadline);
            if closed.is_none() || released.is_none() {
                drop(responses);
                println!("backlog mode={mode} source_bytes={} requests={count} soft_budget_exceeded=true; pending receivers dropped to skip remaining work", source.len());
                continue;
            }
            let mut retained_png_bytes = 0usize;
            let mut closed_errors = 0usize;
            for receiver in responses {
                match receiver.blocking_recv().unwrap() {
                    Ok(bytes) => retained_png_bytes += bytes.len(),
                    Err(error) => { assert!(error.contains("closed")); closed_errors += 1; }
                }
            }
            let work = call(&service, |reply| Request::RenderWorkForDocument(info.id, reply)).unwrap();
            println!("backlog mode={mode} source_bytes={} requests={count} widths=2048..2496 step=64 page=0 close_ms={:.3} release_ms={:.3} elapsed_ms={:.3} retained_png_bytes={retained_png_bytes} closed_errors={closed_errors} dequeued={} skipped_closed={} rendered={}", source.len(), closed.unwrap().duration_since(close_enqueued).as_secs_f64() * 1000.0, released.unwrap().duration_since(release_enqueued).as_secs_f64() * 1000.0, experiment_start.elapsed().as_secs_f64() * 1000.0, work.dequeued, work.skipped_closed, work.rendered);
            assert_eq!(work.dequeued, count as usize);
            assert_eq!(work.skipped_closed, if preclosed { count as usize } else { 0 });
            assert_eq!(work.rendered + closed_errors, if preclosed { 0 } else { count as usize });
            if mode != "live_unread" { assert_eq!(retained_png_bytes, 0); }
            assert!(service.print_render_blocking(snapshot.token, 0, 64, 64).err().unwrap().contains("ended"));
            assert!(call(&service, |reply| Request::Properties(info.id, info.revision, reply)).err().unwrap().contains("closed"));
        }
        assert_eq!(std::fs::read(path).unwrap(), source);
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
    struct UnreadReplyCleanup { service: PdfService, paths: Vec<PathBuf> }
    impl Drop for UnreadReplyCleanup {
        fn drop(&mut self) {
            for path in &self.paths {
                let (tx, rx) = oneshot::channel();
                if self.service.sender.send(Request::OpenDocumentsForPath(path.clone(), tx)).is_ok() {
                    if let Ok(Ok(ids)) = rx.blocking_recv() {
                        for id in ids {
                            let (tx, rx) = oneshot::channel();
                            if self.service.sender.send(Request::Close(id, tx)).is_ok() { let _ = rx.blocking_recv(); }
                        }
                    }
                }
                let (tx, rx) = oneshot::channel();
                if self.service.sender.send(Request::PasswordRequestsForPath(path.clone(), tx)).is_ok() {
                    if let Ok(Ok(ids)) = rx.blocking_recv() {
                        for id in ids {
                            let (tx, rx) = oneshot::channel();
                            if self.service.sender.send(Request::CancelPassword(id, tx)).is_ok() { let _ = rx.blocking_recv(); }
                        }
                    }
                }
            }
        }
    }
    fn unread_document_reply<T>(service: &PdfService, path: &PathBuf, request: impl FnOnce(Reply<T>) -> Request) -> Vec<u64> {
        let (tx, rx) = oneshot::channel(); service.sender.send(request(tx)).unwrap();
        let registered = call(service, |reply| Request::OpenDocumentsForPath(path.clone(), reply)).unwrap();
        assert_eq!(registered.len(), 1, "The worker barrier must observe the successfully registered document");
        assert!(!rx.is_empty(), "The result must already be sent before its unread receiver is dropped");
        drop(rx);
        call(service, |reply| Request::OpenDocumentsForPath(path.clone(), reply)).unwrap()
    }
    fn password_source(root: &PathBuf) -> Vec<u8> {
        let mut pdf = lopdf::Document::load(root.join("resources/welcome.pdf")).unwrap();
        pdf.trailer.set("ID", vec![lopdf::Object::string_literal("unread-password-fixture"), lopdf::Object::string_literal("unread-password-fixture")]);
        let encryption = lopdf::EncryptionVersion::V2 { document: &pdf, owner_password: "owner password", user_password: "test password", key_length: 128, permissions: lopdf::Permissions::all() };
        let state = lopdf::EncryptionState::try_from(encryption).unwrap(); pdf.encrypt(&state).unwrap();
        let mut bytes = Vec::new(); pdf.save_to(&mut bytes).unwrap(); bytes
    }
    #[test]
    fn unread_document_open_and_begin_open_success_replies_release_registered_sessions() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let source = std::fs::read(root.join("resources/welcome.pdf")).unwrap();
        let mut cleanup = UnreadReplyCleanup { service: service.clone(), paths: Vec::new() }; let mut leaks = Vec::new();
        for begin_open in [false, true] {
            let path = folder.path().join(if begin_open { "begin-open.pdf" } else { "open.pdf" }); std::fs::write(&path, &source).unwrap(); cleanup.paths.push(path.clone());
            let retained = if begin_open { unread_document_reply(&service, &path, |reply| Request::BeginOpen(path.clone(), reply)) }
                else { unread_document_reply(&service, &path, |reply| Request::Open(path.clone(), reply)) };
            leaks.push((if begin_open { "BeginOpen" } else { "Open" }, retained.len()));
            assert_eq!(std::fs::read(&path).unwrap(), source);
        }
        assert!(leaks.iter().all(|(_, count)| *count == 0), "Successfully sent but unread document replies retained sessions: {leaks:?}");
    }
    #[test]
    fn unread_document_unlock_success_reply_releases_registered_session() {
        let _password_lock = password_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("unlock.pdf"); let source = password_source(&root); std::fs::write(&path, &source).unwrap();
        let _cleanup = UnreadReplyCleanup { service: service.clone(), paths: vec![path.clone()] };
        let challenge = call(&service, |reply| Request::BeginOpen(path.clone(), reply)).unwrap();
        let request_id = match challenge { OpenResult::PasswordRequired { request_id, .. } => request_id, _ => panic!("Expected password challenge") };
        let retained = unread_document_reply(&service, &path, |reply| Request::Unlock(request_id, "test password".into(), reply));
        assert_eq!(std::fs::read(&path).unwrap(), source);
        assert!(retained.is_empty(), "Successfully sent but unread Unlock retained {} session(s)", retained.len());
    }
    #[test]
    fn unread_document_combine_success_reply_releases_session_and_preserves_published_pdf() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let source = std::fs::read(root.join("resources/welcome.pdf")).unwrap(); let first_path = folder.path().join("first.pdf"); let second_path = folder.path().join("second.pdf"); let output = folder.path().join("combined.pdf");
        std::fs::write(&first_path, &source).unwrap(); std::fs::write(&second_path, &source).unwrap();
        let _cleanup = UnreadReplyCleanup { service: service.clone(), paths: vec![first_path.clone(), second_path.clone(), output.clone()] };
        let first = call(&service, |reply| Request::Open(first_path.clone(), reply)).unwrap(); let second = call(&service, |reply| Request::Open(second_path.clone(), reply)).unwrap();
        let retained = unread_document_reply(&service, &output, |reply| Request::Combine(combine_source(&first), combine_source(&second), output.clone(), reply));
        assert_eq!(lopdf::Document::load(&output).unwrap().get_pages().len(), 12);
        let reopened = call(&service, |reply| Request::Open(output.clone(), reply)).unwrap(); assert_eq!(reopened.pages.len(), 12);
        call(&service, |reply| Request::Close(reopened.id, reply)).unwrap();
        assert_eq!(std::fs::read(&first_path).unwrap(), source); assert_eq!(std::fs::read(&second_path).unwrap(), source);
        for source in [&first, &second] { assert_eq!(call(&service, |reply| Request::Properties(source.id, 0, reply)).unwrap().page_count, 6); }
        assert!(retained.is_empty(), "Successfully sent but unread Combine retained {} session(s)", retained.len());
    }
    #[test]
    fn unread_password_required_reply_releases_pending_challenge() {
        let _password_lock = password_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("password.pdf"); let source = password_source(&root); std::fs::write(&path, &source).unwrap();
        let _cleanup = UnreadReplyCleanup { service: service.clone(), paths: vec![path.clone()] };
        let (tx, rx) = oneshot::channel(); service.sender.send(Request::BeginOpen(path.clone(), tx)).unwrap();
        let pending = call(&service, |reply| Request::PasswordRequestsForPath(path.clone(), reply)).unwrap(); assert_eq!(pending.len(), 1);
        assert!(!rx.is_empty(), "The challenge reply must be sent before its unread receiver is dropped"); drop(rx);
        let retained = call(&service, |reply| Request::PasswordRequestsForPath(path.clone(), reply)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), source);
        assert!(retained.is_empty(), "Successfully sent but unread PasswordRequired retained {} pending token(s)", retained.len());
    }
    #[test]
    fn accepted_document_replies_and_password_challenges_remain_usable_after_dto_drop() {
        let _password_lock = password_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let first_path = folder.path().join("first.pdf"); let second_path = folder.path().join("second.pdf"); let password_path = folder.path().join("password.pdf"); let output = folder.path().join("combined.pdf");
        let source = std::fs::read(root.join("resources/welcome.pdf")).unwrap(); std::fs::write(&first_path, &source).unwrap(); std::fs::write(&second_path, &source).unwrap(); std::fs::write(&password_path, password_source(&root)).unwrap();
        let _cleanup = UnreadReplyCleanup { service: service.clone(), paths: vec![first_path.clone(), second_path.clone(), password_path.clone(), output.clone()] };
        let first = tauri::async_runtime::block_on(service.open(first_path)).unwrap(); let first_source = combine_source(&first); drop(first);
        let second = tauri::async_runtime::block_on(service.begin_open(second_path)).unwrap();
        let second_source = match &second { OpenResult::Opened { document } => combine_source(document), _ => panic!("Ordinary PDF must open") }; drop(second);
        for id in [first_source.id, second_source.id] { assert_eq!(call(&service, |reply| Request::Properties(id, 0, reply)).unwrap().page_count, 6); }
        let challenge = tauri::async_runtime::block_on(service.begin_open(password_path.clone())).unwrap();
        let request_id = match &challenge { OpenResult::PasswordRequired { request_id, .. } => *request_id, _ => panic!("Expected password challenge") }; drop(challenge);
        let wrong = tauri::async_runtime::block_on(service.unlock(request_id, "wrong".into())).unwrap(); assert!(matches!(wrong, OpenResult::PasswordRequired { incorrect: true, .. })); drop(wrong);
        let opened = tauri::async_runtime::block_on(service.unlock(request_id, "test password".into())).unwrap();
        let unlocked_id = match &opened { OpenResult::Opened { document } => document.id, _ => panic!("Correct password must open") }; drop(opened);
        assert_eq!(call(&service, |reply| Request::Properties(unlocked_id, 0, reply)).unwrap().page_count, 6);
        let text = call(&service, |reply| Request::Text(unlocked_id, 0, 0, reply)).unwrap(); assert!(text.contains("Page 1 of 6"));
        let challenge = tauri::async_runtime::block_on(service.begin_open(password_path.clone())).unwrap();
        let canceled = match &challenge { OpenResult::PasswordRequired { request_id, .. } => *request_id, _ => panic!("Expected password challenge") }; drop(challenge);
        tauri::async_runtime::block_on(service.cancel_password(canceled)).unwrap(); assert!(tauri::async_runtime::block_on(service.unlock(canceled, "test password".into())).is_err());
        let combined = tauri::async_runtime::block_on(service.combine(first_source, second_source, output.clone())).unwrap(); let combined_id = combined.document.id;
        assert!(!combined.document.dirty && !combined.document.can_undo && !combined.document.can_redo); drop(combined);
        assert_eq!(call(&service, |reply| Request::Properties(combined_id, 0, reply)).unwrap().page_count, 12);
        assert!(call(&service, |reply| Request::Text(combined_id, 11, 0, reply)).unwrap().contains("Page 6 of 6"));
        assert_eq!(lopdf::Document::load(output).unwrap().get_pages().len(), 12);
    }
    #[test]
    fn unread_preclosed_open_begin_unlock_and_combine_replies_never_retain_resources_or_publish() {
        let _password_lock = password_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("ordinary.pdf"); let password_path = folder.path().join("password.pdf"); let output = folder.path().join("must-not-publish.pdf");
        let source = std::fs::read(root.join("resources/welcome.pdf")).unwrap(); let encrypted = password_source(&root); std::fs::write(&path, &source).unwrap(); std::fs::write(&password_path, &encrypted).unwrap();
        let _cleanup = UnreadReplyCleanup { service: service.clone(), paths: vec![path.clone(), password_path.clone(), output.clone()] };
        let (tx, rx) = oneshot::channel(); drop(rx); service.sender.send(Request::Open(path.clone(), tx)).unwrap();
        let (tx, rx) = oneshot::channel(); drop(rx); service.sender.send(Request::BeginOpen(password_path.clone(), tx)).unwrap();
        assert!(call(&service, |reply| Request::OpenDocumentsForPath(path.clone(), reply)).unwrap().is_empty());
        assert!(call(&service, |reply| Request::PasswordRequestsForPath(password_path.clone(), reply)).unwrap().is_empty());
        let challenge = call(&service, |reply| Request::BeginOpen(password_path.clone(), reply)).unwrap(); let request_id = match challenge { OpenResult::PasswordRequired { request_id, .. } => request_id, _ => panic!("Expected password challenge") };
        let (tx, rx) = oneshot::channel(); drop(rx); service.sender.send(Request::Unlock(request_id, "test password".into(), tx)).unwrap();
        assert!(call(&service, |reply| Request::PasswordRequestsForPath(password_path.clone(), reply)).unwrap().is_empty());
        assert!(call(&service, |reply| Request::OpenDocumentsForPath(password_path.clone(), reply)).unwrap().is_empty());
        let first = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); let second = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap();
        let (tx, rx) = oneshot::channel(); drop(rx); service.sender.send(Request::Combine(combine_source(&first), combine_source(&second), output.clone(), tx)).unwrap();
        assert!(call(&service, |reply| Request::OpenDocumentsForPath(output.clone(), reply)).unwrap().is_empty()); assert!(!output.exists());
        assert_eq!(std::fs::read(path).unwrap(), source); assert_eq!(std::fs::read(password_path).unwrap(), encrypted);
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
    struct TestDocument { service: PdfService, id: u64 }
    impl Drop for TestDocument {
        fn drop(&mut self) {
            let (tx, rx) = oneshot::channel();
            if self.service.sender.send(Request::Close(self.id, tx)).is_ok() { let _ = rx.blocking_recv(); }
        }
    }
    #[test]
    fn replace_all_ordered_fixture_pairs_validate_every_output_page_and_preserve_source_history() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        let fixtures = [("resources/welcome.pdf", 6usize), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)];
        for (target_index, (target_fixture, target_count)) in fixtures.iter().enumerate() {
            for (donor_index, (donor_fixture, donor_count)) in fixtures.iter().enumerate() {
                if target_index == donor_index { continue; }
                let target_path = root.join(target_fixture); let donor_path = root.join(donor_fixture);
                let target_bytes = std::fs::read(&target_path).unwrap(); let donor_bytes = std::fs::read(&donor_path).unwrap();
                let mut target = call(&service, |reply| Request::Open(target_path.clone(), reply)).unwrap(); let _target_cleanup = TestDocument { service: service.clone(), id: target.id };
                let mut donor = call(&service, |reply| Request::Open(donor_path.clone(), reply)).unwrap(); let _donor_cleanup = TestDocument { service: service.clone(), id: donor.id };
                for edit in [PageEdit::Move { from: 0, to: 2 }, PageEdit::Rotate { pages: vec![0], clockwise: true }, PageEdit::Delete { pages: vec![1] }] { target = call(&service, |reply| Request::Edit(target.id, edit, reply)).unwrap(); }
                for edit in [PageEdit::Delete { pages: vec![1, 3] }, PageEdit::Rotate { pages: vec![0], clockwise: false }, PageEdit::Move { from: 0, to: 1 }] { donor = call(&service, |reply| Request::Edit(donor.id, edit, reply)).unwrap(); }
                let count = 10.min(target.pages.len() - 2); let start = (target.pages.len() - count) / 2;
                let path = folder.path().join(format!("replace-{target_count}-{donor_count}.pdf"));
                call(&service, |reply| Request::CheckReplacement(combine_source(&target), combine_source(&donor), start, count, reply)).unwrap();
                let started = std::time::Instant::now();
                let replaced = call(&service, |reply| Request::ReplacePages(combine_source(&target), combine_source(&donor), start, count, path.clone(), reply)).unwrap().document;
                let _replaced_cleanup = TestDocument { service: service.clone(), id: replaced.id };
                println!("replace {target_count}+{donor_count} start={start} count={count}: {} current pages, validation/write {:?}", replaced.pages.len(), started.elapsed());
                assert_ne!(replaced.id, target.id); assert_ne!(replaced.id, donor.id); assert_eq!(replaced.pages.len(), target_count + donor_count - 3 - count);
                assert_eq!(replaced.revision, 0); assert!(!replaced.dirty && !replaced.can_undo && !replaced.can_redo);
                let reopened = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); let _reopened_cleanup = TestDocument { service: service.clone(), id: reopened.id };
                for position in 0..replaced.pages.len() {
                    let (source, local, source_count, is_target) = if position < start { (&target, position, target_count, true) }
                        else if position < start + donor.pages.len() { (&donor, position - start, donor_count, false) }
                        else { (&target, position - donor.pages.len() + count, target_count, true) };
                    let original = if is_target { match local { 0 => 1, 1 => 0, other => other + 1 } } else { match local { 0 => 2, 1 => 0, other => other + 2 } };
                    let footer = format!("Page {} of {source_count}", original + 1);
                    assert_eq!((replaced.pages[position].width, replaced.pages[position].height), (source.pages[local].width, source.pages[local].height));
                    let expected = image::load_from_memory(&call(&service, |reply| Request::Render(source.id, local as u16, 64, reply)).unwrap()).unwrap().into_rgba8();
                    for id in [replaced.id, reopened.id] {
                        assert!(call(&service, |reply| Request::Text(id, position as u16, 0, reply)).unwrap().contains(&footer), "Replacement source/order differs at output page {position}");
                        let actual = image::load_from_memory(&call(&service, |reply| Request::Render(id, position as u16, 64, reply)).unwrap()).unwrap().into_rgba8();
                        assert_eq!(actual, expected, "Replacement/reopened render differs at output page {position}");
                    }
                }
                assert_eq!(std::fs::read(target_path).unwrap(), target_bytes); assert_eq!(std::fs::read(donor_path).unwrap(), donor_bytes);
                for source in [&target, &donor] {
                    assert_eq!(call(&service, |reply| Request::Properties(source.id, source.revision, reply)).unwrap().page_count, source.pages.len());
                    for undo in 0..3 {
                        let restored = call(&service, |reply| Request::Edit(source.id, PageEdit::Undo, reply)).unwrap();
                        assert_eq!(restored.revision, source.revision + undo + 1); assert_eq!(restored.dirty, undo != 2); assert!(restored.can_redo);
                    }
                }
            }
        }
    }
    #[test]
    fn replace_duplicate_invalid_stale_and_closed_after_preflight_never_write_or_mutate_sources() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let mut target = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap(); let _target_cleanup = TestDocument { service: service.clone(), id: target.id };
        let mut donor = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap(); let _donor_cleanup = TestDocument { service: service.clone(), id: donor.id };
        let path = folder.path().join("must-not-replace.pdf");
        let replace = |target, donor, start, count| call(&service, |reply| Request::ReplacePages(target, donor, start, count, path.clone(), reply));
        assert!(replace(combine_source(&target), combine_source(&target), 0, 1).err().unwrap().contains("different"));
        for (start,count) in [(0,0),(6,1),(5,2),(usize::MAX,1),(1,usize::MAX)] { assert!(replace(combine_source(&target), combine_source(&donor), start, count).err().unwrap().contains("range")); }
        assert!(serde_json::from_value::<usize>(serde_json::json!(0.5)).is_err()); assert!(serde_json::from_value::<usize>(serde_json::json!(-1)).is_err());
        assert_eq!(call(&service, |reply| Request::Properties(target.id, 0, reply)).unwrap().page_count, 6);
        assert_eq!(call(&service, |reply| Request::Properties(donor.id, 0, reply)).unwrap().page_count, 6);
        call(&service, |reply| Request::CheckReplacement(combine_source(&target), combine_source(&donor), 2, 2, reply)).unwrap();
        let old_donor = combine_source(&donor); donor = call(&service, |reply| Request::Edit(donor.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap();
        assert!(replace(combine_source(&target), old_donor, 2, 2).err().unwrap().contains("changed"));
        call(&service, |reply| Request::CheckReplacement(combine_source(&target), combine_source(&donor), 2, 2, reply)).unwrap();
        let old_target = combine_source(&target); target = call(&service, |reply| Request::Edit(target.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap();
        assert!(replace(old_target, combine_source(&donor), 2, 2).err().unwrap().contains("changed"));
        call(&service, |reply| Request::CheckReplacement(combine_source(&target), combine_source(&donor), 2, 2, reply)).unwrap();
        call(&service, |reply| Request::Close(donor.id, reply)).unwrap(); assert!(replace(combine_source(&target), combine_source(&donor), 2, 2).err().unwrap().contains("Donor document is closed"));
        call(&service, |reply| Request::Close(target.id, reply)).unwrap(); assert!(replace(combine_source(&target), combine_source(&donor), 2, 2).err().unwrap().contains("Target document is closed"));
        assert!(!path.exists()); assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
    }
    #[test]
    fn replace_cropped_inherited_original_and_edited_rotations_keep_independent_ink_and_visible_tokens() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        for original in [0, 90, 180, 270] {
            for edited in 0..4 {
                let mut sources = Vec::new(); let mut expected = Vec::new(); let mut cleanup = Vec::new();
                for (index, label) in ["Target", "Donor"].into_iter().enumerate() {
                    let rotation = if index == 0 { original } else { (360 - original) % 360 };
                    let bytes = crate::text_geometry::tests::fixture(rotation, [1.0, 0.0, 0.0, 1.0, 100.0, 300.0], &format!("{label}OutsideTop\n\n{label}Header\n{label}Body\n\n{label}OutsideBottom"));
                    let mut pdf = lopdf::Document::load_mem(&bytes).unwrap(); let page = pdf.get_pages()[&1];
                    pdf.get_dictionary_mut(page).unwrap().set("MediaBox", vec![20.into(), 30.into(), 420.into(), 430.into()]);
                    let parent = pdf.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
                    for key in [b"MediaBox".as_slice(), b"CropBox", b"Rotate", b"Resources"] { let value = pdf.get_dictionary_mut(page).unwrap().remove(key).unwrap(); pdf.get_dictionary_mut(parent).unwrap().set(key, value); }
                    let visible_page = if index == 0 {
                        let retained = pdf.add_object(pdf.get_dictionary(page).unwrap().clone());
                        let removed = pdf.get_page_content(page); let removed = String::from_utf8(removed).unwrap().replace("Target", "Removed").into_bytes();
                        let removed = pdf.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), removed)); pdf.get_dictionary_mut(page).unwrap().set("Contents", removed);
                        pdf.get_dictionary_mut(parent).unwrap().set("Kids", vec![lopdf::Object::Reference(page), lopdf::Object::Reference(retained)]);
                        pdf.get_dictionary_mut(parent).unwrap().set("Count", 2); 1
                    } else { 0 };
                    let path = folder.path().join(format!("replace-source-{original}-{edited}-{index}.pdf")); pdf.save(&path).unwrap(); let source_bytes = std::fs::read(&path).unwrap();
                    let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); cleanup.push(TestDocument { service: service.clone(), id: info.id });
                    for _ in 0..edited { info = call(&service, |reply| Request::Edit(info.id, PageEdit::Rotate { pages: if index == 0 { vec![0,1] } else { vec![0] }, clockwise: index == 0 }, reply)).unwrap(); }
                    let turns = (rotation / 90 + if index == 0 { edited } else { (4 - edited) % 4 }) % 4;
                    let (rect, region, old_width, new_width) = match turns {
                        0 => (CropRect { x: 0.1, y: 70.0 / 260.0, width: 0.8, height: 110.0 / 260.0 }, (90, 210, 720, 330), 900, 720),
                        1 => (CropRect { x: 80.0 / 260.0, y: 0.1, width: 110.0 / 260.0, height: 0.8 }, (240, 90, 330, 720), 780, 330),
                        2 => (CropRect { x: 0.1, y: 80.0 / 260.0, width: 0.8, height: 110.0 / 260.0 }, (90, 240, 720, 330), 900, 720),
                        _ => (CropRect { x: 70.0 / 260.0, y: 0.1, width: 110.0 / 260.0, height: 0.8 }, (210, 90, 330, 720), 780, 330),
                    };
                    let before = image::load_from_memory(&call(&service, |reply| Request::Render(info.id, visible_page, old_width, reply)).unwrap()).unwrap().into_rgb8();
                    expected.push((image::imageops::crop_imm(&before, region.0, region.1, region.2, region.3).to_image(), new_width, turns));
                    info = call(&service, |reply| Request::Crop(info.id, visible_page, info.revision, rect, reply)).unwrap(); sources.push((info, path, source_bytes));
                }
                let path = folder.path().join(format!("replace-cropped-{original}-{edited}.pdf"));
                let replaced = call(&service, |reply| Request::ReplacePages(combine_source(&sources[0].0), combine_source(&sources[1].0), 0, 1, path.clone(), reply)).unwrap().document;
                cleanup.push(TestDocument { service: service.clone(), id: replaced.id }); assert_eq!(replaced.pages.len(), 2);
                let reopened = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); cleanup.push(TestDocument { service: service.clone(), id: reopened.id });
                let output = lopdf::Document::load(&path).unwrap();
                for (position, (source_index, label)) in [(1usize, "Donor"), (0, "Target")].into_iter().enumerate() {
                    let (expected, width, turns) = &expected[source_index]; let dimensions = if turns % 2 == 0 { (240.0, 110.0) } else { (110.0, 240.0) };
                    assert_eq!((replaced.pages[position].width, replaced.pages[position].height), dimensions);
                    for id in [replaced.id, reopened.id] {
                        let actual = image::load_from_memory(&call(&service, |reply| Request::Render(id, position as u16, *width, reply)).unwrap()).unwrap().into_rgb8(); assert_same_ink(&actual, expected);
                        let text = call(&service, |reply| Request::Text(id, position as u16, 0, reply)).unwrap();
                        let geometry = call(&service, |reply| Request::TextGeometry(id, position as u16, 0, reply)).unwrap(); assert_eq!(geometry.status, "ok");
                        let copied = geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>();
                        for token in [format!("{label}Header"), format!("{label}Body")] { assert!(text.contains(&token), "{original}/{edited}: {text}"); assert!(copied.contains(&token), "{original}/{edited}: {copied}"); }
                        for token in [format!("{label}OutsideTop"), format!("{label}OutsideBottom"), "RemovedHeader".into(), "RemovedBody".into()] { assert!(!text.contains(&token) && !copied.contains(&token)); }
                    }
                    assert!(String::from_utf8_lossy(&output.get_page_content(output.get_pages()[&(position as u32 + 1)])).contains(&format!("{label}OutsideBottom")), "Replacement must preserve underlying crop-hidden content");
                    let (source, source_path, source_bytes) = &sources[source_index]; assert_eq!(std::fs::read(source_path).unwrap(), *source_bytes);
                    let undo = call(&service, |reply| Request::Edit(source.id, PageEdit::Undo, reply)).unwrap(); assert_eq!(undo.revision, source.revision + 1); assert!(undo.can_redo);
                }
            }
        }
    }
    #[test]
    fn replace_unread_and_preclosed_replies_preserve_publication_and_accepted_copy_owns_its_source() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let source = std::fs::read(root.join("resources/welcome.pdf")).unwrap(); let target_path = folder.path().join("target.pdf"); let donor_path = folder.path().join("donor.pdf");
        std::fs::write(&target_path, &source).unwrap(); std::fs::write(&donor_path, &source).unwrap();
        let target = call(&service, |reply| Request::Open(target_path.clone(), reply)).unwrap(); let _target_cleanup = TestDocument { service: service.clone(), id: target.id };
        let donor = call(&service, |reply| Request::Open(donor_path.clone(), reply)).unwrap(); let _donor_cleanup = TestDocument { service: service.clone(), id: donor.id };
        let target_print = print_snapshot(&service, target.id, target.revision); let donor_print = print_snapshot(&service, donor.id, donor.revision);
        let original_print = service.print_render_blocking(target_print.token, 0, 64, 64).unwrap().bgra;
        let canceled = folder.path().join("canceled.pdf"); let (tx, rx) = oneshot::channel(); drop(rx);
        service.sender.send(Request::ReplacePages(combine_source(&target), combine_source(&donor), 0, 1, canceled.clone(), tx)).unwrap();
        call(&service, |reply| Request::Properties(target.id, target.revision, reply)).unwrap(); assert!(!canceled.exists());
        let output = folder.path().join("unread.pdf"); let _unread_cleanup = UnreadReplyCleanup { service: service.clone(), paths: vec![output.clone()] };
        let retained = unread_document_reply(&service, &output, |reply| Request::ReplacePages(combine_source(&target), combine_source(&donor), 2, 2, output.clone(), reply));
        assert!(retained.is_empty(), "Unread replacement result must release only the new session"); assert!(output.exists());
        let reopened = call(&service, |reply| Request::Open(output.clone(), reply)).unwrap(); let _reopened_cleanup = TestDocument { service: service.clone(), id: reopened.id }; assert_eq!(reopened.pages.len(), 10);
        let accepted_path = folder.path().join("accepted.pdf");
        let accepted = tauri::async_runtime::block_on(service.replace_pages_copy(combine_source(&target), combine_source(&donor), 0, 6, accepted_path.clone())).unwrap();
        assert_eq!(accepted.path, accepted_path.to_string_lossy()); assert_eq!(accepted.document.path, accepted.path); assert_eq!(accepted.document.pages.len(), 6);
        assert!(!accepted.document.dirty && !accepted.document.can_undo && !accepted.document.can_redo);
        let accepted_id = accepted.document.id; drop(accepted); let _accepted_cleanup = TestDocument { service: service.clone(), id: accepted_id };
        assert_eq!(call(&service, |reply| Request::Properties(accepted_id, 0, reply)).unwrap().page_count, 6);
        assert_eq!(std::fs::read(target_path).unwrap(), source); assert_eq!(std::fs::read(donor_path).unwrap(), source);
        call(&service, |reply| Request::Close(target.id, reply)).unwrap(); call(&service, |reply| Request::Close(donor.id, reply)).unwrap();
        assert_eq!(service.print_render_blocking(target_print.token, 0, 64, 64).unwrap().bgra, original_print);
        assert_eq!(service.print_render_blocking(donor_print.token, 0, 64, 64).unwrap().bgra, original_print);
        assert!(call(&service, |reply| Request::Text(accepted_id, 5, 0, reply)).unwrap().contains("Page 6 of 6"));
        assert!(!call(&service, |reply| Request::Render(accepted_id, 5, 64, reply)).unwrap().is_empty());
    }
    #[test]
    fn insert_all_ordered_fixture_pairs_validate_every_output_page_and_preserve_source_history() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll"));
        let folder = tempfile::tempdir().unwrap();
        let fixtures = [("resources/welcome.pdf", 6usize), ("../test-corpus/synthetic-scan-98.pdf", 98), ("../test-corpus/synthetic-text-1500.pdf", 1500)];
        for (target_index, (target_fixture, target_count)) in fixtures.iter().enumerate() {
            for (donor_index, (donor_fixture, donor_count)) in fixtures.iter().enumerate() {
                if target_index == donor_index { continue; }
                let target_path = root.join(target_fixture); let donor_path = root.join(donor_fixture);
                let target_bytes = std::fs::read(&target_path).unwrap(); let donor_bytes = std::fs::read(&donor_path).unwrap();
                let mut target = call(&service, |reply| Request::Open(target_path.clone(), reply)).unwrap();
                let _target_cleanup = TestDocument { service: service.clone(), id: target.id };
                let mut donor = call(&service, |reply| Request::Open(donor_path.clone(), reply)).unwrap();
                let _donor_cleanup = TestDocument { service: service.clone(), id: donor.id };
                for edit in [PageEdit::Move { from: 0, to: 2 }, PageEdit::Rotate { pages: vec![0], clockwise: true }, PageEdit::Delete { pages: vec![1] }] { target = call(&service, |reply| Request::Edit(target.id, edit, reply)).unwrap(); }
                for edit in [PageEdit::Delete { pages: vec![1, 3] }, PageEdit::Rotate { pages: vec![0], clockwise: false }, PageEdit::Move { from: 0, to: 1 }] { donor = call(&service, |reply| Request::Edit(donor.id, edit, reply)).unwrap(); }
                let at = target.pages.len() / 2; let path = folder.path().join(format!("insert-{target_count}-{donor_count}.pdf"));
                call(&service, |reply| Request::CheckInsertion(combine_source(&target), combine_source(&donor), at, reply)).unwrap();
                let started = std::time::Instant::now();
                let inserted = call(&service, |reply| Request::InsertPages(combine_source(&target), combine_source(&donor), at, path.clone(), reply)).unwrap().document;
                let _inserted_cleanup = TestDocument { service: service.clone(), id: inserted.id };
                println!("insert {target_count}+{donor_count} at={at}: {} current pages, validation/write {:?}", inserted.pages.len(), started.elapsed());
                assert_ne!(inserted.id, target.id); assert_ne!(inserted.id, donor.id); assert_eq!(inserted.pages.len(), target_count + donor_count - 3);
                assert_eq!(inserted.revision, 0); assert!(!inserted.dirty && !inserted.can_undo && !inserted.can_redo);
                let reopened = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); let _reopened_cleanup = TestDocument { service: service.clone(), id: reopened.id };
                for position in 0..inserted.pages.len() {
                    let (source, local, count, is_target) = if position < at { (&target, position, target_count, true) }
                        else if position < at + donor.pages.len() { (&donor, position - at, donor_count, false) }
                        else { (&target, position - donor.pages.len(), target_count, true) };
                    let original = if is_target { match local { 0 => 1, 1 => 0, other => other + 1 } } else { match local { 0 => 2, 1 => 0, other => other + 2 } };
                    let footer = format!("Page {} of {count}", original + 1);
                    assert_eq!((inserted.pages[position].width, inserted.pages[position].height), (source.pages[local].width, source.pages[local].height));
                    let expected = image::load_from_memory(&call(&service, |reply| Request::Render(source.id, local as u16, 64, reply)).unwrap()).unwrap().into_rgba8();
                    for id in [inserted.id, reopened.id] {
                        assert!(call(&service, |reply| Request::Text(id, position as u16, 0, reply)).unwrap().contains(&footer), "Inserted source/order differs at output page {position}");
                        let actual = image::load_from_memory(&call(&service, |reply| Request::Render(id, position as u16, 64, reply)).unwrap()).unwrap().into_rgba8();
                        assert_eq!(actual, expected, "Inserted/reopened render differs at output page {position}");
                    }
                }
                assert_eq!(std::fs::read(target_path).unwrap(), target_bytes); assert_eq!(std::fs::read(donor_path).unwrap(), donor_bytes);
                for source in [&target, &donor] {
                    assert_eq!(call(&service, |reply| Request::Properties(source.id, source.revision, reply)).unwrap().page_count, source.pages.len());
                    for undo in 0..3 {
                        let restored = call(&service, |reply| Request::Edit(source.id, PageEdit::Undo, reply)).unwrap();
                        assert_eq!(restored.revision, source.revision + undo + 1); assert_eq!(restored.dirty, undo != 2); assert!(restored.can_redo);
                    }
                }
            }
        }
    }
    #[test]
    fn insert_duplicate_invalid_stale_and_closed_after_preflight_never_write_or_mutate_sources() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let mut target = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap(); let _target_cleanup = TestDocument { service: service.clone(), id: target.id };
        let mut donor = call(&service, |reply| Request::Open(root.join("resources/welcome.pdf"), reply)).unwrap(); let _donor_cleanup = TestDocument { service: service.clone(), id: donor.id };
        let path = folder.path().join("must-not-insert.pdf");
        let insert = |target, donor, at| call(&service, |reply| Request::InsertPages(target, donor, at, path.clone(), reply));
        assert!(insert(combine_source(&target), combine_source(&target), 0).err().unwrap().contains("different"));
        assert!(insert(combine_source(&target), combine_source(&donor), 7).err().unwrap().contains("position"));
        assert!(serde_json::from_value::<usize>(serde_json::json!(0.5)).is_err()); assert!(serde_json::from_value::<usize>(serde_json::json!(-1)).is_err());
        call(&service, |reply| Request::CheckInsertion(combine_source(&target), combine_source(&donor), 3, reply)).unwrap();
        let old_donor = combine_source(&donor); donor = call(&service, |reply| Request::Edit(donor.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap();
        assert!(insert(combine_source(&target), old_donor, 3).err().unwrap().contains("changed"));
        call(&service, |reply| Request::CheckInsertion(combine_source(&target), combine_source(&donor), 3, reply)).unwrap();
        let old_target = combine_source(&target); target = call(&service, |reply| Request::Edit(target.id, PageEdit::Rotate { pages: vec![0], clockwise: true }, reply)).unwrap();
        assert!(insert(old_target, combine_source(&donor), 3).err().unwrap().contains("changed"));
        call(&service, |reply| Request::CheckInsertion(combine_source(&target), combine_source(&donor), 3, reply)).unwrap();
        call(&service, |reply| Request::Close(donor.id, reply)).unwrap(); assert!(insert(combine_source(&target), combine_source(&donor), 3).err().unwrap().contains("Donor document is closed"));
        call(&service, |reply| Request::Close(target.id, reply)).unwrap(); assert!(insert(combine_source(&target), combine_source(&donor), 3).err().unwrap().contains("Target document is closed"));
        assert!(!path.exists()); assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 0);
    }
    #[test]
    fn insert_cropped_inherited_original_and_edited_rotations_keep_independent_ink_and_visible_tokens() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        for original in [0, 90, 180, 270] {
            for edited in 0..4 {
                let mut sources = Vec::new(); let mut expected = Vec::new(); let mut cleanup = Vec::new();
                for (index, label) in ["Target", "Donor"].into_iter().enumerate() {
                    let rotation = if index == 0 { original } else { (360 - original) % 360 };
                    let bytes = crate::text_geometry::tests::fixture(rotation, [1.0, 0.0, 0.0, 1.0, 100.0, 300.0], &format!("{label}OutsideTop\n\n{label}Header\n{label}Body\n\n{label}OutsideBottom"));
                    let mut pdf = lopdf::Document::load_mem(&bytes).unwrap(); let page = pdf.get_pages()[&1];
                    pdf.get_dictionary_mut(page).unwrap().set("MediaBox", vec![20.into(), 30.into(), 420.into(), 430.into()]);
                    let parent = pdf.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
                    for key in [b"MediaBox".as_slice(), b"CropBox", b"Rotate", b"Resources"] { let value = pdf.get_dictionary_mut(page).unwrap().remove(key).unwrap(); pdf.get_dictionary_mut(parent).unwrap().set(key, value); }
                    let path = folder.path().join(format!("insert-source-{original}-{edited}-{index}.pdf")); pdf.save(&path).unwrap(); let source_bytes = std::fs::read(&path).unwrap();
                    let mut info = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); cleanup.push(TestDocument { service: service.clone(), id: info.id });
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
                    info = call(&service, |reply| Request::Crop(info.id, 0, info.revision, rect, reply)).unwrap(); sources.push((info, path, source_bytes));
                }
                let path = folder.path().join(format!("insert-cropped-{original}-{edited}.pdf"));
                let inserted = call(&service, |reply| Request::InsertPages(combine_source(&sources[0].0), combine_source(&sources[1].0), 0, path.clone(), reply)).unwrap().document;
                cleanup.push(TestDocument { service: service.clone(), id: inserted.id });
                let reopened = call(&service, |reply| Request::Open(path.clone(), reply)).unwrap(); cleanup.push(TestDocument { service: service.clone(), id: reopened.id });
                let output = lopdf::Document::load(&path).unwrap();
                for (position, (source_index, label)) in [(1usize, "Donor"), (0, "Target")].into_iter().enumerate() {
                    let (expected, width, turns) = &expected[source_index]; let dimensions = if turns % 2 == 0 { (240.0, 110.0) } else { (110.0, 240.0) };
                    assert_eq!((inserted.pages[position].width, inserted.pages[position].height), dimensions);
                    for id in [inserted.id, reopened.id] {
                        let actual = image::load_from_memory(&call(&service, |reply| Request::Render(id, position as u16, *width, reply)).unwrap()).unwrap().into_rgb8(); assert_same_ink(&actual, expected);
                        let text = call(&service, |reply| Request::Text(id, position as u16, 0, reply)).unwrap();
                        let geometry = call(&service, |reply| Request::TextGeometry(id, position as u16, 0, reply)).unwrap(); assert_eq!(geometry.status, "ok");
                        let copied = geometry.characters.iter().map(|character| character.text.as_str()).collect::<String>();
                        for token in [format!("{label}Header"), format!("{label}Body")] { assert!(text.contains(&token), "{original}/{edited}: {text}"); assert!(copied.contains(&token), "{original}/{edited}: {copied}"); }
                        for token in [format!("{label}OutsideTop"), format!("{label}OutsideBottom")] { assert!(!text.contains(&token) && !copied.contains(&token)); }
                    }
                    assert!(String::from_utf8_lossy(&output.get_page_content(output.get_pages()[&(position as u32 + 1)])).contains(&format!("{label}OutsideBottom")), "Insertion must preserve underlying crop-hidden content");
                    let (source, source_path, source_bytes) = &sources[source_index]; assert_eq!(std::fs::read(source_path).unwrap(), *source_bytes);
                    let undo = call(&service, |reply| Request::Edit(source.id, PageEdit::Undo, reply)).unwrap(); assert_eq!(undo.revision, source.revision + 1); assert!(undo.can_redo);
                }
            }
        }
    }
    #[test]
    fn insert_unread_and_preclosed_replies_preserve_publication_and_accepted_copy_owns_its_source() {
        let _print_lock = print_test_lock();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")); let service = PdfService::start(root.join("resources/pdfium/bin/pdfium.dll")); let folder = tempfile::tempdir().unwrap();
        let source = std::fs::read(root.join("resources/welcome.pdf")).unwrap(); let target_path = folder.path().join("target.pdf"); let donor_path = folder.path().join("donor.pdf");
        std::fs::write(&target_path, &source).unwrap(); std::fs::write(&donor_path, &source).unwrap();
        let target = call(&service, |reply| Request::Open(target_path.clone(), reply)).unwrap(); let _target_cleanup = TestDocument { service: service.clone(), id: target.id };
        let donor = call(&service, |reply| Request::Open(donor_path.clone(), reply)).unwrap(); let _donor_cleanup = TestDocument { service: service.clone(), id: donor.id };
        let target_print = print_snapshot(&service, target.id, target.revision); let donor_print = print_snapshot(&service, donor.id, donor.revision);
        let original_print = service.print_render_blocking(target_print.token, 0, 64, 64).unwrap().bgra;
        let canceled = folder.path().join("canceled.pdf"); let (tx, rx) = oneshot::channel(); drop(rx);
        service.sender.send(Request::InsertPages(combine_source(&target), combine_source(&donor), 0, canceled.clone(), tx)).unwrap();
        call(&service, |reply| Request::Properties(target.id, target.revision, reply)).unwrap(); assert!(!canceled.exists());
        let output = folder.path().join("unread.pdf"); let _unread_cleanup = UnreadReplyCleanup { service: service.clone(), paths: vec![output.clone()] };
        let retained = unread_document_reply(&service, &output, |reply| Request::InsertPages(combine_source(&target), combine_source(&donor), 2, output.clone(), reply));
        assert!(retained.is_empty(), "Unread insertion result must release only the new session"); assert!(output.exists());
        let reopened = call(&service, |reply| Request::Open(output.clone(), reply)).unwrap(); let _reopened_cleanup = TestDocument { service: service.clone(), id: reopened.id }; assert_eq!(reopened.pages.len(), 12);
        let accepted_path = folder.path().join("accepted.pdf");
        let accepted = tauri::async_runtime::block_on(service.insert_pages_copy(combine_source(&target), combine_source(&donor), 6, accepted_path)).unwrap(); let accepted_id = accepted.document.id; drop(accepted);
        let _accepted_cleanup = TestDocument { service: service.clone(), id: accepted_id };
        assert_eq!(call(&service, |reply| Request::Properties(accepted_id, 0, reply)).unwrap().page_count, 12);
        assert_eq!(std::fs::read(target_path).unwrap(), source); assert_eq!(std::fs::read(donor_path).unwrap(), source);
        call(&service, |reply| Request::Close(target.id, reply)).unwrap(); call(&service, |reply| Request::Close(donor.id, reply)).unwrap();
        assert_eq!(service.print_render_blocking(target_print.token, 0, 64, 64).unwrap().bgra, original_print);
        assert_eq!(service.print_render_blocking(donor_print.token, 0, 64, 64).unwrap().bgra, original_print);
        assert!(call(&service, |reply| Request::Text(accepted_id, 11, 0, reply)).unwrap().contains("Page 6 of 6"));
        assert!(!call(&service, |reply| Request::Render(accepted_id, 11, 64, reply)).unwrap().is_empty());
    }
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
        let _password_lock = password_test_lock();
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
            let result = rx.blocking_recv().unwrap().unwrap().accept();
            let info = if user_password.is_empty() {
                match result { OpenResult::Opened { document } => document, _ => panic!("Empty password should open") }
            } else {
                let request_id = match result { OpenResult::PasswordRequired { request_id, incorrect, .. } => { assert!(!incorrect); request_id }, _ => panic!("Expected password challenge") };
                for wrong in ["wrong", ""] {
                    let (tx, rx) = oneshot::channel(); service.sender.send(Request::Unlock(request_id, wrong.into(), tx)).unwrap();
                    assert!(matches!(rx.blocking_recv().unwrap().unwrap().accept(), OpenResult::PasswordRequired { .. }));
                }
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Unlock(request_id, user_password.into(), tx)).unwrap();
                let opened = match rx.blocking_recv().unwrap().unwrap().accept() { OpenResult::Opened { document } => document, _ => panic!("Correct password failed") };
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Unlock(request_id, user_password.into(), tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::BeginOpen(path.clone(), tx)).unwrap();
                let cancelled = match rx.blocking_recv().unwrap().unwrap().accept() { OpenResult::PasswordRequired { request_id, .. } => request_id, _ => panic!("Expected challenge") };
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::CancelPassword(cancelled, tx)).unwrap(); rx.blocking_recv().unwrap().unwrap();
                let (tx, rx) = oneshot::channel(); service.sender.send(Request::Unlock(cancelled, user_password.into(), tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err());
                opened
            };
            assert_eq!(info.pages.len(), 6);
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::Properties(info.id, 0, tx)).unwrap(); let properties = rx.blocking_recv().unwrap().unwrap();
            assert_ne!(properties.security.encrypted, Some(false), "Encrypted v{version}-{kind} must never be reported unencrypted");
            assert_eq!(properties.page_count, 6); assert_eq!(properties.source_size_bytes, source.len());
            let (tx, rx) = oneshot::channel(); service.sender.send(Request::BeginPrint(info.id, 0, tx)).unwrap(); assert!(rx.blocking_recv().unwrap().is_err(), "Encrypted v{version}-{kind} must not print");
            let comments = call(&service, |reply| Request::Comments(info.id, 0, reply)).unwrap(); assert_eq!(comments.status, "unsupported"); assert!(comments.notes.is_empty());
            assert!(call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::Create(0, CropRect { x: 0.1, y: 0.1, width: 0.1, height: 0.1 }, "Blocked encrypted note".into()), reply)).is_err(), "Encrypted v{version}-{kind} must not create notes");
            assert_eq!(call(&service, |reply| Request::Annotations(info.id, 0, reply)).unwrap().status, "unsupported"); assert!(call(&service, |reply| Request::Comment(info.id, 0, CommentMutation::CreateHighlight(0, CropRect { x: 0.1, y: 0.1, width: 0.1, height: 0.1 }, None), reply)).is_err(), "Encrypted v{version}-{kind} must not create highlights");
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
            for (target, donor) in [(&ordinary, &info), (&info, &ordinary)] {
                assert!(call(&service, |reply| Request::CheckInsertion(combine_source(target), combine_source(donor), 0, reply)).is_err(), "Encrypted v{version}-{kind} must not insert");
                assert!(call(&service, |reply| Request::InsertPages(combine_source(target), combine_source(donor), 0, output.clone(), reply)).is_err()); assert!(!output.exists());
                assert!(call(&service, |reply| Request::CheckReplacement(combine_source(target), combine_source(donor), 0, target.pages.len(), reply)).is_err(), "Encrypted v{version}-{kind} must not replace even the entire target");
                assert!(call(&service, |reply| Request::ReplacePages(combine_source(target), combine_source(donor), 0, target.pages.len(), output.clone(), reply)).is_err()); assert!(!output.exists());
            }
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
