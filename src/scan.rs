use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jclassfile::class_file;
use jclassfile::constant_pool::ConstantPool;
use serde_json::Value;
use serde_sarif::sarif::{Artifact, ArtifactLocation, ArtifactRoles};
use zip::ZipArchive;

use crate::ir::{BasicBlock, CallKind, CallSite, Class, Instruction, InstructionKind, Method};

/// Snapshot of parsed artifacts, classes, and counts for a scan.
pub(crate) struct ScanOutput {
    pub(crate) artifacts: Vec<Artifact>,
    pub(crate) class_count: usize,
    pub(crate) classes: Vec<Class>,
}

pub(crate) fn scan_inputs(input: &Path, classpath: &[PathBuf]) -> Result<ScanOutput> {
    let mut artifacts = Vec::new();
    let mut class_count = 0;
    let mut classes = Vec::new();

    scan_path(
        input,
        true,
        true,
        &mut artifacts,
        &mut class_count,
        &mut classes,
    )?;

    // Keep deterministic ordering by sorting classpath entries and directory listings.
    let mut classpath_entries = classpath.to_vec();
    classpath_entries.sort_by(|a, b| path_key(a).cmp(&path_key(b)));

    if is_jar_path(input) {
        classpath_entries.extend(manifest_classpath(input)?);
    }

    let expanded = expand_classpath(classpath_entries)?;
    for entry in expanded {
        if entry == input {
            continue;
        }
        scan_path(
            &entry,
            false,
            true,
            &mut artifacts,
            &mut class_count,
            &mut classes,
        )?;
    }

    Ok(ScanOutput {
        artifacts,
        class_count,
        classes,
    })
}

fn scan_path(
    path: &Path,
    is_input: bool,
    strict: bool,
    artifacts: &mut Vec<Artifact>,
    class_count: &mut usize,
    classes: &mut Vec<Class>,
) -> Result<()> {
    if path.is_dir() {
        scan_dir(path, artifacts, class_count, classes)?;
        return Ok(());
    }

    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let roles = if is_input {
        Some(vec![serde_json::to_value(ArtifactRoles::AnalysisTarget)
            .expect("serialize artifact role")])
    } else {
        None
    };

    match extension {
        "class" => scan_class_file(path, roles, artifacts, class_count, classes),
        "jar" => scan_jar_file(path, roles, artifacts, class_count, classes),
        _ => {
            if strict {
                anyhow::bail!("unsupported input file: {}", path.display())
            } else {
                Ok(())
            }
        }
    }
}

fn scan_dir(
    path: &Path,
    artifacts: &mut Vec<Artifact>,
    class_count: &mut usize,
    classes: &mut Vec<Class>,
) -> Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)
        .with_context(|| format!("failed to read directory {}", path.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read entry under {}", path.display()))?;
        entries.push(entry.path());
    }

    entries.sort_by(|a, b| path_key(a).cmp(&path_key(b)));

    for entry in entries {
        if entry.is_dir() {
            scan_dir(&entry, artifacts, class_count, classes)?;
        } else {
            scan_path(&entry, false, false, artifacts, class_count, classes)?;
        }
    }

    Ok(())
}

fn scan_class_file(
    path: &Path,
    roles: Option<Vec<Value>>,
    artifacts: &mut Vec<Artifact>,
    class_count: &mut usize,
    classes: &mut Vec<Class>,
) -> Result<()> {
    let data = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed =
        parse_class_bytes(&data).with_context(|| format!("failed to parse {}", path.display()))?;
    *class_count += 1;

    let artifact_index = push_path_artifact(path, roles, data.len() as u64, None, artifacts)?;
    classes.push(Class {
        name: parsed.name,
        super_name: parsed.super_name,
        referenced_classes: parsed.referenced_classes,
        methods: parsed.methods,
        artifact_index,
    });
    Ok(())
}

fn scan_jar_file(
    path: &Path,
    roles: Option<Vec<Value>>,
    artifacts: &mut Vec<Artifact>,
    class_count: &mut usize,
    classes: &mut Vec<Class>,
) -> Result<()> {
    let file = fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("failed to read {}", path.display()))?;

    let jar_len = fs::metadata(path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .len();
    let jar_index = push_path_artifact(path, roles, jar_len, None, artifacts)?;

    let mut entry_names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.ends_with(".class") && !name.ends_with("module-info.class") {
            entry_names.push(name);
        }
    }

    entry_names.sort();

    for name in entry_names {
        let mut entry = archive
            .by_name(&name)
            .with_context(|| format!("failed to read {}:{}", path.display(), name))?;
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .with_context(|| format!("failed to read {}:{}", path.display(), name))?;
        let parsed = parse_class_bytes(&data)
            .with_context(|| format!("failed to parse {}:{}", path.display(), name))?;
        *class_count += 1;

        let entry_uri = jar_entry_uri(path, &name);
        let artifact_index =
            push_artifact(entry_uri, entry.size(), Some(jar_index), None, artifacts);
        classes.push(Class {
            name: parsed.name,
            super_name: parsed.super_name,
            referenced_classes: parsed.referenced_classes,
            methods: parsed.methods,
            artifact_index,
        });
    }

    Ok(())
}

/// Push a path-based artifact and return its index for parent linkage (e.g., JAR entries).
fn push_path_artifact(
    path: &Path,
    roles: Option<Vec<Value>>,
    len: u64,
    parent_index: Option<i64>,
    artifacts: &mut Vec<Artifact>,
) -> Result<i64> {
    let uri = path_to_uri(path);
    Ok(push_artifact(uri, len, parent_index, roles, artifacts))
}

fn push_artifact(
    uri: String,
    len: u64,
    parent_index: Option<i64>,
    roles: Option<Vec<Value>>,
    artifacts: &mut Vec<Artifact>,
) -> i64 {
    let location = ArtifactLocation::builder().uri(uri).build();
    let artifact = match (parent_index, roles) {
        (Some(parent_index), Some(roles)) => Artifact::builder()
            .location(location)
            .length(len as i64)
            .parent_index(parent_index)
            .roles(roles)
            .build(),
        (Some(parent_index), None) => Artifact::builder()
            .location(location)
            .length(len as i64)
            .parent_index(parent_index)
            .build(),
        (None, Some(roles)) => Artifact::builder()
            .location(location)
            .length(len as i64)
            .roles(roles)
            .build(),
        (None, None) => Artifact::builder()
            .location(location)
            .length(len as i64)
            .build(),
    };
    let index = artifacts.len() as i64;
    artifacts.push(artifact);
    index
}

fn path_to_uri(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn jar_entry_uri(jar_path: &Path, entry_name: &str) -> String {
    format!("jar:{}!/{}", jar_path.to_string_lossy(), entry_name)
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn expand_classpath(initial: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut queue = VecDeque::new();
    let mut initial_sorted = initial;
    initial_sorted.sort_by(|a, b| path_key(a).cmp(&path_key(b)));
    for entry in initial_sorted {
        queue.push_back(entry);
    }

    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    while let Some(entry) = queue.pop_front() {
        let key = path_key(&entry);
        if !seen.insert(key) {
            continue;
        }
        if !entry.exists() {
            anyhow::bail!("classpath entry not found: {}", entry.display());
        }
        result.push(entry.clone());
        if is_jar_path(&entry) {
            let mut referenced = manifest_classpath(&entry)?;
            referenced.sort_by(|a, b| path_key(a).cmp(&path_key(b)));
            for item in referenced {
                queue.push_back(item);
            }
        }
    }

    Ok(result)
}

fn manifest_classpath(path: &Path) -> Result<Vec<PathBuf>> {
    let file = fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("failed to read {}", path.display()))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if entry.name() != "META-INF/MANIFEST.MF" {
            continue;
        }
        let mut content = String::new();
        entry
            .read_to_string(&mut content)
            .with_context(|| format!("failed to read {}", entry.name()))?;
        return Ok(parse_manifest_classpath(path, &content));
    }

    Ok(Vec::new())
}

fn parse_manifest_classpath(jar_path: &Path, content: &str) -> Vec<PathBuf> {
    let mut class_path = None;
    let mut current_key = None;
    let mut current_value = String::new();

    for raw_line in content.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with(' ') {
            if current_key.is_some() {
                current_value.push_str(&line[1..]);
            }
            continue;
        }

        if let Some(key) = current_key.take() {
            if key == "Class-Path" {
                class_path = Some(current_value.clone());
            }
            current_value.clear();
        }

        if let Some((key, value)) = line.split_once(':') {
            current_key = Some(key.trim().to_string());
            current_value.push_str(value.trim_start());
        }
    }

    if let Some(key) = current_key.take() {
        if key == "Class-Path" {
            class_path = Some(current_value.clone());
        }
    }

    let Some(class_path) = class_path else {
        return Vec::new();
    };

    let base_dir = jar_path.parent().unwrap_or_else(|| Path::new(""));
    class_path
        .split_whitespace()
        .map(|entry| {
            let entry_path = PathBuf::from(entry);
            if entry_path.is_absolute() {
                entry_path
            } else {
                base_dir.join(entry_path)
            }
        })
        .collect()
}

fn is_jar_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("jar"))
        .unwrap_or(false)
}

/// Parsed class data extracted from class file bytes.
/// Parsed class data extracted from class file bytes.
struct ParsedClass {
    name: String,
    super_name: Option<String>,
    referenced_classes: Vec<String>,
    methods: Vec<Method>,
}

fn parse_class_bytes(data: &[u8]) -> Result<ParsedClass> {
    let class_file =
        class_file::parse(data).context("failed to parse class file bytes")?;
    let constant_pool = class_file.constant_pool();
    let class_name = resolve_class_name(constant_pool, class_file.this_class())
        .context("resolve class name")?;
    let super_name = if class_file.super_class() == 0 {
        None
    } else {
        Some(
            resolve_class_name(constant_pool, class_file.super_class())
                .context("resolve super class name")?,
        )
    };

    let mut referenced = std::collections::BTreeSet::new();
    for entry in constant_pool {
        if let ConstantPool::Class { name_index } = entry {
            let name = resolve_utf8(constant_pool, *name_index)
                .context("resolve referenced class name")?;
            if let Some(normalized) = normalize_class_name(&name) {
                referenced.insert(normalized);
            }
        }
    }
    referenced.remove(&class_name);

    let methods = parse_methods(constant_pool, class_file.methods())
        .context("parse method bytecode")?;

    Ok(ParsedClass {
        name: class_name,
        super_name,
        referenced_classes: referenced.into_iter().collect(),
        methods,
    })
}

fn resolve_class_name(constant_pool: &[ConstantPool], class_index: u16) -> Result<String> {
    let entry = constant_pool
        .get(class_index as usize)
        .context("missing class entry")?;
    match entry {
        ConstantPool::Class { name_index } => resolve_utf8(constant_pool, *name_index),
        _ => anyhow::bail!("unexpected class entry"),
    }
}

fn resolve_utf8(constant_pool: &[ConstantPool], index: u16) -> Result<String> {
    let entry = constant_pool
        .get(index as usize)
        .context("missing utf8 entry")?;
    match entry {
        ConstantPool::Utf8 { value } => Ok(value.clone()),
        _ => anyhow::bail!("unexpected utf8 entry"),
    }
}

fn normalize_class_name(raw: &str) -> Option<String> {
    if !raw.starts_with('[') {
        return Some(raw.to_string());
    }
    let mut slice = raw;
    while let Some(rest) = slice.strip_prefix('[') {
        slice = rest;
    }
    if let Some(class_name) = slice.strip_prefix('L').and_then(|s| s.strip_suffix(';')) {
        return Some(class_name.to_string());
    }
    None
}

fn parse_methods(constant_pool: &[ConstantPool], methods: &[jclassfile::methods::MethodInfo]) -> Result<Vec<Method>> {
    let mut parsed = Vec::new();
    for method in methods {
        let name = resolve_utf8(constant_pool, method.name_index())
            .context("resolve method name")?;
        let descriptor = resolve_utf8(constant_pool, method.descriptor_index())
            .context("resolve method descriptor")?;
        let code = method
            .attributes()
            .iter()
            .find_map(|attribute| match attribute {
                jclassfile::attributes::Attribute::Code { code, .. } => Some(code),
                _ => None,
            });
        let Some(code) = code else {
            continue;
        };
        let (instructions, calls) =
            parse_bytecode(code, constant_pool).context("parse bytecode")?;
        let block = BasicBlock {
            start_offset: 0,
            end_offset: code.len() as u32,
            instructions,
        };
        parsed.push(Method {
            name,
            descriptor,
            blocks: vec![block],
            calls,
        });
    }
    Ok(parsed)
}

fn parse_bytecode(code: &[u8], constant_pool: &[ConstantPool]) -> Result<(Vec<Instruction>, Vec<CallSite>)> {
    let mut instructions = Vec::new();
    let mut calls = Vec::new();
    let mut offset = 0usize;
    while offset < code.len() {
        let opcode = code[offset];
        let start_offset = offset as u32;
        let length = opcode_length(code, offset)?;
        if length == 0 || offset + length > code.len() {
            anyhow::bail!("invalid bytecode length at offset {}", offset);
        }
        let kind = match opcode {
            0xb6 | 0xb7 | 0xb8 | 0xb9 => {
                let method_index = read_u16(code, offset + 1)?;
                let method_ref = resolve_method_ref(constant_pool, method_index)
                    .context("resolve method ref")?;
                let call_kind = match opcode {
                    0xb6 => CallKind::Virtual,
                    0xb7 => CallKind::Special,
                    0xb8 => CallKind::Static,
                    0xb9 => CallKind::Interface,
                    _ => CallKind::Virtual,
                };
                let call = CallSite {
                    owner: method_ref.owner,
                    name: method_ref.name,
                    descriptor: method_ref.descriptor,
                    kind: call_kind,
                    offset: start_offset,
                };
                calls.push(call.clone());
                InstructionKind::Invoke(call)
            }
            _ => InstructionKind::Other(opcode),
        };

        instructions.push(Instruction {
            offset: start_offset,
            kind,
        });
        offset += length;
    }
    Ok((instructions, calls))
}

/// Resolved constant pool method reference.
struct MethodRef {
    owner: String,
    name: String,
    descriptor: String,
}

fn resolve_method_ref(constant_pool: &[ConstantPool], index: u16) -> Result<MethodRef> {
    let entry = constant_pool
        .get(index as usize)
        .context("missing method ref entry")?;
    let (class_index, name_and_type_index) = match entry {
        ConstantPool::Methodref {
            class_index,
            name_and_type_index,
        } => (*class_index, *name_and_type_index),
        ConstantPool::InterfaceMethodref {
            class_index,
            name_and_type_index,
        } => (*class_index, *name_and_type_index),
        _ => anyhow::bail!("unexpected method ref entry"),
    };
    let owner = resolve_class_name(constant_pool, class_index).context("resolve owner")?;
    let (name_index, descriptor_index) =
        resolve_name_and_type(constant_pool, name_and_type_index)?;
    let name = resolve_utf8(constant_pool, name_index).context("resolve method name")?;
    let descriptor =
        resolve_utf8(constant_pool, descriptor_index).context("resolve method descriptor")?;
    Ok(MethodRef {
        owner,
        name,
        descriptor,
    })
}

fn resolve_name_and_type(
    constant_pool: &[ConstantPool],
    index: u16,
) -> Result<(u16, u16)> {
    let entry = constant_pool
        .get(index as usize)
        .context("missing name and type entry")?;
    match entry {
        ConstantPool::NameAndType {
            name_index,
            descriptor_index,
        } => Ok((*name_index, *descriptor_index)),
        _ => anyhow::bail!("unexpected name and type entry"),
    }
}

fn opcode_length(code: &[u8], offset: usize) -> Result<usize> {
    let opcode = code[offset];
    let length = match opcode {
        0x00..=0x0f => 1,
        0x10 => 2,
        0x11 => 3,
        0x12 => 2,
        0x13 | 0x14 => 3,
        0x15..=0x19 => 2,
        0x1a..=0x35 => 1,
        0x36..=0x3a => 2,
        0x3b..=0x4e => 1,
        0x4f..=0x56 => 1,
        0x57..=0x5f => 1,
        0x60..=0x83 => 1,
        0x84 => 3,
        0x85..=0x98 => 1,
        0x99..=0xa6 => 3,
        0xa7 | 0xa8 => 3,
        0xa9 => 2,
        0xaa => tableswitch_length(code, offset)?,
        0xab => lookupswitch_length(code, offset)?,
        0xac..=0xb1 => 1,
        0xb2..=0xb5 => 3,
        0xb6..=0xb8 => 3,
        0xb9 | 0xba => 5,
        0xbb => 3,
        0xbc => 2,
        0xbd => 3,
        0xbe | 0xbf => 1,
        0xc0 | 0xc1 => 3,
        0xc2 | 0xc3 => 1,
        0xc4 => wide_length(code, offset)?,
        0xc5 => 4,
        0xc6 | 0xc7 => 3,
        0xc8 | 0xc9 => 5,
        0xca => 1,
        0xfe | 0xff => 1,
        _ => anyhow::bail!("unsupported opcode 0x{:02x}", opcode),
    };
    Ok(length)
}

fn tableswitch_length(code: &[u8], offset: usize) -> Result<usize> {
    let padding = padding(offset);
    let base = offset + 1 + padding;
    let low = read_u32(code, base + 4)?;
    let high = read_u32(code, base + 8)?;
    let count = high
        .checked_sub(low)
        .and_then(|v| v.checked_add(1))
        .context("invalid tableswitch range")?;
    Ok(1 + padding + 12 + (count as usize) * 4)
}

fn lookupswitch_length(code: &[u8], offset: usize) -> Result<usize> {
    let padding = padding(offset);
    let base = offset + 1 + padding;
    let npairs = read_u32(code, base + 4)?;
    Ok(1 + padding + 8 + (npairs as usize) * 8)
}

fn wide_length(code: &[u8], offset: usize) -> Result<usize> {
    let opcode = code
        .get(offset + 1)
        .copied()
        .context("missing wide opcode")?;
    if opcode == 0x84 {
        Ok(6)
    } else {
        Ok(4)
    }
}

fn padding(offset: usize) -> usize {
    (4 - ((offset + 1) % 4)) % 4
}

fn read_u16(code: &[u8], offset: usize) -> Result<u16> {
    let slice = code
        .get(offset..offset + 2)
        .context("bytecode u16 out of bounds")?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32(code: &[u8], offset: usize) -> Result<u32> {
    let slice = code
        .get(offset..offset + 4)
        .context("bytecode u32 out of bounds")?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::io::Write;
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;
    use zip::ZipArchive;

    #[test]
    fn scan_inputs_rejects_invalid_class_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rtro-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let class_path = temp_dir.join("bad.class");
        fs::write(&class_path, b"nope").expect("write test class");

        let result = scan_inputs(&class_path, &[]);

        assert!(result.is_err());
        fs::remove_dir_all(&temp_dir).expect("cleanup temp dir");
    }

    #[test]
    fn scan_inputs_accepts_valid_jar() {
        let jar_path = jspecify_jar_path().expect("download jar");
        let result = scan_inputs(&jar_path, &[]).expect("scan jar");

        assert!(result.class_count > 0);
        assert!(!result.artifacts.is_empty());
        let first_uri = result
            .artifacts
            .first()
            .and_then(|artifact| artifact.location.as_ref())
            .and_then(|location| location.uri.as_ref())
            .cloned()
            .expect("artifact uri");
        assert!(first_uri.ends_with("jspecify-1.0.0.jar"));
    }

    #[test]
    fn scan_inputs_accepts_valid_class_file() {
        let jar_path = jspecify_jar_path().expect("download jar");
        let class_bytes = extract_first_class(&jar_path).expect("extract class");

        let temp_dir = std::env::temp_dir().join(format!(
            "rtro-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let class_path = temp_dir.join("Sample.class");
        fs::write(&class_path, class_bytes).expect("write class file");

        let result = scan_inputs(&class_path, &[]).expect("scan class");

        assert_eq!(result.class_count, 1);
        assert_eq!(result.artifacts.len(), 1);
        fs::remove_dir_all(&temp_dir).expect("cleanup temp dir");
    }

    #[test]
    fn scan_inputs_resolves_manifest_classpath() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rtro-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let dep_path = temp_dir.join("dep.jar");
        create_manifest_jar(&dep_path, None).expect("create dep jar");
        let jar_path = temp_dir.join("main.jar");
        create_manifest_jar(&jar_path, Some("dep.jar")).expect("create main jar");

        let result = scan_inputs(&jar_path, &[]);

        assert!(result.is_ok());
        fs::remove_dir_all(&temp_dir).expect("cleanup temp dir");
    }

    #[test]
    fn scan_inputs_errors_on_missing_manifest_classpath_entry() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rtro-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).expect("create temp dir");

        let jar_path = temp_dir.join("main.jar");
        create_manifest_jar(&jar_path, Some("missing.jar")).expect("create main jar");

        let result = scan_inputs(&jar_path, &[]);

        assert!(result.is_err());
        fs::remove_dir_all(&temp_dir).expect("cleanup temp dir");
    }

    fn extract_first_class(jar_path: &Path) -> Result<Vec<u8>> {
        let file =
            fs::File::open(jar_path).with_context(|| format!("open {}", jar_path.display()))?;
        let mut archive =
            ZipArchive::new(file).with_context(|| format!("read {}", jar_path.display()))?;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .with_context(|| format!("read {}:{}", jar_path.display(), index))?;
            if entry.is_dir()
                || !entry.name().ends_with(".class")
                || entry.name().ends_with("module-info.class")
            {
                continue;
            }
            let mut data = Vec::new();
            entry.read_to_end(&mut data).context("read class bytes")?;
            return Ok(data);
        }

        anyhow::bail!("no class entry found in {}", jar_path.display());
    }

    fn create_manifest_jar(path: &Path, class_path: Option<&str>) -> Result<()> {
        let file = fs::File::create(path).with_context(|| format!("create {}", path.display()))?;
        let mut writer = zip::ZipWriter::new(file);
        let mut manifest = String::from("Manifest-Version: 1.0\n");
        if let Some(class_path) = class_path {
            manifest.push_str(&format!("Class-Path: {class_path}\n"));
        }
        manifest.push('\n');
        writer
            .start_file("META-INF/MANIFEST.MF", SimpleFileOptions::default())
            .context("start manifest entry")?;
        writer
            .write_all(manifest.as_bytes())
            .context("write manifest")?;
        writer.finish().context("finish jar")?;
        Ok(())
    }

    fn jspecify_jar_path() -> Result<PathBuf> {
        static JAR_PATH: OnceLock<PathBuf> = OnceLock::new();
        if let Some(path) = JAR_PATH.get() {
            return Ok(path.clone());
        }
        let jar_path = download_jspecify_jar()?;
        let _ = JAR_PATH.set(jar_path.clone());
        Ok(jar_path)
    }

    fn download_jspecify_jar() -> Result<PathBuf> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-fixtures");
        fs::create_dir_all(&dir).context("create fixture directory")?;
        let jar_path = dir.join("jspecify-1.0.0.jar");
        if jar_path.exists() {
            return Ok(jar_path);
        }

        let url = "https://repo.maven.apache.org/maven2/org/jspecify/jspecify/1.0.0/jspecify-1.0.0.jar";
        let mut response = ureq::get(url)
            .call()
            .context("download jspecify jar")?;
        if response.status().as_u16() >= 400 {
            anyhow::bail!(
                "failed to download jspecify jar: HTTP {}",
                response.status()
            );
        }

        let mut reader = response.body_mut().as_reader();
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .context("read jspecify jar")?;
        fs::write(&jar_path, bytes).context("write jspecify jar")?;

        Ok(jar_path)
    }
}
