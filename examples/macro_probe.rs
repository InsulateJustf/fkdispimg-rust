//! Standalone probe for WPS/Excel macro metadata.
//!
//! This is intentionally separate from the converter while the behavior is
//! being verified. Run it with:
//!
//! cargo run --example macro_probe -- 2.xlsx
//! cargo run --example macro_probe -- --repack probe.xlsm 2.xlsx

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use zip::read::ZipArchive;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const NORMAL_MAIN_CT: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const MACRO_MAIN_CT: &str = "application/vnd.ms-excel.sheet.macroEnabled.main+xml";
const VBA_CT: &str = "application/vnd.ms-office.vbaProject";
const VBA_SIGNATURE_CT: &str = "application/vnd.ms-office.vbaProjectSignature";

struct MacroReport {
    has_vba_project: bool,
    has_vba_signature: bool,
    has_vba_relationship: bool,
    workbook_content_type: Option<String>,
    has_vba_content_type: bool,
    legacy_macro_defined_names: Vec<String>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut action: Option<OutputAction> = None;
    let mut input: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repack" => {
                let output = args.next().map(PathBuf::from);
                if output.is_none() {
                    eprintln!("error: --repack needs an output path");
                    std::process::exit(2);
                }
                action = Some(OutputAction::Repack(output.unwrap()));
            }
            "--sanitize-legacy-macros" => {
                let output = args.next().map(PathBuf::from);
                if output.is_none() {
                    eprintln!("error: --sanitize-legacy-macros needs an output path");
                    std::process::exit(2);
                }
                action = Some(OutputAction::SanitizeLegacyMacros(output.unwrap()));
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            _ => {
                if input.is_none() {
                    input = Some(PathBuf::from(arg));
                } else {
                    eprintln!("error: only one input file is allowed");
                    std::process::exit(2);
                }
            }
        }
    }

    let input = match input {
        Some(path) => path,
        None => {
            print_help();
            std::process::exit(2);
        }
    };

    if let Err(err) = probe(&input, action) {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

enum OutputAction {
    Repack(PathBuf),
    SanitizeLegacyMacros(PathBuf),
}

fn print_help() {
    println!("usage: cargo run --example macro_probe -- [ACTION] INPUT.xlsx");
    println!("actions:");
    println!("  --repack OUTPUT.xlsm                     create a macro-enabled .xlsm test copy");
    println!("  --sanitize-legacy-macros OUTPUT.xlsx     remove XLM macro defined names and create a test copy");
}

fn probe(input: &Path, action: Option<OutputAction>) -> Result<(), String> {
    if !input.is_file() {
        return Err(format!("not a regular file: {}", input.display()));
    }

    let file = File::open(input).map_err(|err| format!("open failed: {err}"))?;
    let reader = BufReader::new(file);
    let mut archive = ZipArchive::new(reader)
        .map_err(|err| format!("not a valid ZIP/OOXML package: {err}"))?;

    let mut names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|err| format!("read ZIP entry {index} failed: {err}"))?;
        names.push(entry.name().to_string());
    }

    let content_types = names
        .iter()
        .find(|name| *name == "[Content_Types].xml")
        .map(|name| read_zip_text(&mut archive, name))
        .transpose()?;
    let workbook_xml = names
        .iter()
        .find(|name| *name == "xl/workbook.xml")
        .map(|name| read_zip_text(&mut archive, name))
        .transpose()?;

    let report = inspect(&names, content_types.as_deref(), workbook_xml.as_deref(), &mut archive);

    println!("input: {}", input.display());
    println!("entries: {}", names.len());
    println!("vbaProject.bin: {}", report.has_vba_project);
    println!("vbaProjectSignature.bin: {}", report.has_vba_signature);
    println!("workbook vbaProject relationship: {}", report.has_vba_relationship);
    println!(
        "workbook content type: {}",
        report
            .workbook_content_type
            .as_deref()
            .unwrap_or("<missing>")
    );
    println!("vbaProject content type override: {}", report.has_vba_content_type);
    if report.legacy_macro_defined_names.is_empty() {
        println!("legacy XLM macro defined names: none");
    } else {
        println!("legacy XLM macro defined names:");
        for name in &report.legacy_macro_defined_names {
            println!("  - {name}");
        }
    }

    let diagnosis = diagnose(&report, input);
    println!("\ndiagnosis: {diagnosis}");

    match action {
        Some(OutputAction::Repack(output)) => {
            repack_as_xlsm(&mut archive, &names, content_types.as_deref(), &output)?;
            println!("\nrepacked test copy: {}", output.display());
            println!("Please test the generated .xlsm file in Excel.");
        }
        Some(OutputAction::SanitizeLegacyMacros(output)) => {
            sanitize_legacy_macros(&mut archive, &names, &workbook_xml, &output)?;
            println!("\nsanitized test copy: {}", output.display());
            println!("Please test the generated .xlsx file in Excel.");
        }
        None => {}
    }

    Ok(())
}

fn read_zip_text<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<String, String> {
    let mut value = String::new();
    let mut entry = archive
        .by_name(name)
        .map_err(|err| format!("read {name} failed: {err}"))?;
    entry
        .read_to_string(&mut value)
        .map_err(|err| format!("read {name} failed: {err}"))?;
    Ok(value)
}

fn inspect<R: Read + std::io::Seek>(
    names: &[String],
    content_types: Option<&str>,
    workbook_xml: Option<&str>,
    archive: &mut ZipArchive<R>,
) -> MacroReport {
    let has_vba_project = names.iter().any(|name| name == "xl/vbaProject.bin");
    let has_vba_signature = names
        .iter()
        .any(|name| name == "xl/vbaProjectSignature.bin");

    let has_vba_relationship = ["xl/_rels/workbook.xml.rels"]
        .iter()
        .filter(|name| names.iter().any(|item| item == *name))
        .any(|name| {
            archive
                .by_name(name)
                .ok()
                .map(|mut entry| {
                    let mut text = String::new();
                    if entry.read_to_string(&mut text).is_err() {
                        return false;
                    }
                    text.contains("vbaProject.bin")
                })
                .unwrap_or(false)
        });

    let workbook_content_type = content_types.and_then(|xml| {
        let marker = "PartName=\"/xl/workbook.xml\"";
        let position = xml.find(marker)?;
        let rest = &xml[position..];
        let marker = "ContentType=\"";
        let start = rest.find(marker)? + marker.len();
        let end = rest[start..].find('"')? + start;
        Some(rest[start..end].to_string())
    });

    let has_vba_content_type = content_types
        .map(|xml| xml.contains(VBA_CT) || xml.contains(VBA_SIGNATURE_CT))
        .unwrap_or(false);

    let legacy_macro_defined_names = workbook_xml
        .map(find_legacy_macro_defined_names)
        .unwrap_or_default();

    MacroReport {
        has_vba_project,
        has_vba_signature,
        has_vba_relationship,
        workbook_content_type,
        has_vba_content_type,
        legacy_macro_defined_names,
    }
}

fn diagnose(report: &MacroReport, input: &Path) -> String {
    let extension = input
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .eq_ignore_ascii_case("xlsx");

    if report.has_vba_project || report.has_vba_signature || report.has_vba_relationship {
        let ct = report
            .workbook_content_type
            .as_deref()
            .unwrap_or("<missing>");

        if extension && ct != MACRO_MAIN_CT {
            return "macro parts are present but the package is declared/named as macro-free .xlsx; generate a .xlsm test copy".to_string();
        }

        if !extension {
            return "macro parts are present and the file is already macro-enabled by extension; no repack needed".to_string();
        }

        return "macro parts are present; the workbook content type is macro-enabled but the .xlsx extension may still confuse Excel".to_string();
    }

    if !report.legacy_macro_defined_names.is_empty() {
        return "legacy XLM macro functions are defined in workbook.xml; generate a sanitized test copy".to_string();
    }

    if report.workbook_content_type.as_deref() == Some(MACRO_MAIN_CT) {
        return "workbook is declared macro-enabled, but no VBA parts were found".to_string();
    }

    if report.workbook_content_type.as_deref() == Some(NORMAL_MAIN_CT) {
        return "no VBA parts detected; this looks like a macro-free workbook".to_string();
    }

    "workbook content type is missing or unexpected".to_string()
}

fn legacy_macro_defined_name_value_contains(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    ["EVALUATE(", "CALL(", "REGISTER(", "REGISTER.ID("]
        .iter()
        .any(|function| upper.contains(function))
}

fn find_legacy_macro_defined_names(workbook_xml: &str) -> Vec<String> {
    let entry_re = regex::Regex::new(r#"(?s)<definedName\b[^>]*>(.*?)</definedName>"#).unwrap();
    let name_re = regex::Regex::new(r#"name="([^"]+)""#).unwrap();

    entry_re
        .captures_iter(workbook_xml)
        .filter(|captures| legacy_macro_defined_name_value_contains(&captures[1]))
        .filter_map(|captures| {
            name_re
                .captures(&captures[0])
                .and_then(|name| name.get(1))
                .map(|name| name.as_str().to_string())
        })
        .collect()
}

fn sanitize_legacy_macro_defined_names(workbook_xml: &str) -> (String, Vec<String>) {
    let removed = find_legacy_macro_defined_names(workbook_xml);
    if removed.is_empty() {
        return (workbook_xml.to_string(), removed);
    }

    let entry_re = regex::Regex::new(r#"(?s)<definedName\b[^>]*>(.*?)</definedName>"#).unwrap();
    let sanitized = entry_re.replace_all(workbook_xml, |captures: &regex::Captures| {
        if legacy_macro_defined_name_value_contains(&captures[1]) {
            String::new()
        } else {
            captures[0].to_string()
        }
    });

    (sanitized.into_owned(), removed)
}

fn sanitize_legacy_macros<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    names: &[String],
    workbook_xml: &Option<String>,
    output: &Path,
) -> Result<(), String> {
    let workbook_xml = match workbook_xml {
        Some(value) => value,
        None => return Err("cannot sanitize: xl/workbook.xml was not found".to_string()),
    };

    let (sanitized_workbook, removed) = sanitize_legacy_macro_defined_names(workbook_xml);
    if removed.is_empty() {
        return Err("cannot sanitize: no legacy XLM macro defined names were found".to_string());
    }

    if output.exists() {
        return Err(format!("output already exists: {}", output.display()));
    }

    let out_file =
        File::create(output).map_err(|err| format!("create output failed: {err}"))?;
    let writer = BufWriter::new(out_file);
    let mut zip_writer = ZipWriter::new(writer);

    for name in names {
        let mut entry = archive
            .by_name(name)
            .map_err(|err| format!("read {name} failed: {err}"))?;
        if entry.is_dir() {
            continue;
        }

        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|err| format!("read {name} failed: {err}"))?;
        if name == "xl/workbook.xml" {
            data = sanitized_workbook.clone().into_bytes();
        }

        let options = SimpleFileOptions::default()
            .compression_method(entry.compression())
            .unix_permissions(entry.unix_mode().unwrap_or(0o644));
        zip_writer
            .start_file(name.as_str(), options)
            .map_err(|err| format!("write {name} failed: {err}"))?;
        zip_writer
            .write_all(&data)
            .map_err(|err| format!("write {name} failed: {err}"))?;
    }

    zip_writer
        .finish()
        .map_err(|err| format!("finish ZIP failed: {err}"))?;
    Ok(())
}

fn repack_as_xlsm<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    names: &[String],
    content_types: Option<&str>,
    output: &Path,
) -> Result<(), String> {
    if !names.iter().any(|name| name == "xl/vbaProject.bin") {
        return Err("cannot repack: xl/vbaProject.bin was not found".to_string());
    }
    if content_types.is_none() {
        return Err("cannot repack: [Content_Types].xml was not found".to_string());
    }
    if output.exists() {
        return Err(format!("output already exists: {}", output.display()));
    }

    let mut entries = Vec::new();
    for name in names {
        let mut entry = archive
            .by_name(name)
            .map_err(|err| format!("read {name} failed: {err}"))?;
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .map_err(|err| format!("read {name} failed: {err}"))?;

        let mut options = SimpleFileOptions::default()
            .compression_method(entry.compression())
            .unix_permissions(entry.unix_mode().unwrap_or(0o644));

        if name == "[Content_Types].xml" {
            let patched = patch_content_types(&String::from_utf8_lossy(&data))?;
            data = patched.into_bytes();
            options = options.compression_method(CompressionMethod::Deflated);
        }

        entries.push((name.clone(), data, options));
    }

    let out_file =
        File::create(output).map_err(|err| format!("create output failed: {err}"))?;
    let writer = BufWriter::new(out_file);
    let mut zip_writer = ZipWriter::new(writer);

    for (name, data, options) in entries {
        zip_writer
            .start_file(name.as_str(), options)
            .map_err(|err| format!("write {name} failed: {err}"))?;
        zip_writer
            .write_all(&data)
            .map_err(|err| format!("write {name} failed: {err}"))?;
    }

    zip_writer
        .finish()
        .map_err(|err| format!("finish ZIP failed: {err}"))?;
    Ok(())
}

fn patch_content_types(xml: &str) -> Result<String, String> {
    if !xml.contains("PartName=\"/xl/workbook.xml\"") {
        return Err("[Content_Types].xml has no /xl/workbook.xml override".to_string());
    }

    let mut patched = if xml.contains(&format!("ContentType=\"{MACRO_MAIN_CT}\"")) {
        xml.to_string()
    } else {
        replace_workbook_content_type(xml)?
    };

    if !patched.contains("PartName=\"/xl/vbaProject.bin\"") && !patched.contains(VBA_CT) {
        let override_entry = format!(
            "<Override PartName=\"/xl/vbaProject.bin\" ContentType=\"{VBA_CT}\"/>"
        );
        patched = patched.replacen("</Types>", &format!("{override_entry}</Types>"), 1);
    }

    Ok(patched)
}

fn replace_workbook_content_type(xml: &str) -> Result<String, String> {
    let marker = "PartName=\"/xl/workbook.xml\"";
    let position = xml
        .find(marker)
        .ok_or_else(|| "[Content_Types].xml has no /xl/workbook.xml override".to_string())?;
    let rest = &xml[position..];
    let content_marker = "ContentType=\"";
    let relative = rest
        .find(content_marker)
        .ok_or_else(|| "workbook override has no ContentType".to_string())?;
    let start = position + relative + content_marker.len();
    let end = xml[start..]
        .find('"')
        .ok_or_else(|| "workbook ContentType is unterminated".to_string())?
        + start;

    Ok(format!(
        "{}{}{}",
        &xml[..start],
        MACRO_MAIN_CT,
        &xml[end..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn zip_buffer(entries: &[(&str, &str)]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn inspect_detects_vba_parts_in_wps_macro_xlsx() {
        let data = zip_buffer(&[
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
</Types>"#,
            ),
            ("xl/workbook.xml", r#"<workbook/>"#),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Target="vbaProject.bin"/></Relationships>"#,
            ),
            ("xl/vbaProject.bin", "VBA test data"),
        ]);
        let mut archive = ZipArchive::new(Cursor::new(data)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect();

        let report = inspect(
            &names,
            Some(
                r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
            ),
            None,
            &mut archive,
        );

        assert!(report.has_vba_project);
        assert!(report.has_vba_relationship);
        assert_eq!(
            report.workbook_content_type.as_deref(),
            Some(NORMAL_MAIN_CT)
        );
    }

    #[test]
    fn patch_content_types_converts_workbook_to_macro_enabled() {
        let xml = r#"<Types>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
</Types>"#;

        let patched = patch_content_types(xml).unwrap();

        assert!(patched.contains(&format!("ContentType=\"{MACRO_MAIN_CT}\"")));
        assert!(patched.contains(&format!("ContentType=\"{VBA_CT}\"")));
        assert!(!patched.contains(NORMAL_MAIN_CT));
    }
}
