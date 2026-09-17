use lopdf::{dictionary, Document, Object, ObjectId};
use serde::Deserialize;
use std::{collections::{HashSet, VecDeque}, io::Write, path::Path};

const HISTORY_BUDGET: usize = 32 * 1024 * 1024;

fn snapshot_bytes(snapshot: &Vec<PageSpec>) -> usize {
    std::mem::size_of::<Vec<PageSpec>>() + snapshot.capacity() * std::mem::size_of::<PageSpec>()
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageSpec {
    pub source: usize,
    pub turns: i32,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PageEdit {
    Rotate { pages: Vec<usize>, clockwise: bool },
    Delete { pages: Vec<usize> },
    Move { from: usize, to: usize },
    Undo,
    Redo,
}

pub struct EditSession {
    pub source: Vec<u8>,
    source_page_count: usize,
    pub plan: Vec<PageSpec>,
    undo: VecDeque<Vec<PageSpec>>,
    redo: VecDeque<Vec<PageSpec>>,
    history_bytes: usize,
    history_budget: usize,
    saved: Vec<PageSpec>,
    pub revision: u64,
}

impl EditSession {
    pub fn new(source: Vec<u8>, count: usize) -> Self {
        let plan: Vec<_> = (0..count).map(|source| PageSpec { source, turns: 0 }).collect();
        Self { source, source_page_count: count, saved: plan.clone(), plan, undo: VecDeque::new(), redo: VecDeque::new(), history_bytes: 0, history_budget: HISTORY_BUDGET, revision: 0 }
    }
    pub fn dirty(&self) -> bool { self.plan != self.saved }
    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }
    pub fn mark_saved(&mut self) { self.saved = self.plan.clone(); }
    fn trim_history(&mut self) {
        while self.history_bytes > self.history_budget {
            let snapshot = self.undo.pop_front().or_else(|| self.redo.pop_front()).expect("History byte count matches stored snapshots");
            self.history_bytes -= snapshot_bytes(&snapshot);
        }
    }
    fn load_source(&self) -> Result<Document, String> {
        let document = Document::load_mem(&self.source).map_err(|e| e.to_string())?;
        if document.is_encrypted() || document.encryption_state.is_some() { return Err("Encrypted PDFs cannot be edited in this build.".into()); }
        let pages = document.get_pages();
        if pages.len() != self.source_page_count {
            return Err("The PDF engines disagree about the source page count. Editing and export are blocked to preserve the document.".into());
        }
        if pages.values().collect::<HashSet<_>>().len() != pages.len() {
            return Err("The PDF repeats a page object in its page tree. Editing and export are blocked to avoid changing other pages.".into());
        }
        Ok(document)
    }

    pub fn apply(&mut self, edit: PageEdit) -> Result<(), String> {
        match edit {
            PageEdit::Undo => {
                let previous = self.undo.pop_back().ok_or("Nothing to undo")?;
                self.history_bytes -= snapshot_bytes(&previous);
                let current = std::mem::replace(&mut self.plan, previous);
                self.history_bytes += snapshot_bytes(&current);
                self.redo.push_back(current);
            }
            PageEdit::Redo => {
                let next = self.redo.pop_back().ok_or("Nothing to redo")?;
                self.history_bytes -= snapshot_bytes(&next);
                let current = std::mem::replace(&mut self.plan, next);
                self.history_bytes += snapshot_bytes(&current);
                self.undo.push_back(current);
            }
            edit => {
                let mut next = self.plan.clone();
                let structural = !matches!(&edit, PageEdit::Rotate { .. });
                let removal = matches!(&edit, PageEdit::Delete { .. });
                check_supported(&self.load_source()?, structural, removal)?;
                match edit {
                    PageEdit::Rotate { pages, clockwise } => {
                        for index in validate_selection(&pages, next.len())? {
                            next[index].turns = (next[index].turns + if clockwise { 1 } else { 3 }) % 4;
                        }
                    }
                    PageEdit::Delete { pages } => {
                        let selected = validate_selection(&pages, next.len())?;
                        if selected.len() == next.len() { return Err("Keep at least one page in the document.".into()); }
                        next = next.into_iter().enumerate().filter_map(|(i, page)| (!selected.contains(&i)).then_some(page)).collect();
                    }
                    PageEdit::Move { from, to } => {
                        if from >= next.len() || to >= next.len() { return Err("Page position is out of range.".into()); }
                        let page = next.remove(from); next.insert(to, page);
                    }
                    _ => unreachable!(),
                }
                if next == self.plan { return Ok(()); }
                let previous = std::mem::replace(&mut self.plan, next);
                self.history_bytes += snapshot_bytes(&previous);
                self.undo.push_back(previous);
                self.history_bytes -= self.redo.drain(..).map(|snapshot| snapshot_bytes(&snapshot)).sum::<usize>();
            }
        }
        self.trim_history();
        self.revision += 1;
        Ok(())
    }

    pub fn export(&self, selection: Option<&[usize]>) -> Result<Vec<u8>, String> {
        let plan = match selection {
            Some(selection) => {
                let selected = validate_selection(selection, self.plan.len())?;
                self.plan.iter().enumerate().filter_map(|(i, p)| selected.contains(&i).then_some(p.clone())).collect::<Vec<_>>()
            }
            None => self.plan.clone(),
        };
        let mut document = self.load_source()?;
        let original: Vec<_> = document.get_pages().values().copied().collect();
        if plan.iter().any(|p| p.source >= original.len()) { return Err("Source page mapping is invalid.".into()); }
        let structural = plan.len() != original.len() || plan.iter().enumerate().any(|(i, p)| p.source != i);
        check_supported(&document, structural, plan.len() < original.len())?;
        let new_root = structural.then(|| document.new_object_id());
        let mut kids = Vec::new();
        for spec in &plan {
            let id = original[spec.source];
            let rotation = inherited(&document, id, b"Rotate")?.map(|o| o.as_i64().map_err(|e| e.to_string())).transpose()?.unwrap_or(0);
            if rotation % 90 != 0 { return Err("This document has an unsupported page rotation.".into()); }
            let attributes: Vec<_> = if structural {
                [b"Resources".as_slice(), b"MediaBox", b"CropBox"].iter().map(|key| Ok((key.to_vec(), inherited(&document, id, key)?))).collect::<Result<_, String>>()?
            } else { Vec::new() };
            let page = document.get_object_mut(id).map_err(|e| e.to_string())?.as_dict_mut().map_err(|e| e.to_string())?;
            page.set("Rotate", (rotation.rem_euclid(360) + i64::from(spec.turns) * 90).rem_euclid(360));
            if let Some(root) = new_root {
                for (key, value) in attributes { if let Some(value) = value { page.set(key, value); } }
                page.set("Parent", root);
                kids.push(Object::Reference(id));
            }
        }
        if let Some(root) = new_root {
            document.objects.insert(root, Object::Dictionary(dictionary! { "Type" => "Pages", "Count" => plan.len() as i64, "Kids" => kids }));
            document.catalog_mut().map_err(|e| e.to_string())?.set("Pages", root);
            document.prune_objects();
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }
}

fn validate_selection(pages: &[usize], count: usize) -> Result<HashSet<usize>, String> {
    if pages.is_empty() || pages.iter().any(|&i| i >= count) { return Err("Select valid pages first.".into()); }
    Ok(pages.iter().copied().collect())
}

fn inherited(document: &Document, mut id: ObjectId, key: &[u8]) -> Result<Option<Object>, String> {
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(id) { return Err("The page tree contains a cycle.".into()); }
        let dictionary = document.get_dictionary(id).map_err(|e| e.to_string())?;
        if let Ok(value) = dictionary.get(key) {
            return Ok(Some(document.dereference(value).map_err(|e| e.to_string())?.1.clone()));
        }
        match dictionary.get(b"Parent") {
            Ok(parent) => id = parent.as_reference().map_err(|e| e.to_string())?,
            Err(_) => return Ok(None),
        }
    }
}

fn check_supported(document: &Document, structural: bool, removal: bool) -> Result<(), String> {
    if document.is_encrypted() { return Err("Encrypted PDFs cannot be edited in this build.".into()); }
    let catalog = document.catalog().map_err(|e| e.to_string())?;
    if catalog.has(b"Perms") || document.objects.values().any(|object| object.as_dict().is_ok_and(|dict| dict.has(b"ByteRange") || dict.get(b"Type").is_ok_and(|value| value.as_name().is_ok_and(|name| name == b"Sig")))) {
        return Err("Signed or certified PDFs cannot be edited in this build; signatures must remain intact.".into());
    }
    if structural && [b"AcroForm".as_slice(), b"StructTreeRoot", b"PageLabels", b"Threads"].iter().any(|key| catalog.has(key)) {
        return Err("Reordering/extraction/deletion of forms, tagged PDFs, page labels, or article threads is not supported yet. Rotation is available.".into());
    }
    if removal && ([b"Outlines".as_slice(), b"Dests", b"Names", b"OpenAction"].iter().any(|key| catalog.has(key)) || document.get_pages().values().any(|id| document.get_dictionary(*id).is_ok_and(|page| page.has(b"Annots")))) {
        return Err("Deleting/extracting pages with bookmarks, destinations, attachments, actions, or annotations is not supported yet. This avoids losing or breaking their references.".into());
    }
    Ok(())
}

pub fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() { return Err("That file already exists. Choose a new filename; Save a Copy never overwrites an existing file.".into()); }
    if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) { return Err("The output filename must end in .pdf.".into()); }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).ok_or("Choose an output folder.")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary.write_all(bytes).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist_noclobber(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Vec<u8> { std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/welcome.pdf")).unwrap() }
    fn assert_history_budget(session: &EditSession) {
        let allocated = session.undo.iter().chain(session.redo.iter()).map(snapshot_bytes).sum::<usize>();
        assert_eq!(session.history_bytes, allocated);
        assert!(allocated <= session.history_budget);
    }
    #[test]
    fn shared_page_objects_reject_rotation_and_export_without_mutation() {
        let mut document = Document::load_mem(&sample()).unwrap();
        let pages = document.get_pages();
        let root = document.catalog().unwrap().get(b"Pages").unwrap().as_reference().unwrap();
        let mut kids: Vec<_> = pages.values().copied().map(Object::Reference).collect();
        kids[1] = kids[0].clone();
        document.get_object_mut(root).unwrap().as_dict_mut().unwrap().set("Kids", kids);
        let mut source = Vec::new(); document.save_to(&mut source).unwrap();
        assert_eq!(Document::load_mem(&source).unwrap().get_pages().len(), 6);
        let mut session = EditSession::new(source.clone(), 6);
        let original_plan = session.plan.clone();
        let error = session.apply(PageEdit::Rotate { pages: vec![0], clockwise: true }).unwrap_err();
        assert!(error.contains("repeats a page object"));
        assert!(session.export(None).unwrap_err().contains("repeats a page object"));
        assert!(session.export(Some(&[0])).is_err());
        assert_eq!(session.source, source);
        assert_eq!(session.plan, original_plan);
        assert_eq!(session.saved, original_plan);
        assert_eq!(session.revision, 0);
        assert!(!session.can_undo());
        assert!(!session.can_redo());
        assert!(!session.dirty());
        let mut ordinary = EditSession::new(sample(), 6);
        ordinary.apply(PageEdit::Rotate { pages: vec![0], clockwise: true }).unwrap();
        assert!(ordinary.export(None).is_ok());
        assert_eq!(ordinary.plan[0].turns, 1);
        assert_eq!(ordinary.plan[1].turns, 0);
    }
    #[test]
    fn history_evicts_oldest_snapshots_and_preserves_current_source_and_saved_state() {
        let source = sample();
        let mut session = EditSession::new(source.clone(), 6);
        session.history_budget = snapshot_bytes(&session.plan) * 3;
        let saved = session.saved.clone();
        let mut states = vec![session.plan.clone()];
        for _ in 0..17 {
            session.apply(PageEdit::Rotate { pages: vec![0], clockwise: true }).unwrap();
            states.push(session.plan.clone());
            assert_history_budget(&session);
        }
        assert_eq!(session.undo.len(), 3);
        assert_eq!(session.plan, states[17]);
        assert_eq!(session.source, source);
        assert_eq!(session.saved, saved);
        for index in (14..17).rev() {
            session.apply(PageEdit::Undo).unwrap();
            assert_eq!(session.plan, states[index]);
            assert_history_budget(&session);
        }
        assert!(!session.can_undo());
        assert!(session.apply(PageEdit::Undo).is_err());
        for state in states.iter().take(18).skip(15) {
            session.apply(PageEdit::Redo).unwrap();
            assert_eq!(&session.plan, state);
            assert_history_budget(&session);
        }
        assert!(!session.can_redo());
        assert_eq!(session.source, source);
        assert_eq!(session.saved, saved);
    }
    #[test]
    fn bounded_history_branches_clear_redo_and_keep_saved_baseline() {
        let mut session = EditSession::new(sample(), 6);
        session.history_budget = snapshot_bytes(&session.plan) * 2;
        session.apply(PageEdit::Rotate { pages: vec![0], clockwise: true }).unwrap();
        session.mark_saved();
        let saved = session.plan.clone();
        session.apply(PageEdit::Delete { pages: vec![1, 2, 3] }).unwrap();
        session.apply(PageEdit::Undo).unwrap();
        assert!(!session.dirty());
        assert!(session.can_redo());
        assert_history_budget(&session);
        session.apply(PageEdit::Rotate { pages: vec![1], clockwise: true }).unwrap();
        assert!(!session.can_redo());
        assert!(session.dirty());
        assert_eq!(session.saved, saved);
        assert_history_budget(&session);
        session.apply(PageEdit::Undo).unwrap();
        assert_eq!(session.plan, saved);
        assert!(!session.dirty());
        assert_history_budget(&session);
    }
    #[test]
    fn oversized_snapshot_is_dropped_without_rejecting_or_reverting_edit() {
        let source = sample();
        let mut session = EditSession::new(source.clone(), 6);
        session.history_budget = snapshot_bytes(&session.plan) - 1;
        session.apply(PageEdit::Rotate { pages: vec![0], clockwise: true }).unwrap();
        assert_eq!(session.plan[0].turns, 1);
        assert!(session.dirty());
        assert!(!session.can_undo());
        assert!(!session.can_redo());
        assert_eq!(session.source, source);
        assert_eq!(session.saved[0].turns, 0);
        assert_history_budget(&session);
    }
    #[test]
    fn history_accounts_for_reserved_capacity_and_both_stacks() {
        let mut session = EditSession::new(sample(), 6);
        let mut roomy = Vec::with_capacity(100);
        roomy.push(PageSpec { source: 0, turns: 0 });
        let roomy_bytes = snapshot_bytes(&roomy);
        assert!(roomy_bytes > std::mem::size_of::<PageSpec>() * roomy.len());
        let small = session.plan.clone();
        session.history_budget = snapshot_bytes(&small);
        session.undo.push_back(roomy);
        session.redo.push_back(small);
        session.history_bytes = roomy_bytes + session.history_budget;
        let current = session.plan.clone();
        session.trim_history();
        assert!(!session.can_undo());
        assert!(session.can_redo());
        assert_eq!(session.plan, current);
        assert_history_budget(&session);
        session.history_budget = 0;
        session.trim_history();
        assert!(!session.can_redo());
        assert_eq!(session.plan, current);
        assert_history_budget(&session);
    }
    #[test]
    fn undo_redo_and_branch_preserve_source() {
        let bytes = sample(); let mut session = EditSession::new(bytes.clone(), 6);
        session.apply(PageEdit::Rotate { pages: vec![0, 0], clockwise: true }).unwrap();
        assert_eq!(session.plan[0].turns, 1);
        session.apply(PageEdit::Delete { pages: vec![1, 3] }).unwrap();
        assert_eq!(session.plan.iter().map(|p| p.source).collect::<Vec<_>>(), vec![0, 2, 4, 5]);
        session.apply(PageEdit::Undo).unwrap(); assert_eq!(session.plan.len(), 6);
        session.apply(PageEdit::Redo).unwrap(); assert_eq!(session.plan.len(), 4);
        session.apply(PageEdit::Undo).unwrap();
        session.apply(PageEdit::Move { from: 5, to: 0 }).unwrap();
        assert!(!session.can_redo()); assert_eq!(session.source, bytes);
        assert!(session.apply(PageEdit::Delete { pages: (0..6).collect() }).is_err());
        assert!(session.apply(PageEdit::Rotate { pages: vec![99], clockwise: true }).is_err());
    }
    #[test]
    fn output_preserves_order_rotation_text_and_safe_save() {
        let mut session = EditSession::new(sample(), 6);
        session.apply(PageEdit::Rotate { pages: vec![0], clockwise: false }).unwrap();
        session.apply(PageEdit::Move { from: 5, to: 0 }).unwrap();
        let bytes = session.export(Some(&[0, 1])).unwrap();
        let document = Document::load_mem(&bytes).unwrap();
        assert_eq!(document.get_pages().len(), 2);
        assert!(document.extract_text(&[1]).unwrap().contains("Ready for a real document"));
        assert!(document.extract_text(&[2]).unwrap().contains("A place for your PDFs"));
        let second = document.get_pages()[&2];
        assert_eq!(inherited(&document, second, b"Rotate").unwrap().unwrap().as_i64().unwrap(), 270);
        let folder = tempfile::tempdir().unwrap(); let path = folder.path().join("copy.pdf");
        write_new_file(&path, &bytes).unwrap(); assert!(write_new_file(&path, b"overwrite").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn inherited_page_attributes_survive_reordering() {
        let mut document = Document::load_mem(&sample()).unwrap();
        let root = document.catalog().unwrap().get(b"Pages").unwrap().as_reference().unwrap();
        let first = document.get_pages()[&1];
        let resources = document.get_dictionary(first).unwrap().get(b"Resources").unwrap().clone();
        for id in document.get_pages().values() {
            let page = document.get_object_mut(*id).unwrap().as_dict_mut().unwrap();
            page.remove(b"Resources"); page.remove(b"MediaBox");
        }
        let parent = document.get_object_mut(root).unwrap().as_dict_mut().unwrap();
        parent.set("Resources", resources); parent.set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]); parent.set("Rotate", 90);
        let mut bytes=Vec::new(); document.save_to(&mut bytes).unwrap();
        let mut session = EditSession::new(bytes, 6); session.apply(PageEdit::Move { from: 0, to: 5 }).unwrap();
        let output = Document::load_mem(&session.export(None).unwrap()).unwrap();
        for id in output.get_pages().values() {
            assert!(output.get_dictionary(*id).unwrap().has(b"Resources"));
            assert_eq!(inherited(&output, *id, b"Rotate").unwrap().unwrap().as_i64().unwrap(), 90);
        }
    }
    #[test]
    fn parser_page_count_disagreement_blocks_edits_and_export() {
        for count in [5, 7] {
            let mut session = EditSession::new(sample(), count);
            let original = session.plan.clone();
            assert!(session.apply(PageEdit::Rotate { pages: vec![0], clockwise: true }).unwrap_err().contains("disagree"));
            assert!(session.export(None).unwrap_err().contains("disagree"));
            assert!(session.export(Some(&[0])).unwrap_err().contains("disagree"));
            assert_eq!(session.plan, original); assert_eq!(session.revision, 0); assert!(!session.can_undo());
        }
        let mut valid = EditSession::new(sample(), 6);
        valid.apply(PageEdit::Delete { pages: vec![0] }).unwrap();
        assert_eq!(Document::load_mem(&valid.export(None).unwrap()).unwrap().get_pages().len(), 5);
    }
    #[test]
    fn extreme_rotations_export_without_overflow() {
        for rotation in [i64::MAX / 90 * 90, i64::MIN / 90 * 90] {
            let mut document = Document::load_mem(&sample()).unwrap();
            let first = document.get_pages()[&1];
            document.get_object_mut(first).unwrap().as_dict_mut().unwrap().set("Rotate", rotation);
            let mut source = Vec::new(); document.save_to(&mut source).unwrap();
            let mut session = EditSession::new(source, 6);
            session.apply(PageEdit::Rotate { pages: vec![0], clockwise: false }).unwrap();
            let output = Document::load_mem(&session.export(None).unwrap()).unwrap();
            let actual = inherited(&output, output.get_pages()[&1], b"Rotate").unwrap().unwrap().as_i64().unwrap();
            assert_eq!(actual, (rotation.rem_euclid(360) + 270) % 360);
        }
    }
    #[test]
    fn unsupported_structures_fail_before_mutation() {
        let mut document = Document::load_mem(&sample()).unwrap();
        document.catalog_mut().unwrap().set("AcroForm", dictionary!{});
        let mut bytes=Vec::new(); document.save_to(&mut bytes).unwrap();
        let mut session=EditSession::new(bytes, 6);
        assert!(session.apply(PageEdit::Delete { pages: vec![0] }).is_err()); assert_eq!(session.plan.len(),6);
        session.apply(PageEdit::Rotate { pages: vec![0], clockwise: true }).unwrap();
        let mut document=Document::load_mem(&sample()).unwrap(); document.catalog_mut().unwrap().set("Perms", dictionary!{});
        let mut bytes=Vec::new(); document.save_to(&mut bytes).unwrap();
        assert!(EditSession::new(bytes,6).export(None).is_err());
    }
}
