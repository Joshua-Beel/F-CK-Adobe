use crate::editor::EditSession;
use lopdf::{content::{Content, Operation}, dictionary, Dictionary, Document, LoadOptions, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MAX_FIELDS: usize = 256;
const MAX_SOURCE: usize = 64 * 1024 * 1024;
const MAX_OUTPUT: usize = 256 * 1024 * 1024;
const MAX_VALUE: usize = 4096;
const MAX_TEXT: usize = 1024 * 1024;
const MAX_AP: usize = 64 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFields {
    pub document_id: u64, pub revision: u64, pub status: &'static str, pub reason: Option<String>,
    pub input: &'static str, pub value_byte_limit: usize, pub fields: Vec<FormField>,
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/reportlab-plain-fields.pdf");
    fn session(bytes: Vec<u8>) -> EditSession { EditSession::new(bytes, 1) }
    fn fixture() -> EditSession { session(FIXTURE.to_vec()) }
    fn mutated(change: impl FnOnce(&mut Document)) -> EditSession {
        let mut document = Document::load_mem(FIXTURE).unwrap(); change(&mut document);
        let mut bytes = Vec::new(); document.save_to(&mut bytes).unwrap(); session(bytes)
    }
    fn field_ids(document: &Document) -> Vec<ObjectId> {
        dict(document, document.catalog().unwrap().get(b"AcroForm").unwrap()).unwrap().get(b"Fields").unwrap().as_array().unwrap().iter().map(|value| value.as_reference().unwrap()).collect()
    }
    #[test]
    fn forms_reportlab_values_appearances_and_styles_round_trip_and_clear() {
        let source = fixture(); let parsed = FormDocument::parse(&source).unwrap();
        assert_eq!(parsed.fields.len(), 2); assert_eq!(parsed.fields[0].dto.value, "Original");
        let literal = "Portland (OR) \\ `city`";
        let patches = parsed.fields.iter().enumerate().map(|(index, field)| FieldValue { field_id: field.dto.field_id.clone(), value: if index == 0 { String::new() } else { literal.into() } }).collect::<Vec<_>>();
        let (bytes, fields) = parsed.prepare(&patches).unwrap();
        let output = session(bytes); let reparsed = FormDocument::parse(&output).unwrap();
        assert_eq!(fields[0].value, ""); assert_eq!(fields[1].value, literal);
        let city = reparsed.document.get_dictionary(reparsed.fields[1].object).unwrap();
        let ap = dict(&reparsed.document, city.get(b"AP").unwrap()).unwrap().get(b"N").unwrap();
        let stream = object(&reparsed.document, ap).unwrap().as_stream().unwrap();
        let decoded = Content::decode(&stream.content).unwrap(); let shown = decoded.operations.iter().filter(|operation| operation.operator == "Tj").collect::<Vec<_>>();
        assert_eq!(shown.len(),1); assert_eq!(shown[0].operands[0].as_str().unwrap(),literal.as_bytes());
        let original = FormDocument::parse(&source).unwrap();
        for (before, after) in original.fields.iter().zip(&reparsed.fields) {
            assert_eq!(before.style.size, after.style.size); assert_eq!(before.style.background, after.style.background);
            assert_eq!(before.style.border_color, after.style.border_color); assert_eq!(before.style.text_color, after.style.text_color);
            let a = original.document.get_dictionary(before.object).unwrap(); let b = reparsed.document.get_dictionary(after.object).unwrap();
            for key in [b"DV".as_slice(), b"DA", b"Rect", b"BS", b"MK", b"P", b"T", b"F", b"Ff"] { assert_eq!(a.get(key).ok(), b.get(key).ok(), "Preserve original widget property"); }
        }
        assert_eq!(source.source, FIXTURE); assert_eq!(source.revision, 0);
    }
    #[test]
    fn forms_refuse_ambiguous_widgets_hierarchies_actions_and_unknown_appearances() {
        for kind in 0..13 {
            let source = mutated(|doc| {
                let ids = field_ids(doc); let page = *doc.get_pages().values().next().unwrap();
                match kind {
                    0 => { doc.get_dictionary_mut(page).unwrap().set("Annots", vec![Object::Reference(ids[0]),Object::Reference(ids[0])]); },
                    1 => { doc.get_dictionary_mut(page).unwrap().set("Annots", vec![Object::Reference(ids[0])]); },
                    2 => { doc.get_dictionary_mut(ids[1]).unwrap().set("T", Object::string_literal("Name")); },
                    3 => { doc.get_dictionary_mut(ids[0]).unwrap().set("Parent", ids[0]); },
                    4 => { doc.get_dictionary_mut(ids[0]).unwrap().set("Kids", vec![Object::Reference(ids[0])]); },
                    5 => { doc.get_dictionary_mut(ids[0]).unwrap().set("AA", dictionary!{}); },
                    6 => { doc.get_dictionary_mut(ids[0]).unwrap().set("Ff", 4096); },
                    7 => { doc.get_dictionary_mut(ids[0]).unwrap().set("P", ids[0]); },
                    8 => { doc.get_dictionary_mut(ids[0]).unwrap().remove(b"AP"); },
                    9 => { doc.get_dictionary_mut(ids[0]).unwrap().set("V", Object::string_literal("different")); },
                    10 => { let ap = dict(doc, doc.get_dictionary(ids[0]).unwrap().get(b"AP").unwrap()).unwrap().get(b"N").unwrap().as_reference().unwrap(); doc.get_object_mut(ap).unwrap().as_stream_mut().unwrap().dict.set("Matrix", real(&[1.0,0.0,0.0,1.0,5.0,0.0])); },
                    11 => { let pages = doc.catalog().unwrap().get(b"Pages").unwrap().as_reference().unwrap(); doc.get_dictionary_mut(pages).unwrap().set("Kids", vec![Object::Reference(pages)]); },
                    _ => { let form = doc.catalog().unwrap().get(b"AcroForm").unwrap().as_reference().unwrap(); doc.get_dictionary_mut(form).unwrap().set("NeedAppearances", true); },
                }
            });
            let before = source.source.clone(); let query = FormDocument::query(&source, 7);
            assert_eq!(query.status, "unsupported", "Adversarial case {kind}"); assert!(query.fields.is_empty()); assert_eq!(source.source, before);
        }
    }
    #[test]
    fn forms_patch_validation_is_atomic_and_enforces_ascii_length_and_fit() {
        let source = fixture(); let id = FormDocument::parse(&source).unwrap().fields[0].dto.field_id.clone();
        for values in [vec![], vec![FieldValue { field_id: "unknown".into(), value: "new".into() }], vec![FieldValue { field_id:id.clone(),value:"Original".into() }], vec![FieldValue { field_id:id.clone(),value:"é".into() }], vec![FieldValue { field_id:id.clone(),value:"line\nline".into() }], vec![FieldValue { field_id:id.clone(),value:"W".repeat(40) }], vec![FieldValue { field_id:id.clone(),value:"a".repeat(41) }], vec![FieldValue { field_id:id.clone(),value:"a".repeat(4097) }], vec![FieldValue { field_id:id.clone(),value:"a".into() },FieldValue { field_id:id.clone(),value:"b".into() }]] {
            assert!(FormDocument::parse(&source).unwrap().prepare(&values).is_err()); assert_eq!(source.source, FIXTURE); assert_eq!(source.revision, 0);
        }
        let mut edited = fixture(); edited.plan[0].turns = 1; assert!(FormDocument::parse(&edited).is_err());
        let oversized = session(vec![b' '; MAX_SOURCE+1]); assert!(FormDocument::parse(&oversized).err().unwrap().contains("64 MiB"));
    }
    #[test]
    fn forms_import_budgets_and_font_style_trust_boundary_are_explicit() {
        let build_fields = |count: usize| mutated(|doc| {
            let original = doc.get_dictionary(field_ids(doc)[0]).unwrap().clone(); let page = *doc.get_pages().values().next().unwrap(); let form = doc.catalog().unwrap().get(b"AcroForm").unwrap().as_reference().unwrap(); let mut refs = Vec::new();
            for index in 0..count { let mut field = original.clone(); field.set("T",Object::string_literal(format!("Field{index}"))); refs.push(Object::Reference(doc.add_object(field))); }
            doc.get_dictionary_mut(form).unwrap().set("Fields",refs.clone()); doc.get_dictionary_mut(page).unwrap().set("Annots",refs);
        });
        assert_eq!(FormDocument::parse(&build_fields(MAX_FIELDS)).unwrap().fields.len(),MAX_FIELDS);
        assert!(FormDocument::parse(&build_fields(MAX_FIELDS+1)).err().unwrap().contains("256"));
        for kind in 0..12 {
            let source = mutated(|doc| {
                let id=field_ids(doc)[0]; let form=doc.catalog().unwrap().get(b"AcroForm").unwrap().as_reference().unwrap(); let page=*doc.get_pages().values().next().unwrap();
                match kind {
                    0 => {let font=dict(doc,doc.get_dictionary(form).unwrap().get(b"DR").unwrap()).unwrap().get(b"Font").unwrap().as_dict().unwrap().get(b"Helv").unwrap().as_reference().unwrap(); doc.get_dictionary_mut(font).unwrap().set("Widths",vec![Object::Integer(999)]);},
                    1 => {doc.get_dictionary_mut(id).unwrap().set("DA",Object::string_literal("/Helv 0 Tf 0 g"));},
                    2 => {doc.get_dictionary_mut(id).unwrap().set("MaxLen",-1);},
                    3 => {doc.get_dictionary_mut(id).unwrap().set("F",6);},
                    4 => {doc.get_dictionary_mut(form).unwrap().set("XFA",Object::string_literal("unsupported"));},
                    5 => {doc.get_dictionary_mut(form).unwrap().set("CO",vec![Object::Reference(id)]);},
                    6 => {doc.get_dictionary_mut(id).unwrap().set("ByteRange",vec![Object::Integer(0)]);},
                    7 => {let root=doc.trailer.get(b"Root").unwrap().as_reference().unwrap(); doc.get_dictionary_mut(root).unwrap().remove(b"AcroForm");},
                    8 => {doc.get_dictionary_mut(id).unwrap().set("AP",id);},
                    9 => {let mut extra=doc.get_dictionary(id).unwrap().clone(); extra.set("F",2); let extra=doc.add_object(extra); doc.get_dictionary_mut(page).unwrap().get_mut(b"Annots").unwrap().as_array_mut().unwrap().push(Object::Reference(extra));},
                    10 => {let ap=dict(doc,doc.get_dictionary(id).unwrap().get(b"AP").unwrap()).unwrap().get(b"N").unwrap().as_reference().unwrap(); let stream=doc.get_object_mut(ap).unwrap().as_stream_mut().unwrap(); stream.set_content(vec![b' ';MAX_AP+1]); stream.compress().unwrap();},
                    _ => {doc.get_dictionary_mut(id).unwrap().set("T",Object::string_literal(""));},
                }
            });
            let query=FormDocument::query(&source,1); assert_eq!(query.status,"unsupported","Trust-boundary case {kind}"); assert!(query.fields.is_empty());
        }
        let blank_without_ap=mutated(|doc| {let id=field_ids(doc)[1]; doc.get_dictionary_mut(id).unwrap().remove(b"AP");}); assert_eq!(FormDocument::query(&blank_without_ap,1).status,"supported");
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormField { pub field_id: String, pub name: String, pub page: usize, pub value: String, pub max_length: Option<usize> }
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldValue { pub field_id: String, pub value: String }

#[derive(Clone)]
struct Style { width: f32, height: f32, border: f32, background: Option<Vec<f32>>, border_color: Option<Vec<f32>>, text_color: Vec<f32>, font: String, size: f32, font_object: Object }
struct ParsedField { dto: FormField, object: ObjectId, style: Style }
pub struct FormDocument { document: Document, fields: Vec<ParsedField>, pub pages: usize }

fn object<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Object, String> { doc.dereference(value).map(|(_, value)| value).map_err(|_| "The form contains an invalid or cyclic reference.".into()) }
fn dict<'a>(doc: &'a Document, value: &'a Object) -> Result<&'a Dictionary, String> { object(doc, value)?.as_dict().map_err(|_| "The form contains an invalid dictionary.".into()) }
fn keys(value: &Dictionary, allowed: &[&[u8]]) -> Result<(), String> { if value.iter().any(|(key, _)| !allowed.contains(&key.as_slice())) { Err("This form uses unsupported field, appearance, or layout properties.".into()) } else { Ok(()) } }
fn number(value: &Object) -> Result<f32, String> { let value = value.as_float().map_err(|_| "The form contains an invalid number.")?; if value.is_finite() { Ok(value) } else { Err("The form contains a non-finite number.".into()) } }
fn numbers(value: &Object) -> Result<Vec<f32>, String> { let values = value.as_array().map_err(|_| "The form contains an invalid numeric array.")?; if values.len() > 6 { return Err("The form numeric array is too large.".into()); } values.iter().map(number).collect() }
fn text(value: &Object) -> Result<String, String> { lopdf::decode_text_string(value).map_err(|_| "The form contains an unsupported text encoding.".into()) }
fn ascii(value: &str) -> Result<(), String> { if value.len() > MAX_VALUE { return Err("Each form value is limited to 4 KiB.".into()); } if !value.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) { return Err("This form tool accepts printable ASCII text only, without tabs or newlines.".into()); } Ok(()) }
fn color(value: &Object) -> Result<Vec<f32>, String> { let value = numbers(value)?; if !matches!(value.len(), 1 | 3) || value.iter().any(|x| !(0.0..=1.0).contains(x)) { return Err("Only gray or RGB form colors are supported.".into()); } Ok(value) }
fn real(values: &[f32]) -> Vec<Object> { values.iter().copied().map(Object::Real).collect() }
fn op(name: &str, values: &[f32]) -> Operation { Operation::new(name, real(values)) }
fn paint(values: &[f32], stroke: bool) -> Operation { op(match (values.len(), stroke) { (1, false) => "g", (1, true) => "G", (_, false) => "rg", (_, true) => "RG" }, values) }

fn appearance(style: &Style, value: &str) -> Content<Vec<Operation>> {
    let mut operations = Vec::new();
    if let Some(background) = &style.background { operations.extend([paint(background, false), op("re", &[0.0, 0.0, style.width, style.height]), op("f", &[])]); }
    if let Some(border) = &style.border_color { if style.border > 0.0 { operations.extend([paint(border, true), op("w", &[style.border]), op("re", &[style.border / 2.0, style.border / 2.0, style.width - style.border, style.height - style.border]), op("s", &[])]); } }
    let inset = style.border * 2.0;
    operations.extend([Operation::new("BMC", vec![Object::Name(b"Tx".to_vec())]), op("q", &[]), op("re", &[inset, inset, style.width - 2.0 * inset, style.height - 2.0 * inset]), op("W", &[]), op("n", &[]), op("g", &[0.0]), op("G", &[0.0])]);
    if !value.is_empty() { operations.extend([op("BT", &[]), Operation::new("Tf", vec![Object::Name(style.font.as_bytes().to_vec()), Object::Real(style.size)]), paint(&style.text_color, false), op("Tm", &[1.0, 0.0, 0.0, 1.0, style.border * 4.0, style.height - style.size - inset]), Operation::new("Tj", vec![Object::string_literal(value.as_bytes().to_vec())]), op("ET", &[])]); }
    operations.extend([op("Q", &[]), op("EMC", &[])]);
    Content { operations }
}
fn same_object(a: &Object, b: &Object) -> bool {
    match (a, b) { (Object::Integer(_) | Object::Real(_), Object::Integer(_) | Object::Real(_)) => number(a).ok().zip(number(b).ok()).is_some_and(|(a, b)| (a - b).abs() < 0.0001), _ => a == b }
}
fn same_appearance(actual: &Content<Vec<Operation>>, expected: &Content<Vec<Operation>>) -> bool {
    actual.operations.len() == expected.operations.len() && actual.operations.iter().zip(&expected.operations).all(|(a, b)| a.operator == b.operator && a.operands.len() == b.operands.len() && a.operands.iter().zip(&b.operands).all(|(a, b)| same_object(a, b)))
}

fn font(doc: &Document, value: &Object) -> Result<(), String> {
    let font = dict(doc, value)?; keys(font, &[b"Type", b"Subtype", b"BaseFont", b"Name", b"Encoding"])?;
    if font.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Type1") || font.get(b"BaseFont").and_then(Object::as_name).ok() != Some(b"Helvetica") { return Err("Only standard Helvetica form fonts are supported.".into()); }
    let encoding = object(doc, font.get(b"Encoding").map_err(|_| "The form font encoding is missing.")?)?;
    if encoding.as_name().ok() == Some(b"WinAnsiEncoding") { return Ok(()); }
    let encoding = encoding.as_dict().map_err(|_| "The form font encoding is unsupported.")?; keys(encoding, &[b"Type", b"BaseEncoding", b"Differences"])?;
    let base = encoding.get(b"BaseEncoding").and_then(Object::as_name).ok();
    if base.is_some() && base != Some(b"WinAnsiEncoding") && base != Some(b"StandardEncoding") { return Err("The form font encoding is unsupported.".into()); }
    let differences = encoding.get(b"Differences").and_then(Object::as_array).map_err(|_| "The form font differences are unsupported.")?;
    if differences.len() > 512 { return Err("The form font encoding is too large.".into()); }
    let mut code = 0; let mut quote = base == Some(b"WinAnsiEncoding"); let mut grave = quote;
    for value in differences {
        if let Ok(index) = value.as_i64() { if !(0..=255).contains(&index) { return Err("Invalid form font encoding index.".into()); } code = index; }
        else { let name = value.as_name().map_err(|_| "Invalid form font glyph.")?; if code > 255 { return Err("Invalid form font encoding range.".into()); } if (32..=126).contains(&code) { if code == 39 && name == b"quotesingle" { quote = true; } else if code == 96 && name == b"grave" { grave = true; } else { return Err("The form remaps printable ASCII glyphs.".into()); } } code += 1; }
    }
    if !quote || !grave { return Err("The form does not use the supported ASCII quote encoding.".into()); } Ok(())
}

fn style(doc: &Document, form: &Dictionary, field: &Dictionary) -> Result<Style, String> {
    let rect = numbers(field.get(b"Rect").map_err(|_| "A form field has no rectangle.")?)?;
    if rect.len() != 4 { return Err("A form field rectangle is malformed.".into()); }
    let (width, height) = (rect[2] - rect[0], rect[3] - rect[1]);
    if width < 8.0 || height < 8.0 || width > 10000.0 || height > 10000.0 { return Err("The form field dimensions are unsupported.".into()); }
    let da = field.get(b"DA").or_else(|_| form.get(b"DA")).map_err(|_| "The form has no default text appearance.")?.as_str().map_err(|_| "The form text appearance is invalid.")?;
    if da.len() > 1024 { return Err("The form text appearance is too large.".into()); }
    let da = Content::decode(da).map_err(|_| "The form text appearance cannot be parsed.")?;
    if da.operations.len() != 2 || da.operations[0].operator != "Tf" || da.operations[0].operands.len() != 2 || !matches!(da.operations[1].operator.as_str(), "g" | "rg") { return Err("The form uses unsupported text appearance operators.".into()); }
    let name = da.operations[0].operands[0].as_name().map_err(|_| "The form font name is invalid.")?;
    if name.is_empty() || name.len() > 64 || !name.iter().all(u8::is_ascii_alphanumeric) { return Err("The form font name is unsupported.".into()); }
    let size = number(&da.operations[0].operands[1])?;
    if !(4.0..=72.0).contains(&size) { return Err("Automatic or unsupported form font sizes are unavailable.".into()); }
    let text_color = color(&Object::Array(da.operations[1].operands.clone()))?;
    if da.operations[1].operator == "g" && text_color.len() != 1 || da.operations[1].operator == "rg" && text_color.len() != 3 { return Err("The form text color is malformed.".into()); }
    let resources = dict(doc, form.get(b"DR").map_err(|_| "The form font resources are missing.")?)?; keys(resources, &[b"Font", b"Encoding"])?;
    let fonts = dict(doc, resources.get(b"Font").map_err(|_| "The form fonts are missing.")?)?;
    if fonts.len() > 16 { return Err("The form font resource list is too large.".into()); }
    let font_object = fonts.get(name).map_err(|_| "The form font resource cannot be found.")?.clone(); font(doc, &font_object)?;
    let mut border = 0.0;
    if let Ok(value) = field.get(b"BS") { let value = dict(doc, value)?; keys(value, &[b"S", b"W"])?; if value.get(b"S").and_then(Object::as_name).ok() != Some(b"S") { return Err("Only solid form borders are supported.".into()); } border = number(value.get(b"W").map_err(|_| "The form border width is missing.")?)?; }
    if !(0.0..=4.0).contains(&border) { return Err("The form border width is unsupported.".into()); }
    let (background, border_color) = if let Ok(value) = field.get(b"MK") { let value = dict(doc, value)?; keys(value, &[b"BG", b"BC"])?; (value.get(b"BG").ok().map(color).transpose()?, value.get(b"BC").ok().map(color).transpose()?) } else { (None, None) };
    if border > 0.0 && border_color.is_none() { return Err("The form border color is missing.".into()); }
    if width <= 8.0 * border || height < 1.225 * size + 4.0 * border { return Err("The form layout cannot fit its fixed text size without clipping descenders.".into()); }
    Ok(Style { width, height, border, background, border_color, text_color, font: String::from_utf8(name.to_vec()).map_err(|_| "Invalid form font name.")?, size, font_object })
}

// Standard Helvetica ASCII advance widths, matching ReportLab's standard-font metrics (1/1000 em).
const WIDTHS: [u16; 95] = [278,278,355,556,556,889,667,191,333,333,389,584,278,333,278,278,556,556,556,556,556,556,556,556,556,556,278,278,584,584,584,556,1015,667,667,722,722,667,611,778,722,278,500,667,556,833,722,778,667,778,722,667,611,722,667,944,667,667,611,278,278,278,469,556,333,556,556,500,556,556,278,556,556,222,222,500,222,833,556,556,556,556,333,500,278,556,500,722,500,500,500,334,260,334,584];
fn fits(field: &ParsedField, value: &str) -> Result<(), String> {
    ascii(value)?;
    if field.dto.max_length.is_some_and(|max| value.len() > max) { return Err("The form value exceeds the field's MaxLength.".into()); }
    let advance = value.bytes().map(|byte| WIDTHS[(byte - 32) as usize] as f32).sum::<f32>() * field.style.size / 1000.0;
    if advance > field.style.width - 8.0 * field.style.border - 1.0 || advance + 0.04 * field.style.size > field.style.width - 6.0 * field.style.border || value.starts_with(['j','/']) && field.style.border * 2.0 < 0.03 * field.style.size { return Err("The text does not fit this field at its existing font size without clipping. Use a shorter value.".into()); } Ok(())
}

impl FormDocument {
    pub fn parse(session: &EditSession) -> Result<Self, String> {
        if session.source.len() > MAX_SOURCE { return Err("Form filling is limited to source PDFs of 64 MiB.".into()); }
        if session.plan.iter().enumerate().any(|(index, page)| page.source != index || page.turns != 0 || page.crop.is_some() || !page.notes.is_empty()) { return Err("Form filling requires the original page plan. Open a fresh copy before filling fields.".into()); }
        let document = Document::load_mem_with_options(&session.source, LoadOptions::with_max_decompressed_size(MAX_TEXT)).map_err(|_| "The PDF cannot be parsed within the form-loading limits.")?;
        if document.is_encrypted() || document.encryption_state.is_some() { return Err("Encrypted PDFs cannot be filled in this build.".into()); }
        if document.objects.len() > 50000 { return Err("This PDF exceeds the form object-count limit.".into()); }
        let catalog = document.catalog().map_err(|_| "The PDF catalog is invalid.")?;
        if catalog.has(b"Perms") { return Err("Signed or certified PDFs cannot be filled in this build.".into()); }
        let mut nodes = 0;
        fn safe(value: &Object, nodes: &mut usize, depth: usize) -> Result<(), String> {
            *nodes += 1; if *nodes > 200000 || depth > 32 { return Err("The form document exceeds traversal limits.".into()); }
            match value { Object::Dictionary(dict) => { if [b"A".as_slice(), b"AA", b"OpenAction", b"JS", b"JavaScript", b"ByteRange", b"XFA", b"CO"].iter().any(|key| dict.has(key)) || dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Sig") || dict.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig") { return Err("Forms with signatures, actions, calculations, or XFA are not supported.".into()); } for (_, child) in dict { safe(child, nodes, depth + 1)?; } }, Object::Array(values) => for child in values { safe(child, nodes, depth + 1)?; }, Object::Stream(stream) => safe(&Object::Dictionary(stream.dict.clone()), nodes, depth + 1)?, _ => {} } Ok(())
        }
        for value in document.objects.values() { safe(value, &mut nodes, 0)?; }
        let mut pages = Vec::new(); let mut seen = HashSet::new();
        fn page_tree(doc: &Document, id: ObjectId, parent: Option<ObjectId>, pages: &mut Vec<ObjectId>, seen: &mut HashSet<ObjectId>, depth: usize) -> Result<(), String> {
            if depth > 32 || seen.len() >= 8192 || !seen.insert(id) { return Err("The page tree is cyclic, repeated, or too large for form filling.".into()); }
            let value = doc.get_dictionary(id).map_err(|_| "The form page tree is invalid.")?;
            if let Some(parent) = parent { if value.get(b"Parent").and_then(Object::as_reference).ok() != Some(parent) { return Err("The form page parent mapping is invalid.".into()); } }
            match value.get(b"Type").and_then(Object::as_name).ok() { Some(b"Page") => { pages.push(id); if pages.len() > 4096 { return Err("Form filling is limited to 4,096 pages.".into()); } }, Some(b"Pages") => { let kids = value.get(b"Kids").and_then(Object::as_array).map_err(|_| "The form page children are invalid.")?; if kids.len() > 4096 { return Err("The form page child list is too large.".into()); } let before = pages.len(); for kid in kids { page_tree(doc, kid.as_reference().map_err(|_| "The form page child is invalid.")?, Some(id), pages, seen, depth + 1)?; } if value.get(b"Count").and_then(Object::as_i64).ok() != Some((pages.len() - before) as i64) { return Err("The form page count is inconsistent.".into()); } }, _ => return Err("The form page type is invalid.".into()) } Ok(())
        }
        page_tree(&document, catalog.get(b"Pages").and_then(Object::as_reference).map_err(|_| "The form page root is invalid.")?, None, &mut pages, &mut seen, 0)?;
        if pages.len() != session.plan.len() { return Err("The form page count disagrees with the open document.".into()); }
        let Some(form) = catalog.get(b"AcroForm").ok() else {
            for page in &pages { if let Ok(annots) = document.get_dictionary(*page).map_err(|_| "Invalid form page.")?.get(b"Annots") {
                let annots = object(&document, annots)?.as_array().map_err(|_| "The page annotation list is invalid.")?;
                if annots.len() > MAX_FIELDS { return Err("The page annotation list exceeds the form inspection limit.".into()); }
                for annotation in annots { if dict(&document, annotation)?.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget") { return Err("A page widget has no canonical AcroForm field tree.".into()); } }
            } }
            return Ok(Self { document, fields: Vec::new(), pages: pages.len() });
        };
        let form = dict(&document, form)?; keys(form, &[b"Fields", b"DA", b"DR", b"NeedAppearances"])?;
        if form.get(b"NeedAppearances").ok().is_some_and(|value| value.as_bool().ok() != Some(false)) { return Err("Forms requiring viewer-generated appearances are not supported.".into()); }
        let roots = object(&document, form.get(b"Fields").map_err(|_| "The form field tree is missing.")?)?.as_array().map_err(|_| "The form field tree is invalid.")?;
        if roots.len() > MAX_FIELDS { return Err("Form filling is limited to 256 fields.".into()); }
        let mut widgets = HashMap::new();
        for (page, id) in pages.iter().enumerate() { if let Ok(annots) = document.get_dictionary(*id).map_err(|_| "Invalid form page.")?.get(b"Annots") { let annots = object(&document, annots)?.as_array().map_err(|_| "The form widget list is invalid.")?; if annots.len() > MAX_FIELDS { return Err("The form widget list is too large.".into()); } for annot in annots { let id = annot.as_reference().map_err(|_| "Only referenced form widgets are supported.")?; if widgets.insert(id, page).is_some() { return Err("Repeated or ambiguous form widgets are not supported.".into()); } if widgets.len() > MAX_FIELDS { return Err("Form filling is limited to 256 total page widgets.".into()); } } } }
        let mut fields = Vec::new(); let mut names = HashSet::new(); let mut ids = HashSet::new(); let mut text_bytes = 0; let mut ap_bytes = 0;
        for reference in roots {
            let id = reference.as_reference().map_err(|_| "Only flat referenced form fields are supported.")?;
            if !ids.insert(id) { return Err("Repeated form field references are not supported.".into()); }
            let field = document.get_dictionary(id).map_err(|_| "The form field is invalid.")?;
            keys(field, &[b"Type", b"Subtype", b"FT", b"T", b"TU", b"V", b"DV", b"F", b"Ff", b"Rect", b"P", b"DA", b"AP", b"BS", b"MK", b"MaxLen", b"Q"])?;
            if field.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Widget") || field.get(b"FT").and_then(Object::as_name).ok() != Some(b"Tx") { return Err("Only flat single-line text fields with one merged widget are supported.".into()); }
            if field.get(b"Type").ok().is_some_and(|value| value.as_name().ok() != Some(b"Annot")) { return Err("The form widget annotation type is invalid.".into()); }
            if field.get(b"F").and_then(Object::as_i64).ok() != Some(4) || field.get(b"Ff").ok().is_some_and(|value| !matches!(value.as_i64().ok(), Some(0 | 2))) || field.get(b"Q").ok().is_some_and(|value| value.as_i64().ok() != Some(0)) { return Err("The form flags or text alignment are unsupported.".into()); }
            let page = *widgets.get(&id).ok_or("The canonical form field has no unique page widget.")?;
            if field.get(b"P").and_then(Object::as_reference).ok() != Some(pages[page]) { return Err("The form widget page relationship is inconsistent.".into()); }
            let name = text(field.get(b"T").map_err(|_| "An unnamed form field is unsupported.")?)?;
            if name.trim().is_empty() || name.len() > 1024 || name.chars().any(char::is_control) || !names.insert(name.clone()) { return Err("Unnamed, duplicate, or unsupported field names are not supported.".into()); }
            let value = field.get(b"V").ok().map(text).transpose()?.unwrap_or_default(); ascii(&value)?;
            if let Ok(default) = field.get(b"DV") { ascii(&text(default)?)?; }
            text_bytes += name.len() + value.len(); if text_bytes > MAX_TEXT { return Err("The form text exceeds 1 MiB.".into()); }
            let max_length = field.get(b"MaxLen").ok().map(|value| value.as_i64().map_err(|_| "The form MaxLength is invalid.").and_then(|value| usize::try_from(value).map_err(|_| "The form MaxLength is invalid."))).transpose()?;
            if max_length.is_some_and(|value| value == 0 || value > i32::MAX as usize) { return Err("The form MaxLength is unsupported.".into()); }
            let style = style(&document, form, field)?;
            if let Ok(ap) = field.get(b"AP") {
                let ap = dict(&document, ap)?; keys(ap, &[b"N"])?;
                let stream = object(&document, ap.get(b"N").map_err(|_| "The form normal appearance is missing.")?)?.as_stream().map_err(|_| "The form normal appearance is invalid.")?;
                keys(&stream.dict, &[b"Type", b"Subtype", b"FormType", b"BBox", b"Matrix", b"Resources", b"Length", b"Filter"])?;
                if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") || stream.dict.get(b"FormType").ok().is_some_and(|value| value.as_i64().ok() != Some(1)) { return Err("The form appearance type is unsupported.".into()); }
                if stream.dict.get(b"Type").ok().is_some_and(|value| value.as_name().ok() != Some(b"XObject")) { return Err("The form appearance object type is invalid.".into()); }
                let bbox = numbers(stream.dict.get(b"BBox").map_err(|_| "The form appearance bounds are missing.")?)?;
                if bbox.len() != 4 || !bbox.iter().zip([0.0, 0.0, style.width, style.height]).all(|(a,b)| (a-b).abs() < 0.0001) { return Err("The form appearance bounds do not match its widget.".into()); }
                if stream.dict.get(b"Matrix").ok().map(numbers).transpose()?.is_some_and(|matrix| matrix != vec![1.0,0.0,0.0,1.0,0.0,0.0]) { return Err("Rotated or transformed form appearances are unsupported.".into()); }
                let resources = dict(&document, stream.dict.get(b"Resources").map_err(|_| "The form appearance resources are missing.")?)?; keys(resources, &[b"Font", b"ProcSet"])?;
                if resources.get(b"ProcSet").ok().is_some_and(|value| value.as_array().ok().is_none_or(|values| values.as_slice() != [Object::Name(b"PDF".to_vec()), Object::Name(b"Text".to_vec())])) { return Err("The form appearance procedure resources are unsupported.".into()); }
                let fonts = dict(&document, resources.get(b"Font").map_err(|_| "The appearance font resource is missing.")?)?;
                if fonts.len() != 1 { return Err("The appearance font resources are unsupported.".into()); } font(&document, fonts.get(style.font.as_bytes()).map_err(|_| "The appearance font differs from the form font.")?)?;
                let decoded = stream.decompressed_content_with_limit(MAX_AP).map_err(|_| "The form appearance exceeds its decoding limit or uses an unsupported filter.")?; ap_bytes += decoded.len(); if ap_bytes > MAX_TEXT { return Err("The form appearances exceed 1 MiB.".into()); }
                if !same_appearance(&Content::decode(&decoded).map_err(|_| "The form appearance cannot be parsed.")?, &appearance(&style, &value)) { return Err("The form contains an unknown appearance or layout; filling it could discard artwork.".into()); }
            } else if !value.is_empty() { return Err("A populated form field without its original appearance is unsupported.".into()); }
            let parsed = ParsedField { dto: FormField { field_id: format!("field-{}-{}", id.0, id.1), name, page, value, max_length }, object: id, style }; fits(&parsed, &parsed.dto.value)?; fields.push(parsed);
        }
        if ids.len() != widgets.len() { return Err("The form contains orphaned or foreign page annotations.".into()); }
        Ok(Self { document, fields, pages: pages.len() })
    }
    pub fn query(session: &EditSession, id: u64) -> FormFields {
        let (fields, reason) = match Self::parse(session) { Ok(form) => (form.fields.into_iter().map(|field| field.dto).collect(), None), Err(reason) => (Vec::new(), Some(reason)) };
        FormFields { document_id: id, revision: session.revision, status: if reason.is_none() { "supported" } else { "unsupported" }, reason, input: "printable-ascii", value_byte_limit: MAX_VALUE, fields }
    }
    pub fn prepare(mut self, values: &[FieldValue]) -> Result<(Vec<u8>, Vec<FormField>), String> {
        if values.is_empty() || values.len() > MAX_FIELDS { return Err("Change between one and 256 form fields before saving a copy.".into()); }
        let mut seen = HashSet::new(); let mut changed = false;
        for patch in values { if !seen.insert(&patch.field_id) { return Err("A form field was submitted more than once.".into()); } let field = self.fields.iter_mut().find(|field| field.dto.field_id == patch.field_id).ok_or("A form field no longer exists. Refresh the field list.")?; fits(field, &patch.value)?; changed |= patch.value != field.dto.value; field.dto.value = patch.value.clone(); }
        if !changed { return Err("Change at least one form value before saving a new copy.".into()); }
        if self.fields.iter().map(|field| field.dto.name.len() + field.dto.value.len()).sum::<usize>() > MAX_TEXT { return Err("The form names and values exceed 1 MiB.".into()); }
        for field in &self.fields { if !seen.contains(&field.dto.field_id) { continue; } let content = appearance(&field.style, &field.dto.value).encode().map_err(|_| "Could not encode the form appearance.")?; let resources = dictionary! { "Font" => dictionary! { field.style.font.as_bytes() => field.style.font_object.clone() }, "ProcSet" => vec![Object::Name(b"PDF".to_vec()), Object::Name(b"Text".to_vec())] }; let ap = self.document.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Form", "FormType" => 1, "BBox" => real(&[0.0,0.0,field.style.width,field.style.height]), "Matrix" => real(&[1.0,0.0,0.0,1.0,0.0,0.0]), "Resources" => resources }, content)); let widget = self.document.get_dictionary_mut(field.object).map_err(|_| "The form field disappeared during preparation.")?; widget.set("V", Object::string_literal(field.dto.value.as_bytes().to_vec())); widget.set("AP", dictionary! { "N" => ap }); }
        let mut bytes = Vec::new(); self.document.save_to(&mut bytes).map_err(|_| "Could not serialize the filled form.")?; if bytes.len() > MAX_OUTPUT { return Err("The filled form output exceeds 256 MiB.".into()); } Ok((bytes, self.fields.into_iter().map(|field| field.dto).collect()))
    }
}
