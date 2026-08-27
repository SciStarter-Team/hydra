//! Import/export of component collections as zip archives
//!
//! A "library" is any collection of components bundled as a zip archive
//! containing one JSON file per component. Libraries have no server-side
//! representation; they exist only as the bytes that flow in and out of
//! the admin UI.

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};

use anyhow::Error;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::peering::LoadStorePeered;

use super::PageBuilderComponent;

/// Outcome of an import operation, suitable for rendering to the user
#[derive(Debug, Default)]
pub struct ImportReport {
    pub imported: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Serialize the given components as a zip archive
pub fn export_to_zip(components: &[PageBuilderComponent]) -> Result<Vec<u8>, Error> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options: SimpleFileOptions =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut used: HashSet<String> = HashSet::new();
    for component in components {
        let base = slugify(&component.name);
        let mut filename = format!("{base}.json");
        let mut suffix = 2;
        while !used.insert(filename.clone()) {
            filename = format!("{base}-{suffix}.json");
            suffix += 1;
        }

        zip.start_file(&filename, options)?;
        let json = serde_json::to_vec_pretty(component)?;
        zip.write_all(&json)?;
    }

    Ok(zip.finish()?.into_inner())
}

/// Parse a zip archive and store each component it contains, overwriting
/// any existing component with a matching id
pub async fn import_from_zip(bytes: Vec<u8>) -> Result<ImportReport, Error> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut report = ImportReport::default();

    let mut entries: Vec<(String, String)> = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        if !file.is_file() || !name.ends_with(".json") {
            continue;
        }

        let mut contents = String::new();
        if let Err(e) = file.read_to_string(&mut contents) {
            report.failed.push((name, format!("read error: {e}")));
            continue;
        }

        entries.push((name, contents));
    }
    drop(archive);

    for (name, contents) in entries {
        match serde_json::from_str::<PageBuilderComponent>(&contents) {
            Ok(component) => {
                let component_name = component.name.clone();

                // Held to the same rules as a component authored by hand, so
                // a restore cannot reintroduce what the editor would refuse.
                if let Err(e) = PageBuilderComponent::check_source(&component.source) {
                    report.failed.push((name, format!("invalid source: {e}")));
                    continue;
                }

                match component.store().await {
                    Ok(()) => report.imported.push(component_name),
                    Err(e) => report.failed.push((name, format!("store error: {e}"))),
                }
            }
            Err(e) => report.failed.push((name, format!("parse error: {e}"))),
        }
    }

    Ok(report)
}

fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_dash = false;
    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("component");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert2::check;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hero Banner"), "hero-banner");
        assert_eq!(slugify("  Odd / Name!! "), "odd-name");
        assert_eq!(slugify("***"), "component");
        assert_eq!(slugify("MixedCase_123"), "mixedcase-123");
    }

    /// The shipped library has to satisfy the same rules an import enforces,
    /// or a restore would reject the very components it is restoring.
    #[test]
    fn foundation_library_files_valid() {
        use std::fs;
        use std::path::PathBuf;

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("components");

        for entry in fs::read_dir(&dir).expect("components dir") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&path).expect("read json");
            let comp = serde_json::from_str::<PageBuilderComponent>(&text);
            check!(let Ok(_) = &comp, "parse {}", path.display());
            if let Ok(comp) = comp {
                check!(
                    let Ok(_) = PageBuilderComponent::check_source(&comp.source),
                    "check {}",
                    path.display()
                );
            }
        }
    }
}
