use crate::editor::{CropBox, PageSpec};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use serde::Serialize;
use std::collections::HashSet;

pub const MAX_NOTES: usize = 1_000;
pub const MAX_TEXT_BYTES: usize = 8 * 1024;
pub const MAX_TOTAL_BYTES: usize = 1024 * 1024;
const PREFIX: &str = "pdf-workstation-note-";

#[derive(Clone, Debug, PartialEq)]
pub struct Note { pub id: String, pub rect: CropBox, pub contents: String }
#[derive(Clone, Debug, Serialize)]
pub struct DisplayRect { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
#[derive(Serialize)]
pub struct CommentInfo { pub id: String, pub page: usize, pub rect: Option<DisplayRect>, pub contents: String }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentList { pub document_id: u64, pub revision: u64, pub status: &'static str, pub reason: Option<String>, pub notes: Vec<CommentInfo> }

pub fn validate_text(contents: &str) -> Result<(), String> {
    if contents.trim().is_empty() { return Err("Enter some text for the note.".into()); }
    if contents.contains('\0') { return Err("Notes cannot contain a null character.".into()); }
    if contents.len() > MAX_TEXT_BYTES { return Err("Each note is limited to 8 KiB of UTF-8 text.".into()); }
    Ok(())
}
pub fn validate_plan(plan: &[PageSpec]) -> Result<(), String> {
    let mut count = 0usize; let mut total = 0usize; let mut ids = HashSet::new();
    for note in plan.iter().flat_map(|page| &page.notes) {
        validate_text(&note.contents)?; number(&note.id)?;
        if !ids.insert(&note.id) { return Err("The PDF contains duplicate note IDs. Commenting is unavailable.".into()); }
        count += 1; total += note.contents.len();
    }
    if count > MAX_NOTES { return Err("This document is limited to 1,000 notes.".into()); }
    if total > MAX_TOTAL_BYTES { return Err("This document is limited to 1 MiB of total note text.".into()); }
    Ok(())
}
pub fn number(id: &str) -> Result<u64, String> {
    let suffix = id.strip_prefix(PREFIX).ok_or("The PDF has a foreign annotation. Commenting is unavailable.")?;
    let value = suffix.parse::<u64>().map_err(|_| "The PDF has an invalid note ID.")?;
    if value == 0 || suffix != value.to_string() { return Err("The PDF has an invalid note ID.".into()); }
    Ok(value)
}
pub fn id(value: u64) -> String { format!("{PREFIX}{value}") }
fn keys(value: &Dictionary, allowed: &[&[u8]]) -> Result<(), String> {
    if value.iter().any(|(key, _)| !allowed.contains(&key.as_slice())) { return Err("The PDF has an unknown annotation or appearance feature. Commenting is unavailable.".into()); }
    Ok(())
}
fn name(value: &Dictionary, key: &[u8], expected: &[u8]) -> Result<(), String> {
    if value.get(key).and_then(Object::as_name).ok() != Some(expected) { return Err("The PDF has an unsupported annotation schema. Commenting is unavailable.".into()); }
    Ok(())
}
fn values(value: &Object) -> Result<Vec<f32>, String> {
    value.as_array().map_err(|error| error.to_string())?.iter().map(|value| value.as_float().map_err(|error| error.to_string())).collect()
}
fn rect(value: &Object) -> Result<CropBox, String> {
    let value = values(value)?;
    if value.len() != 4 || !value.iter().all(|value| value.is_finite()) || !(value[2] - value[0]).is_finite() || !(value[3] - value[1]).is_finite() || value[2] - value[0] < 1.0 || value[3] - value[1] < 1.0 { return Err("The PDF has invalid note bounds. Commenting is unavailable.".into()); }
    Ok(CropBox { left: value[0], bottom: value[1], right: value[2], top: value[3] })
}
fn media_box(document: &Document, mut page: ObjectId) -> Result<CropBox, String> {
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(page) { return Err("The PDF has a cyclic page boundary inheritance.".into()); }
        let dictionary = document.get_dictionary(page).map_err(|error| error.to_string())?;
        if let Ok(value) = dictionary.get(b"MediaBox") {
            let value = document.dereference(value).map_err(|error| error.to_string())?.1.as_array().map_err(|error| error.to_string())?;
            let values = value.iter().map(|value| document.dereference(value).map(|(_, value)| value.clone()).map_err(|error| error.to_string())).collect::<Result<Vec<_>, _>>()?;
            return rect(&Object::Array(values));
        }
        page = dictionary.get(b"Parent").and_then(Object::as_reference).map_err(|_| "The note page has no supported MediaBox.")?;
    }
}
fn appearance(bounds: CropBox) -> Vec<u8> {
    let (w, h) = (bounds.right - bounds.left, bounds.top - bounds.bottom);
    format!("q 1 0.85 0 rg 0 0 {w:.6} {h:.6} re f 0.25 0.2 0 RG 0.25 w {x:.6} {a:.6} m {r:.6} {a:.6} l {x:.6} {b:.6} m {r:.6} {b:.6} l S Q\n", x=w*0.2, r=w*0.8, a=h*0.65, b=h*0.4).into_bytes()
}
fn appearance_dict(bounds: CropBox) -> Dictionary {
    dictionary! { "Type" => "XObject", "Subtype" => "Form", "FormType" => 1,
        "BBox" => vec![0.into(), 0.into(), Object::Real(bounds.right - bounds.left), Object::Real(bounds.top - bounds.bottom)], "Resources" => dictionary! {} }
}
fn decode_text(value: &Object) -> Result<String, String> {
    let bytes = value.as_str().map_err(|error| error.to_string())?;
    if bytes.len() > MAX_TEXT_BYTES * 2 + 2 { return Err("Each note is limited to 8 KiB of UTF-8 text.".into()); }
    if bytes.len() < 2 || bytes[..2] != [0xfe, 0xff] || bytes.len() % 2 != 0 { return Err("The PDF has unsupported note text encoding.".into()); }
    let units = bytes[2..].chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|_| "The PDF has invalid Unicode note text.".into())
}
fn encode_text(value: &str) -> Object {
    let mut bytes = vec![0xfe, 0xff]; for unit in value.encode_utf16() { bytes.extend(unit.to_be_bytes()); }
    Object::String(bytes, StringFormat::Hexadecimal)
}
fn effective_version(document: &Document) -> Result<(u8, u8), String> {
    let parse = |value: &str| match value {
        "1.0" => Ok((1, 0)), "1.1" => Ok((1, 1)), "1.2" => Ok((1, 2)), "1.3" => Ok((1, 3)),
        "1.4" => Ok((1, 4)), "1.5" => Ok((1, 5)), "1.6" => Ok((1, 6)), "1.7" => Ok((1, 7)), "2.0" => Ok((2, 0)),
        _ => Err("Commenting on this PDF version is not supported in this build.".to_owned()),
    };
    let header = parse(&document.version)?;
    let catalog = document.catalog().map_err(|error| error.to_string())?;
    let declared = catalog.get(b"Version").ok().map(|value| {
        let value = document.dereference(value).map_err(|error| error.to_string())?.1.as_name().map_err(|error| error.to_string())?;
        parse(std::str::from_utf8(value).map_err(|error| error.to_string())?)
    }).transpose()?;
    Ok(declared.map_or(header, |declared| declared.max(header)))
}

pub fn read(document: &Document) -> Result<Vec<Vec<Note>>, String> {
    if document.is_encrypted() || document.encryption_state.is_some() { return Err("Encrypted PDFs cannot be commented on in this build.".into()); }
    let catalog = document.catalog().map_err(|error| error.to_string())?;
    effective_version(document)?;
    if catalog.has(b"Perms") || document.objects.values().any(|object| object.as_dict().is_ok_and(|dict| dict.has(b"ByteRange") || dict.get(b"Type").is_ok_and(|value| value.as_name().ok() == Some(b"Sig")))) { return Err("Signed or certified PDFs cannot be commented on in this build.".into()); }
    if [b"AcroForm".as_slice(), b"StructTreeRoot", b"PageLabels", b"Threads"].iter().any(|key| catalog.has(key)) { return Err("Commenting on forms, tagged PDFs, page labels, or article threads is not supported in this build.".into()); }
    let pages = document.get_pages(); let page_ids = pages.values().copied().collect::<HashSet<_>>(); let mut result = Vec::with_capacity(pages.len()); let mut appearance_ids = HashSet::new();
    let mut annotation_ids = HashSet::new(); let mut note_ids = HashSet::new(); let mut count = 0usize; let mut text_bytes = 0usize;
    for page_id in pages.into_values() {
        let page = document.get_dictionary(page_id).map_err(|error| error.to_string())?;
        let mut notes = Vec::new();
        if let Ok(annots) = page.get(b"Annots") {
            let annots = annots.as_array().map_err(|_| "The PDF has unsupported indirect or malformed annotations.")?;
            if annots.is_empty() { return Err("The PDF has an unsupported empty annotation array. Commenting is unavailable.".into()); }
            for annotation in annots {
                let annotation_id = annotation.as_reference().map_err(|_| "The PDF has unsupported inline annotations.")?;
                if !annotation_ids.insert(annotation_id) { return Err("The PDF repeats an annotation object. Commenting is unavailable.".into()); }
                let annotation = document.get_dictionary(annotation_id).map_err(|error| error.to_string())?;
                keys(annotation, &[b"Type", b"Subtype", b"Rect", b"NM", b"Contents", b"Name", b"C", b"Open", b"F", b"P", b"AP"])?;
                name(annotation, b"Type", b"Annot")?; name(annotation, b"Subtype", b"Text")?; name(annotation, b"Name", b"Note")?;
                if annotation.get(b"P").and_then(Object::as_reference).ok() != Some(page_id) || annotation.get(b"F").and_then(Object::as_i64).ok() != Some(4) || annotation.get(b"Open").and_then(Object::as_bool).ok() != Some(false) { return Err("The PDF has unsupported note links, flags, or popup state.".into()); }
                if values(annotation.get(b"C").map_err(|error| error.to_string())?)? != [1.0, 0.85, 0.0] { return Err("The PDF has an altered note color.".into()); }
                let bounds = rect(annotation.get(b"Rect").map_err(|error| error.to_string())?)?;
                bounds.validate_within(media_box(document, page_id)?).map_err(|_| "The PDF has note bounds outside its MediaBox. Commenting is unavailable.")?;
                let id_bytes = annotation.get(b"NM").and_then(Object::as_str).map_err(|error| error.to_string())?;
                if id_bytes.len() > PREFIX.len() + 20 { return Err("The PDF has an invalid note ID.".into()); }
                let note_id = std::str::from_utf8(id_bytes).map_err(|_| "The PDF has an invalid note ID.")?.to_owned(); number(&note_id)?;
                if !note_ids.insert(note_id.clone()) { return Err("The PDF contains duplicate note IDs. Commenting is unavailable.".into()); }
                let contents = decode_text(annotation.get(b"Contents").map_err(|error| error.to_string())?)?; validate_text(&contents)?;
                let ap = annotation.get(b"AP").and_then(Object::as_dict).map_err(|_| "The PDF has unsupported note appearances.")?; keys(ap, &[b"N"])?;
                let appearance_id = ap.get(b"N").and_then(Object::as_reference).map_err(|_| "The PDF has unsupported note appearances.")?; appearance_ids.insert(appearance_id);
                let stream = document.get_object(appearance_id).and_then(Object::as_stream).map_err(|error| error.to_string())?;
                keys(&stream.dict, &[b"Type", b"Subtype", b"FormType", b"BBox", b"Resources", b"Length"])?;
                let expected = appearance_dict(bounds);
                for (key, value) in expected.iter() {
                    let matches = if key == b"BBox" { stream.dict.get(key).ok().and_then(|actual| values(actual).ok()) == values(value).ok() } else { stream.dict.get(key).ok() == Some(value) };
                    if !matches { return Err("The PDF has an altered note appearance schema.".into()); }
                }
                if stream.content != appearance(bounds) || stream.dict.get(b"Length").is_ok_and(|value| value.as_i64().ok() != Some(stream.content.len() as i64)) { return Err("The PDF has an altered note appearance.".into()); }
                count += 1; text_bytes += contents.len();
                if count > MAX_NOTES || text_bytes > MAX_TOTAL_BYTES { return Err("The PDF exceeds the 1,000-note or 1 MiB total note-text limit.".into()); }
                notes.push(Note { id: note_id, rect: bounds, contents });
            }
        }
        result.push(notes);
    }
    let owned = annotation_ids.union(&appearance_ids).copied().collect::<HashSet<_>>();
    for (id, object) in &document.objects {
        if owned.contains(id) { continue; }
        if page_ids.contains(id) {
            for (key, value) in object.as_dict().map_err(|error| error.to_string())?.iter() {
                if key != b"Annots" && references_owned(value, &owned) { return Err("An unrelated PDF object references a note. Commenting is unavailable to preserve it.".into()); }
            }
        } else if references_owned(object, &owned) { return Err("An unrelated PDF object references a note. Commenting is unavailable to preserve it.".into()); }
    }
    if references_owned(&Object::Dictionary(document.trailer.clone()), &owned) { return Err("The PDF trailer references a note. Commenting is unavailable to preserve it.".into()); }
    Ok(result)
}
fn references_owned(value: &Object, owned: &HashSet<ObjectId>) -> bool {
    let mut remaining = vec![value];
    while let Some(value) = remaining.pop() {
        match value {
            Object::Reference(id) if owned.contains(id) => return true,
            Object::Array(values) => remaining.extend(values),
            Object::Dictionary(values) => remaining.extend(values.iter().map(|(_, value)| value)),
            Object::Stream(value) => remaining.extend(value.dict.iter().map(|(_, value)| value)),
            _ => (),
        }
    }
    false
}

pub fn write(document: &mut Document, plan: &[PageSpec]) -> Result<(), String> {
    read(document)?; validate_plan(plan)?;
    let version = effective_version(document)?.max((1, 4)); document.version = format!("{}.{}", version.0, version.1);
    let page_ids = document.get_pages().into_values().collect::<Vec<ObjectId>>();
    let mut old_objects = HashSet::new();
    for page_id in &page_ids {
        if let Ok(annots) = document.get_dictionary(*page_id).map_err(|error| error.to_string())?.get(b"Annots") {
            for annotation in annots.as_array().map_err(|error| error.to_string())? {
                let id = annotation.as_reference().map_err(|error| error.to_string())?;
                let ap = document.get_dictionary(id).map_err(|error| error.to_string())?.get(b"AP").and_then(Object::as_dict).map_err(|error| error.to_string())?.get(b"N").and_then(Object::as_reference).map_err(|error| error.to_string())?;
                old_objects.insert(id); old_objects.insert(ap);
            }
        }
    }
    for id in &page_ids { document.get_dictionary_mut(*id).map_err(|error| error.to_string())?.remove(b"Annots"); }
    for id in old_objects { document.objects.remove(&id); }
    for spec in plan {
        let page_id = *page_ids.get(spec.source).ok_or("Source page mapping is invalid.")?; let mut annots = Vec::new();
        for note in &spec.notes {
            let appearance = document.add_object(Stream::new(appearance_dict(note.rect), appearance(note.rect)));
            let annotation = document.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Text", "Rect" => vec![Object::Real(note.rect.left), Object::Real(note.rect.bottom), Object::Real(note.rect.right), Object::Real(note.rect.top)],
                "NM" => Object::String(note.id.as_bytes().to_vec(), StringFormat::Literal), "Contents" => encode_text(&note.contents), "Name" => "Note", "C" => vec![Object::Real(1.0), Object::Real(0.85), Object::Real(0.0)], "Open" => false, "F" => 4, "P" => page_id, "AP" => dictionary! { "N" => appearance } });
            annots.push(Object::Reference(annotation));
        }
        if !annots.is_empty() { document.get_dictionary_mut(page_id).map_err(|error| error.to_string())?.set("Annots", annots); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{EditSession, PageEdit};
    fn sample() -> Vec<u8> { std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/welcome.pdf")).unwrap() }
    fn annotated() -> (EditSession, Vec<u8>) {
        let source = sample(); let mut session = EditSession::new(source.clone(), 6);
        let plan = session.proposed_comment(Some((0, CropBox { left: 40.0, bottom: 60.0, right: 70.0, top: 90.0 })), None, Some("Unicode é 漢字 😀\nSecond line")).unwrap();
        session.commit_comments(plan); let bytes = session.export(None).unwrap(); (session, bytes)
    }
    #[test]
    fn comments_unicode_owned_schema_reopens_and_foreign_links_or_appearance_refuse() {
        let (session, bytes) = annotated(); let document = Document::load_mem(&bytes).unwrap();
        let notes = read(&document).unwrap(); assert_eq!(notes[0], session.plan[0].notes);
        assert_eq!(session.source, sample());
        let page = document.get_pages()[&1]; let annotation = document.get_dictionary(page).unwrap().get(b"Annots").unwrap().as_array().unwrap()[0].as_reference().unwrap();
        for key in [b"AA".as_slice(), b"A", b"Popup", b"Unknown"] {
            let mut altered = document.clone(); altered.get_dictionary_mut(annotation).unwrap().set(key, Object::Null); assert!(read(&altered).is_err());
        }
        let mut altered = document.clone(); let ap = altered.get_dictionary(annotation).unwrap().get(b"AP").unwrap().as_dict().unwrap().get(b"N").unwrap().as_reference().unwrap();
        altered.get_object_mut(ap).unwrap().as_stream_mut().unwrap().content.push(b' '); assert!(read(&altered).is_err());
        let mut altered = document.clone(); altered.get_dictionary_mut(page).unwrap().get_mut(b"Annots").unwrap().as_array_mut().unwrap().push(Object::Reference(annotation)); assert!(read(&altered).is_err());
        let mut altered = document.clone(); let dependency = altered.add_object(dictionary! { "Unrelated" => annotation }); altered.trailer.set("Info", dependency); assert!(read(&altered).is_err());
        let mut empty = Document::load_mem(&sample()).unwrap(); let page = empty.get_pages()[&1]; empty.get_dictionary_mut(page).unwrap().set("Annots", Vec::<Object>::new());
        let original_page = empty.get_dictionary(page).unwrap().clone(); assert!(read(&empty).unwrap_err().contains("empty annotation array")); assert_eq!(empty.get_dictionary(page).unwrap(), &original_page);
    }
    #[test]
    fn comments_undo_redo_saved_baseline_noops_and_source_remain_consistent() {
        let (mut session, bytes) = annotated(); let note = session.plan[0].notes[0].clone();
        session.mark_saved(); assert!(!session.dirty()); let revision = session.revision;
        let plan = session.proposed_comment(None, Some(&note.id), Some(&note.contents)).unwrap(); session.commit_comments(plan); assert_eq!(session.revision, revision);
        let plan = session.proposed_comment(None, Some(&note.id), Some("Changed")).unwrap(); session.commit_comments(plan); assert!(session.dirty());
        session.apply(PageEdit::Undo).unwrap(); assert!(!session.dirty()); assert_eq!(session.plan[0].notes[0], note);
        session.apply(PageEdit::Redo).unwrap(); assert_eq!(session.plan[0].notes[0].contents, "Changed");
        session.apply(PageEdit::Undo).unwrap(); let plan = session.proposed_comment(None, Some(&note.id), None).unwrap(); session.commit_comments(plan); assert!(!session.can_redo());
        let output = Document::load_mem(&session.export(None).unwrap()).unwrap(); assert!(read(&output).unwrap().iter().all(Vec::is_empty));
        assert!(output.get_pages().values().all(|page| !output.get_dictionary(*page).unwrap().has(b"Annots")));
        let reopened = EditSession::new(bytes, 6); assert_eq!(reopened.plan[0].notes[0], note); assert!(!reopened.dirty());
        assert!(session.proposed_comment(None, Some("missing"), Some("changed")).is_err()); assert_eq!(session.source, sample());
    }
    #[test]
    fn comments_text_count_total_caps_and_duplicate_ids_are_explicit() {
        assert!(validate_text(" \n\t").is_err()); assert!(validate_text("x\0y").is_err());
        assert!(validate_text(&"é".repeat(MAX_TEXT_BYTES / 2)).is_ok()); assert!(validate_text(&"é".repeat(MAX_TEXT_BYTES / 2 + 1)).is_err());
        let mut session = EditSession::new(sample(), 6);
        let bounds = CropBox { left: 40.0, bottom: 60.0, right: 70.0, top: 90.0 };
        session.plan[0].notes = (1..=MAX_NOTES).map(|value| Note { id: id(value as u64), rect: bounds, contents: "x".into() }).collect(); assert!(validate_plan(&session.plan).is_ok());
        session.plan[0].notes.push(Note { id: id(MAX_NOTES as u64 + 1), rect: bounds, contents: "x".into() }); assert!(validate_plan(&session.plan).unwrap_err().contains("1,000"));
        session.plan[0].notes = (1..=128).map(|value| Note { id: id(value), rect: bounds, contents: "x".repeat(MAX_TEXT_BYTES) }).collect(); assert!(validate_plan(&session.plan).is_ok());
        session.plan[0].notes.push(Note { id: id(129), rect: bounds, contents: "x".into() }); assert!(validate_plan(&session.plan).unwrap_err().contains("1 MiB"));
        session.plan[0].notes = vec![Note { id: id(1), rect: bounds, contents: "x".into() }; 2]; assert!(validate_plan(&session.plan).unwrap_err().contains("duplicate"));
        let (_, bytes) = annotated(); let mut pdf = Document::load_mem(&bytes).unwrap(); let page = pdf.get_pages()[&1];
        let original = pdf.get_dictionary(page).unwrap().get(b"Annots").unwrap().as_array().unwrap()[0].as_reference().unwrap();
        let duplicate = pdf.add_object(pdf.get_object(original).unwrap().clone()); pdf.get_dictionary_mut(page).unwrap().get_mut(b"Annots").unwrap().as_array_mut().unwrap().push(Object::Reference(duplicate)); assert!(read(&pdf).unwrap_err().contains("duplicate"));
    }
    #[test]
    fn comments_pdf_version_floor_and_newer_catalog_version_survive_last_note_removal() {
        for (header, declared, expected) in [("1.0", None, "1.4"), ("1.2", Some("1.7"), "1.7"), ("2.0", Some("1.7"), "2.0")] {
            let mut pdf = Document::load_mem(&sample()).unwrap(); pdf.version = header.into(); if let Some(declared) = declared { pdf.catalog_mut().unwrap().set("Version", declared); }
            let original_catalog_version = pdf.catalog().unwrap().get(b"Version").ok().cloned(); let mut source = Vec::new(); pdf.save_to(&mut source).unwrap();
            let mut session = EditSession::new(source.clone(), 6); let plan = session.proposed_comment(Some((0, CropBox { left: 40.0, bottom: 60.0, right: 70.0, top: 90.0 })), None, Some("Note")).unwrap(); session.commit_comments(plan);
            let output = Document::load_mem(&session.export(None).unwrap()).unwrap(); assert_eq!(output.version, expected); assert_eq!(output.catalog().unwrap().get(b"Version").ok().cloned(), original_catalog_version);
            let mut reopened = EditSession::new(session.export(None).unwrap(), 6); let note_id = reopened.plan[0].notes[0].id.clone(); let plan = reopened.proposed_comment(None, Some(&note_id), None).unwrap(); reopened.commit_comments(plan);
            let output = Document::load_mem(&reopened.export(None).unwrap()).unwrap(); assert_eq!(output.version, expected); assert!(read(&output).unwrap().iter().all(Vec::is_empty));
            reopened.mark_saved(); let revision = reopened.revision; assert!(!reopened.dirty()); reopened.apply(PageEdit::Move { from: 0, to: 0 }).unwrap(); assert_eq!(reopened.revision, revision); assert_eq!(Document::load_mem(&reopened.export(None).unwrap()).unwrap().version, expected); assert!(reopened.proposed_comment(None, Some(&note_id), None).is_err()); assert_eq!(reopened.revision, revision); assert!(!reopened.dirty()); assert_eq!(session.source, source);
        }
    }
}
