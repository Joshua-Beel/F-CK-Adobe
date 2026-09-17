use pdfium_render::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct MetadataEntry {
    pub name: &'static str,
    pub value: String,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct PageDimensions {
    pub width_points: f32,
    pub height_points: f32,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct SecurityProperties {
    pub encrypted: Option<bool>,
    pub handler_revision: Option<u8>,
    pub print_high_quality: Option<bool>,
    pub print_low_quality_only: Option<bool>,
    pub modify_contents: Option<bool>,
    pub assemble_document: Option<bool>,
    pub fill_existing_forms: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct DocumentProperties {
    pub version: Option<String>,
    pub source_size_bytes: usize,
    pub page_count: usize,
    pub metadata: Vec<MetadataEntry>,
    pub page_dimensions: Vec<PageDimensions>,
    pub page_dimensions_truncated: bool,
    pub security: SecurityProperties,
    pub reported_signature_count: Option<u16>,
    pub signature_validation: &'static str,
}

// Called only by the PDF worker. Dimensions describe the current edit plan;
// file size, metadata, permissions and signatures describe the opened snapshot.
pub fn inspect(document: &PdfDocument<'_>, source_size_bytes: usize, current_page_dimensions: &[(f32, f32)]) -> Result<DocumentProperties, String> {
    let (page_dimensions, page_dimensions_truncated) = dimensions(current_page_dimensions)?;
    let metadata = document.metadata().iter().filter_map(|tag| metadata_entry(metadata_name(tag.tag_type()), tag.value())).collect();
    let permissions = document.permissions();
    let revision = permissions.security_handler_revision().ok();
    let (encrypted, handler_revision) = security_revision(revision);
    Ok(DocumentProperties {
        version: version_label(document.version()),
        source_size_bytes,
        page_count: current_page_dimensions.len(),
        metadata,
        page_dimensions,
        page_dimensions_truncated,
        security: SecurityProperties {
            encrypted,
            handler_revision,
            print_high_quality: permissions.can_print_high_quality().ok(),
            print_low_quality_only: permissions.can_print_only_low_quality().ok(),
            modify_contents: permissions.can_modify_document_content().ok(),
            assemble_document: permissions.can_assemble_document().ok(),
            fill_existing_forms: permissions.can_fill_existing_interactive_form_fields().ok(),
        },
        reported_signature_count: checked_signature_count(document.signatures().len()),
        signature_validation: "not_performed",
    })
}

fn checked_signature_count(count: u16) -> Option<u16> {
    (count != u16::MAX).then_some(count)
}

fn metadata_name(tag: PdfDocumentMetadataTagType) -> &'static str {
    match tag {
        PdfDocumentMetadataTagType::Title => "title",
        PdfDocumentMetadataTagType::Author => "author",
        PdfDocumentMetadataTagType::Subject => "subject",
        PdfDocumentMetadataTagType::Keywords => "keywords",
        PdfDocumentMetadataTagType::Creator => "creator",
        PdfDocumentMetadataTagType::Producer => "producer",
        PdfDocumentMetadataTagType::CreationDate => "creation_date_raw",
        PdfDocumentMetadataTagType::ModificationDate => "modification_date_raw",
    }
}

fn metadata_entry(name: &'static str, value: &str) -> Option<MetadataEntry> {
    if value.trim().is_empty() { return None; }
    let mut characters = value.chars();
    let value = characters.by_ref().take(4096).collect();
    Some(MetadataEntry { name, value, truncated: characters.next().is_some() })
}

fn security_revision(revision: Option<PdfSecurityHandlerRevision>) -> (Option<bool>, Option<u8>) {
    match revision {
        Some(PdfSecurityHandlerRevision::Unprotected) => (Some(false), None),
        Some(PdfSecurityHandlerRevision::Revision2) => (Some(true), Some(2)),
        Some(PdfSecurityHandlerRevision::Revision3) => (Some(true), Some(3)),
        Some(PdfSecurityHandlerRevision::Revision4) => (Some(true), Some(4)),
        None => (None, None),
    }
}

fn version_label(version: PdfDocumentVersion) -> Option<String> {
    let number = match version {
        PdfDocumentVersion::Unset => return None,
        PdfDocumentVersion::Pdf1_0 => 10,
        PdfDocumentVersion::Pdf1_1 => 11,
        PdfDocumentVersion::Pdf1_2 => 12,
        PdfDocumentVersion::Pdf1_3 => 13,
        PdfDocumentVersion::Pdf1_4 => 14,
        PdfDocumentVersion::Pdf1_5 => 15,
        PdfDocumentVersion::Pdf1_6 => 16,
        PdfDocumentVersion::Pdf1_7 => 17,
        PdfDocumentVersion::Pdf2_0 => 20,
        PdfDocumentVersion::Other(number) => number,
    };
    (number > 0).then(|| format!("{}.{}", number / 10, number % 10))
}

fn dimensions(pages: &[(f32, f32)]) -> Result<(Vec<PageDimensions>, bool), String> {
    if pages.is_empty() || pages.len() > 65536 { return Err("Invalid page count for document properties.".into()); }
    let mut groups = Vec::<PageDimensions>::new();
    let mut indices = HashMap::<(u32, u32), usize>::new();
    let mut truncated = false;
    for &(width, height) in pages {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err("Invalid page dimensions for document properties.".into());
        }
        let key = (width.to_bits(), height.to_bits());
        if let Some(&index) = indices.get(&key) { groups[index].count += 1; }
        else if groups.len() < 128 {
            indices.insert(key, groups.len());
            groups.push(PageDimensions { width_points: width, height_points: height, count: 1 });
        } else { truncated = true; }
    }
    Ok((groups, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_bounded_without_splitting_unicode_or_interpreting_dates() {
        assert!(metadata_entry("author", "  \t").is_none());
        let text = "🦀".repeat(4097);
        let value = metadata_entry("title", &text).unwrap();
        assert_eq!(value.value, "🦀".repeat(4096));
        assert!(value.truncated);
        assert!(!metadata_entry("title", &"x".repeat(4096)).unwrap().truncated);
        assert_eq!(metadata_entry("creation_date_raw", "D:20260917123456-05'00'").unwrap().value, "D:20260917123456-05'00'");
        assert_eq!(metadata_entry("title", "<script>alert(1)</script>").unwrap().value, "<script>alert(1)</script>");
    }

    #[test]
    fn dimension_groups_preserve_orientation_and_bound_distinct_sizes() {
        let (groups, truncated) = dimensions(&[(612.0,792.0),(792.0,612.0),(612.0,792.0)]).unwrap();
        assert!(!truncated);
        assert_eq!(groups.len(),2);
        assert_eq!(groups[0].count,2);
        assert_eq!(groups[1].width_points,792.0);
        let mut pages: Vec<_> = (1..=130).map(|n| (n as f32, 100.0)).collect();
        pages.push((1.0,100.0));
        let (groups,truncated) = dimensions(&pages).unwrap();
        assert!(truncated);
        assert_eq!(groups.len(),128);
        assert_eq!(groups[0].count,2);
        for bad in [0.0,-1.0,f32::NAN,f32::INFINITY] { assert!(dimensions(&[(bad,100.0)]).is_err()); }
        assert!(dimensions(&[]).is_err());
        assert!(dimensions(&vec![(1.0,1.0);65537]).is_err());
    }

    #[test]
    fn unknown_security_and_versions_are_not_guessed() {
        assert_eq!(checked_signature_count(u16::MAX), None);
        assert_eq!(checked_signature_count(0), Some(0));
        assert_eq!(checked_signature_count(2), Some(2));
        assert_eq!(security_revision(None),(None,None));
        assert_eq!(security_revision(Some(PdfSecurityHandlerRevision::Unprotected)),(Some(false),None));
        assert_eq!(security_revision(Some(PdfSecurityHandlerRevision::Revision4)),(Some(true),Some(4)));
        assert_eq!(version_label(PdfDocumentVersion::Unset),None);
        assert_eq!(version_label(PdfDocumentVersion::Other(-1)),None);
        assert_eq!(version_label(PdfDocumentVersion::Other(21)),Some("2.1".into()));
        assert_eq!(version_label(PdfDocumentVersion::Pdf1_7),Some("1.7".into()));
    }
}
