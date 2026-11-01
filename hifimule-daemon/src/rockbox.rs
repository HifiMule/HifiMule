//! Rockbox tagcache files. Format authority: Rockbox apps/tagcache.{c,h}.
use anyhow::{Context, Result, bail, ensure};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_FILE: usize = 128 * 1024 * 1024;
const MAX_TOTAL: usize = 512 * 1024 * 1024;
// Rockbox TAG_MAXLEN = MAX_PATH (260) * 2; the codec keeps its wider legacy budget.
const MAX_METADATA_TEXT: usize = 520;
const STRINGS: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12];
const MASTER: &str = "database_idx.tcd";
const JOURNAL: &str = ".hifimule-rbdb-journal.json";
const BACKUP: &str = ".hifimule-rbdb-backup";
const OWNERSHIP: &str = ".hifimule-rbdb-managed.json";

fn tag_name(tag: usize) -> &'static str {
    match tag {
        0 => "artist",
        1 => "album",
        2 => "genre",
        3 => "title",
        4 => "filename",
        5 => "composer",
        6 => "comment",
        7 => "album artist",
        8 => "grouping",
        12 => "canonical artist",
        _ => "unknown tag",
    }
}

fn bounded_metadata(value: &str) -> &str {
    let value = value.split('\0').next().unwrap_or_default();
    let mut end = value.len().min(MAX_METADATA_TEXT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub import_ratings: bool,
    pub format: String,
    pub path_prefix: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            import_ratings: false,
            format: "4.0".into(),
            path_prefix: "/".into(),
        }
    }
}
impl Settings {
    pub fn active(&self) -> bool {
        self.enabled || self.import_ratings
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.format.as_str(), "4.0" | "3.15"),
            "Rockbox format must be 4.0 or 3.15"
        );
        virtual_path(&self.path_prefix)?;
        ensure!(
            self.path_prefix.len() <= 512,
            "Rockbox path prefix is too long"
        );
        Ok(())
    }
}
fn virtual_path(path: &str) -> Result<String> {
    ensure!(
        path.starts_with('/')
            && !path.contains(['\\', ':', '\0'])
            && path.split('/').all(|s| s != "." && s != ".."),
        "Invalid Rockbox path"
    );
    Ok(format!(
        "/{}",
        path.split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    ))
}
fn firmware_path(prefix: &str, local: &str) -> Result<String> {
    Ok(format!(
        "{}/{}",
        virtual_path(prefix)?.trim_end_matches('/'),
        device_path(local)?
    ))
}
fn device_path(path: &str) -> Result<String> {
    let normalized = path.replace('\\', "/");
    ensure!(
        !normalized.starts_with('/')
            && !normalized.contains([':', '\0'])
            && normalized
                .split('/')
                .all(|s| !s.is_empty() && s != "." && s != ".."),
        "Invalid managed audio path"
    );
    Ok(normalized)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    // Existing tagcache metadata may retain legacy encodings; only filenames are UTF-8.
    strings: [Vec<u8>; 23],
    numbers: [i32; 23],
    flags: i32,
}
impl Default for Entry {
    fn default() -> Self {
        Self {
            strings: std::array::from_fn(|_| Vec::new()),
            numbers: [0; 23],
            flags: 0,
        }
    }
}
impl Entry {
    fn path(&self) -> &str {
        std::str::from_utf8(&self.strings[4]).expect("Rockbox filename validated before use")
    }
}
#[derive(Clone, Debug)]
pub struct RbDatabase {
    version: u8,
    big: bool,
    serial: i32,
    commit: i32,
    entries: Vec<Entry>,
}
fn physical(tag: usize, version: u8) -> usize {
    if version == 15 && tag > 12 {
        tag - 1
    } else {
        tag
    }
}
fn word(bytes: &[u8], at: usize, big: bool) -> Result<i32> {
    let bytes: [u8; 4] = bytes
        .get(at..at + 4)
        .context("Truncated Rockbox word")?
        .try_into()?;
    Ok(if big {
        i32::from_be_bytes(bytes)
    } else {
        i32::from_le_bytes(bytes)
    })
}
fn put(out: &mut Vec<u8>, value: i32, big: bool) {
    out.extend_from_slice(&if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    });
}
fn size(value: usize) -> Result<i32> {
    i32::try_from(value).context("Rockbox database exceeds format limits")
}
fn charge(used: &mut usize, bytes: usize, limit: usize) -> Result<()> {
    *used = used
        .checked_add(bytes)
        .context("Rockbox allocation overflow")?;
    ensure!(
        *used <= limit,
        "Rockbox database exceeds memory/output limit"
    );
    Ok(())
}
fn header(bytes: &[u8], version: u8, big: bool) -> Result<(usize, usize)> {
    ensure!(
        bytes.len() >= 12 && bytes.len() <= MAX_FILE,
        "Invalid Rockbox file size"
    );
    ensure!(
        word(bytes, 0, big)? == 0x54434800 + version as i32,
        "Mixed Rockbox database formats"
    );
    let data = usize::try_from(word(bytes, 4, big)?).context("Negative Rockbox data size")?;
    let count = usize::try_from(word(bytes, 8, big)?).context("Negative Rockbox entry count")?;
    ensure!(
        data <= MAX_TOTAL && count <= 1_000_000,
        "Rockbox database limit exceeded"
    );
    Ok((data, count))
}
impl RbDatabase {
    fn empty(version: u8, big: bool) -> Self {
        Self {
            version,
            big,
            serial: 0,
            commit: 1,
            entries: Vec::new(),
        }
    }
    pub fn decode(files: &BTreeMap<String, Vec<u8>>) -> Result<Self> {
        Self::decode_with_budget(files, MAX_TOTAL)
    }
    fn decode_with_budget(files: &BTreeMap<String, Vec<u8>>, budget: usize) -> Result<Self> {
        let mut allocated = 0;
        for bytes in files.values() {
            charge(&mut allocated, bytes.len(), budget)?;
        }
        ensure!(
            files.values().map(Vec::len).sum::<usize>() <= MAX_TOTAL,
            "Rockbox database exceeds memory limit"
        );
        let master = files.get(MASTER).context("Missing Rockbox master index")?;
        let le = word(master, 0, false)?;
        let (magic, big) = if matches!(le, 0x5443480f | 0x54434810) {
            (le, false)
        } else {
            (word(master, 0, true)?, true)
        };
        ensure!(
            matches!(magic, 0x5443480f | 0x54434810),
            "Unsupported Rockbox database version"
        );
        let version = (magic & 255) as u8;
        let (master_data, count) = header(master, version, big)?;
        let slots = if version == 15 { 22 } else { 23 };
        ensure!(
            master.len() == 24 + count * (slots + 1) * 4,
            "Invalid Rockbox master length"
        );
        ensure!(
            word(master, 20, big)? == 0,
            "Rockbox database has a pending commit"
        );
        charge(
            &mut allocated,
            count
                .checked_mul(std::mem::size_of::<Entry>() + 64)
                .context("Rockbox row allocation overflow")?,
            budget,
        )?;
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let mut entry = Entry::default();
            for tag in 0..23 {
                if tag == 12 && version == 15 {
                    continue;
                }
                entry.numbers[tag] = word(
                    master,
                    24 + index * (slots + 1) * 4 + physical(tag, version) * 4,
                    big,
                )?;
            }
            entry.flags = word(master, 24 + index * (slots + 1) * 4 + slots * 4, big)?;
            ensure!(
                entry.flags & !31 == 0 && entry.flags & 2 == 0,
                "Unsupported Rockbox index flags/dircache pointer"
            );
            entries.push(entry);
        }
        let mut expected_data = master.len();
        for tag in STRINGS
            .into_iter()
            .filter(|tag| version == 16 || *tag != 12)
        {
            let bytes = files
                .get(&format!("database_{}.tcd", physical(tag, version)))
                .context("Missing Rockbox string file")?;
            let (data, records) = header(bytes, version, big)?;
            if tag != 4 {
                expected_data += data;
            }
            ensure!(data == bytes.len() - 12, "Invalid Rockbox string data size");
            let mut offsets = HashMap::new();
            let mut at = 12;
            for _ in 0..records {
                let length = usize::try_from(word(bytes, at, big)?)
                    .context("Negative Rockbox string length")?;
                let owner = word(bytes, at + 4, big)?;
                ensure!(
                    length > 0 && length <= 8192 && at + 8 + length <= bytes.len(),
                    "Invalid Rockbox string record"
                );
                if tag != 4 {
                    ensure!(length % 8 == 0, "Invalid Rockbox string alignment");
                }
                let data = &bytes[at + 8..at + 8 + length];
                let end = data
                    .iter()
                    .position(|b| *b == 0)
                    .context("Unterminated Rockbox string")?;
                let value = &data[..end];
                if tag == 4 {
                    std::str::from_utf8(value).with_context(|| {
                        format!("Invalid Rockbox filename UTF-8 at offset {at}")
                    })?;
                }
                ensure!(
                    owner == -1 || (owner >= 0 && (owner as usize) < count),
                    "Invalid Rockbox string owner"
                );
                charge(&mut allocated, 64, budget)?;
                offsets.insert(at as i32, (value, owner));
                at += 8 + length;
            }
            ensure!(
                at == bytes.len(),
                "Trailing/truncated Rockbox string records"
            );
            for (index, entry) in entries.iter_mut().enumerate() {
                if entry.flags & 1 != 0 {
                    continue;
                }
                let (value, owner) = offsets
                    .get(&entry.numbers[tag])
                    .context("Invalid Rockbox string offset")?;
                ensure!(
                    if matches!(tag, 3 | 4) {
                        *owner == index as i32
                    } else {
                        *owner == -1
                    },
                    "Rockbox string owner mismatch"
                );
                charge(&mut allocated, value.len(), budget)?;
                entry.strings[tag] = (*value).to_vec();
            }
        }
        ensure!(
            master_data == expected_data,
            "Invalid Rockbox master data size"
        );
        entries.retain(|e| e.flags & 1 == 0);
        let mut paths = HashSet::new();
        for entry in &entries {
            virtual_path(entry.path())?;
            ensure!(paths.insert(entry.path()), "Duplicate Rockbox track path");
        }
        Ok(Self {
            version,
            big,
            serial: word(master, 12, big)?,
            commit: word(master, 16, big)?,
            entries,
        })
    }
    pub fn encode(&self) -> Result<BTreeMap<String, Vec<u8>>> {
        self.encode_with_limits(MAX_FILE, MAX_TOTAL)
    }
    fn encode_with_limits(
        &self,
        file_limit: usize,
        total_limit: usize,
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        ensure!(
            matches!(self.version, 15 | 16),
            "Unsupported Rockbox version"
        );
        let mut memory = self
            .entries
            .len()
            .checked_mul(std::mem::size_of::<Entry>() + 92)
            .context("Rockbox encoding allocation overflow")?;
        ensure!(
            memory <= MAX_TOTAL,
            "Rockbox encoding memory limit exceeded"
        );
        for entry in &self.entries {
            virtual_path(
                std::str::from_utf8(&entry.strings[4]).context("Invalid Rockbox filename UTF-8")?,
            )?;
            for value in &entry.strings {
                charge(&mut memory, value.len(), MAX_TOTAL)?;
            }
        }
        let slots = if self.version == 15 { 22 } else { 23 };
        let master_len = 24 + self.entries.len() * (slots + 1) * 4;
        ensure!(
            master_len <= file_limit && master_len <= total_limit,
            "Rockbox master output limit exceeded"
        );
        let mut output_bytes = master_len;
        let mut files = BTreeMap::new();
        let mut seeks = vec![[0i32; 23]; self.entries.len()];
        let mut total_payload = 0;
        for tag in STRINGS
            .into_iter()
            .filter(|t| self.version == 16 || *t != 12)
        {
            let mut records: Vec<(&[u8], Vec<usize>)> = Vec::new();
            if matches!(tag, 3 | 4) {
                records.extend(
                    self.entries
                        .iter()
                        .enumerate()
                        .map(|(i, e)| (e.strings[tag].as_slice(), vec![i])),
                );
            } else {
                let mut unique: BTreeMap<Vec<u8>, (&[u8], Vec<usize>)> = BTreeMap::new();
                for (i, e) in self.entries.iter().enumerate() {
                    let value = &e.strings[tag];
                    unique
                        .entry(value.iter().map(|b| b.to_ascii_lowercase()).collect())
                        .or_insert_with(|| (value.as_slice(), Vec::new()))
                        .1
                        .push(i);
                }
                records.extend(unique.into_values());
            }
            if tag != 4 {
                records.sort_by_key(|(v, _)| {
                    (
                        *v != b"<Untagged>",
                        v.iter().map(|b| b.to_ascii_lowercase()).collect::<Vec<_>>(),
                    )
                });
            }
            charge(&mut output_bytes, 12, total_limit)?;
            ensure!(12 <= file_limit, "Rockbox string output limit exceeded");
            let mut out = Vec::new();
            put(&mut out, 0x54434800 + self.version as i32, self.big);
            put(&mut out, 0, self.big);
            put(&mut out, size(records.len())?, self.big);
            for (value, owners) in records {
                ensure!(
                    !value.contains(&0) && value.len() < 8192,
                    "Invalid/oversized Rockbox {} ({} bytes) for {}",
                    tag_name(tag),
                    value.len(),
                    self.entries[owners[0]].path()
                );
                let offset = size(out.len())?;
                for i in &owners {
                    seeks[*i][tag] = offset;
                }
                let raw = value.len() + 1;
                let padded = if tag == 4 { raw } else { raw.div_ceil(8) * 8 };
                ensure!(
                    out.len() + 8 + padded <= file_limit,
                    "Rockbox string output limit exceeded"
                );
                charge(&mut output_bytes, 8 + padded, total_limit)?;
                put(&mut out, size(padded)?, self.big);
                put(
                    &mut out,
                    if matches!(tag, 3 | 4) {
                        size(owners[0])?
                    } else {
                        -1
                    },
                    self.big,
                );
                out.extend_from_slice(value);
                out.push(0);
                out.resize(out.len() + padded - raw, b'X');
            }
            let payload = out.len() - 12;
            if tag != 4 {
                total_payload += payload;
            }
            let data = if self.big {
                size(payload)?.to_be_bytes()
            } else {
                size(payload)?.to_le_bytes()
            };
            out[4..8].copy_from_slice(&data);
            ensure!(out.len() <= file_limit, "Rockbox string file exceeds limit");
            files.insert(format!("database_{}.tcd", physical(tag, self.version)), out);
        }
        let slots = if self.version == 15 { 22 } else { 23 };
        let mut out = Vec::new();
        for value in [
            0x54434800 + self.version as i32,
            size(24 + self.entries.len() * (slots + 1) * 4 + total_payload)?,
            size(self.entries.len())?,
            self.serial,
            self.commit,
            0,
        ] {
            put(&mut out, value, self.big);
        }
        for (index, e) in self.entries.iter().enumerate() {
            for tag in 0..23 {
                if tag == 12 && self.version == 15 {
                    continue;
                }
                put(
                    &mut out,
                    if STRINGS.contains(&tag) {
                        seeks[index][tag]
                    } else {
                        e.numbers[tag]
                    },
                    self.big,
                );
            }
            put(&mut out, e.flags & 8, self.big);
        }
        files.insert(MASTER.into(), out);
        ensure!(
            files.values().map(Vec::len).sum::<usize>() <= MAX_TOTAL,
            "Rockbox database exceeds limit"
        );
        Ok(files)
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        !std::fs::symlink_metadata(path).with_context(|| format!("Cannot inspect Rockbox file {}",path.display()))?.file_type().is_symlink(),
        "Rockbox file cannot be a symlink"
    );
    let mut bytes = Vec::new();
    std::fs::File::open(path).with_context(|| format!("Cannot open Rockbox file {}",path.display()))?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes).with_context(|| format!("Cannot read Rockbox file {}",path.display()))?;
    ensure!(bytes.len() <= MAX_FILE, "Rockbox file exceeds size limit");
    Ok(bytes)
}
fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
fn optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(Some(read_bounded(path)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Cannot inspect Rockbox file {}",path.display())),
    }
}
fn atomic_file(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context("No parent")?)
        .with_context(|| format!("Cannot stage Rockbox file {}",path.display()))?;
    file.write_all(data).with_context(|| format!("Cannot write Rockbox file {}",path.display()))?;
    file.as_file().sync_all().with_context(|| format!("Cannot flush Rockbox file {}",path.display()))?;
    file.persist(path).map_err(|e| anyhow::anyhow!(e.error))
        .with_context(|| format!("Cannot publish Rockbox file {}",path.display()))?;
    Ok(())
}
fn backup_file(path: &Path, data: &[u8]) -> Result<()> {
    let stage = path.with_file_name(format!(
        ".hifimule-rbdb-stage-{}",
        path.file_name()
            .context("Missing backup filename")?
            .to_string_lossy()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&stage).with_context(|| format!("Cannot stage Rockbox backup {}",path.display()))?;
    file.write_all(data).with_context(|| format!("Cannot write Rockbox backup {}",path.display()))?;
    file.sync_all().with_context(|| format!("Cannot flush Rockbox backup {}",path.display()))?;
    drop(file);
    std::fs::rename(stage, path).with_context(|| format!("Cannot publish Rockbox backup {}",path.display()))?;
    Ok(())
}
fn sync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    let _ = dir;
    Ok(())
}
fn db_filename(name: &str) -> bool {
    name == MASTER
        || name == "database_state.tcd"
        || STRINGS
            .iter()
            .any(|tag| name == format!("database_{tag}.tcd"))
}
#[derive(Serialize, Deserialize)]
struct Change {
    name: String,
    old: Option<String>,
    new: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    #[serde(default)]
    prepared: bool,
    committed: bool,
    changes: Vec<Change>,
}
fn cleanup_backup(dir: &Path, changes: &[Change]) -> Result<()> {
    let backup = dir.join(BACKUP);
    if !backup.exists() {
        return Ok(());
    }
    for change in changes {
        let p = backup.join(&change.name);
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        let stage = backup.join(format!(".hifimule-rbdb-stage-{}", change.name));
        if std::fs::symlink_metadata(&stage).is_ok() {
            std::fs::remove_file(stage)?;
        }
    }
    std::fs::remove_dir(backup)?;
    Ok(())
}
fn recover(dir: &Path) -> Result<()> {
    let Some(bytes) = optional_bytes(&dir.join(JOURNAL))? else {
        return Ok(());
    };
    let mut journal: Journal = serde_json::from_slice(&bytes)
        .context("Invalid Rockbox recovery journal; preserved for recovery")?;
    let backup = dir.join(BACKUP);
    ensure!(
        ((journal.committed || !journal.prepared) && !backup.exists())
            || (backup.is_dir() && !std::fs::symlink_metadata(&backup)?.file_type().is_symlink()),
        "Invalid Rockbox backup directory"
    );
    ensure!(
        journal.changes.len() <= 12 && journal.changes.iter().all(|c| db_filename(&c.name)),
        "Invalid Rockbox recovery filenames"
    );
    for change in &journal.changes {
        let current = optional_bytes(&dir.join(&change.name))?
            .as_deref()
            .map(hash);
        ensure!(
            current == change.old || current == change.new,
            "Rockbox file changed outside interrupted transaction: {}",
            change.name
        );
        if !journal.prepared {
            ensure!(
                current == change.old,
                "Rockbox changed during backup preparation"
            );
        }
        if journal.committed {
            ensure!(
                current == change.new,
                "Incomplete committed Rockbox database"
            );
        }
        if !journal.committed
            && let Some(old) = &change.old
        {
            if journal.prepared || backup.join(&change.name).exists() {
                ensure!(
                    hash(&read_bounded(&backup.join(&change.name))?) == *old,
                    "Rockbox recovery backup damaged"
                );
            }
        }
    }
    if !journal.committed {
        // Restore every string file before publishing the prior master.
        for change in journal.changes.iter().filter(|_| journal.prepared) {
            if change.old.is_some() {
                atomic_file(
                    &dir.join(&change.name),
                    &read_bounded(&backup.join(&change.name))?,
                )?;
            } else if dir.join(&change.name).exists() {
                std::fs::remove_file(dir.join(&change.name))?;
            }
        }
        sync_dir(dir)?;
        journal.committed = true;
        for change in &mut journal.changes {
            change.new = change.old.clone();
        }
        atomic_file(&dir.join(JOURNAL), &serde_json::to_vec(&journal)?)?;
        sync_dir(dir)?;
    }
    cleanup_backup(dir, &journal.changes)?;
    std::fs::remove_file(dir.join(JOURNAL))?;
    sync_dir(dir)?;
    Ok(())
}
fn publish(
    dir: &Path,
    files: &BTreeMap<String, Vec<u8>>,
    expected: Option<&BTreeMap<String, String>>,
) -> Result<()> {
    RbDatabase::decode(files)?;
    recover(dir)?;
    let backup = dir.join(BACKUP);
    ensure!(
        !backup.exists(),
        "An unjournaled Rockbox backup exists; preserve it for manual recovery"
    );
    let mut names: Vec<String> = files
        .keys()
        .filter(|n| n.as_str() != MASTER)
        .cloned()
        .collect();
    names.push("database_state.tcd".into());
    names.push(MASTER.into());
    let mut journal = Journal {
        prepared: false,
        committed: false,
        changes: Vec::new(),
    };
    for name in names {
        let old = optional_bytes(&dir.join(&name))?;
        if let Some(expected) = expected {
            ensure!(
                old.as_deref().map(hash) == expected.get(&name).cloned(),
                "Rockbox database changed since sync started"
            );
        }
        journal.changes.push(Change {
            name: name.clone(),
            old: old.as_deref().map(hash),
            new: files.get(&name).map(|b| hash(b)),
        });
    }
    atomic_file(&dir.join(JOURNAL), &serde_json::to_vec(&journal)?)?;
    sync_dir(dir)?;
    let prepared = (|| -> Result<()> {
        std::fs::create_dir(&backup)?;
        for change in &journal.changes {
            let old = optional_bytes(&dir.join(&change.name))?;
            ensure!(
                old.as_deref().map(hash) == change.old,
                "Rockbox file changed during backup preparation"
            );
            if let Some(bytes) = &old {
                backup_file(&backup.join(&change.name), bytes)?;
            }
        }
        sync_dir(&backup)?;
        journal.prepared = true;
        atomic_file(&dir.join(JOURNAL), &serde_json::to_vec(&journal)?)?;
        sync_dir(dir)?;
        Ok(())
    })();
    if let Err(error) = prepared {
        recover(dir).context(format!("Rockbox backup preparation failed: {error}"))?;
        return Err(error);
    }
    let result = (|| -> Result<()> {
        for change in &journal.changes {
            ensure!(
                optional_bytes(&dir.join(&change.name))?
                    .as_deref()
                    .map(hash)
                    == change.old,
                "Rockbox file changed during publication"
            );
            if let Some(bytes) = files.get(&change.name) {
                atomic_file(&dir.join(&change.name), bytes)?;
            } else if change.old.is_some() {
                std::fs::remove_file(dir.join(&change.name))?;
            }
        }
        sync_dir(dir)?;
        ensure!(
            load(dir)?
                .context("Missing published Rockbox database")?
                .encode()
                .is_ok(),
            "Rockbox publication verification failed"
        );
        journal.committed = true;
        atomic_file(&dir.join(JOURNAL), &serde_json::to_vec(&journal)?)?;
        sync_dir(dir)?;
        Ok(())
    })();
    if let Err(error) = result {
        recover(dir).context(format!("Rockbox publish failed: {error}; rollback failed"))?;
        return Err(error);
    }
    recover(dir)?;
    Ok(())
}
fn load_files(dir: &Path) -> Result<Option<BTreeMap<String, Vec<u8>>>> {
    let Some(master) = optional_bytes(&dir.join(MASTER))? else {
        ensure!(
            !std::fs::read_dir(dir)?.filter_map(|e| e.ok()).any(|e| e
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("database_") && n.ends_with(".tcd"))),
            "Incomplete Rockbox database"
        );
        return Ok(None);
    };
    let le = word(&master, 0, false)?;
    let magic = if matches!(le, 0x5443480f | 0x54434810) {
        le
    } else {
        word(&master, 0, true)?
    };
    ensure!(
        matches!(magic, 0x5443480f | 0x54434810),
        "Unsupported Rockbox database version"
    );
    let mut files = BTreeMap::from([(MASTER.into(), master)]);
    for tag in STRINGS
        .into_iter()
        .filter(|t| magic & 255 == 16 || *t != 12)
    {
        let name = format!("database_{}.tcd", physical(tag, (magic & 255) as u8));
        files.insert(name.clone(), read_bounded(&dir.join(name))?);
        ensure!(
            files.values().map(Vec::len).sum::<usize>() <= MAX_TOTAL,
            "Rockbox database exceeds memory limit"
        );
    }
    Ok(Some(files))
}
fn load(dir: &Path) -> Result<Option<RbDatabase>> {
    load_files(dir)?
        .map(|files| RbDatabase::decode(&files))
        .transpose()
}
fn mounted_dir(root: &Path) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .context("Rockbox requires mounted storage")?;
    let dir = root
        .join(".rockbox")
        .canonicalize()
        .context("Rockbox firmware directory .rockbox is required")?;
    ensure!(
        dir.is_dir() && dir.starts_with(&root),
        "Rockbox directory escapes device root"
    );
    if let Some(bytes) = optional_bytes(&dir.join("config.cfg"))? {
        for line in std::str::from_utf8(&bytes)?.lines() {
            if let Some((key, value)) = line.split_once(':') {
                if key.trim().eq_ignore_ascii_case("database path") {
                    ensure!(
                        matches!(value.trim().trim_end_matches('/'), "" | "/.rockbox"),
                        "Custom Rockbox database locations are unsupported"
                    );
                }
            }
        }
    }
    recover(&dir)?;
    for name in ["database_tmp.tcd", "database_changelog.txt"] {
        if let Some(bytes) = optional_bytes(&dir.join(name))? {
            ensure!(
                name != "database_tmp.tcd" && bytes.is_empty(),
                "Rockbox has pending database data in {name}; commit it on device first"
            );
        }
    }
    Ok(dir)
}

#[derive(Clone, Serialize, Deserialize)]
struct OwnedPath {
    path: String,
    server: Option<String>,
    track: String,
    #[serde(default)]
    prefix: Option<String>,
}
fn owned(manifest: &crate::device::DeviceManifest) -> Vec<OwnedPath> {
    manifest
        .synced_items
        .iter()
        .map(|i| OwnedPath {
            path: i.local_path.clone(),
            server: i.server_id.clone(),
            track: i.jellyfin_id.clone(),
            prefix: Some(manifest.rockbox.path_prefix.clone()),
        })
        .collect()
}
pub struct Session {
    dir: PathBuf,
    db: RbDatabase,
    before: crate::device::DeviceManifest,
    previous: Vec<OwnedPath>,
    by_path: HashMap<String, Option<usize>>,
    original: BTreeMap<String, String>,
    sources_by_path: HashMap<String, Option<(Option<String>, String)>>,
    cleared_ratings: std::sync::Mutex<HashSet<(String, String, String)>>,
    // The value records whether refresh initialized a new entry without source history.
    refreshed_sources: HashMap<(Option<String>, String, String), bool>,
    staged_backfills: std::sync::Mutex<Vec<(String, String, String, i32)>>,
}
pub(crate) struct PreparedUpdate {
    dir: PathBuf,
    files: BTreeMap<String, Vec<u8>>,
    original: BTreeMap<String, String>,
    owners: Vec<u8>,
}
impl PreparedUpdate {
    pub(crate) async fn publish(self) -> Result<()> {
        tokio::task::spawn_blocking(move || {
            publish(&self.dir, &self.files, Some(&self.original))
                .with_context(|| format!("Cannot publish Rockbox database in {}",self.dir.display()))?;
            atomic_file(&self.dir.join(OWNERSHIP), &self.owners)?;
            sync_dir(&self.dir)
        })
        .await?
    }
}
impl Session {
    pub fn open(root: &Path, before: crate::device::DeviceManifest) -> Result<Self> {
        before.rockbox.validate()?;
        let dir = mounted_dir(root)?;
        let files = load_files(&dir)?;
        let mut original: BTreeMap<String, String> = files
            .as_ref()
            .into_iter()
            .flat_map(|f| f.iter())
            .map(|(name, bytes)| (name.clone(), hash(bytes)))
            .collect();
        if let Some(state) = optional_bytes(&dir.join("database_state.tcd"))? {
            original.insert("database_state.tcd".into(), hash(&state));
        }
        let db = files
            .as_ref()
            .map(RbDatabase::decode)
            .transpose()?
            .unwrap_or_else(|| {
                RbDatabase::empty(
                    if before.rockbox.format == "3.15" {
                        15
                    } else {
                        16
                    },
                    false,
                )
            });
        let mut previous: Vec<OwnedPath> = optional_bytes(&dir.join(OWNERSHIP))?
            .map(|b| serde_json::from_slice(&b))
            .transpose()?
            .unwrap_or_default();
        for entry in &previous {
            device_path(&entry.path)?;
        }
        let mut seen: HashSet<_> = previous
            .iter()
            .map(|p| {
                (
                    p.path.clone(),
                    p.server.clone(),
                    p.track.clone(),
                    p.prefix.clone(),
                )
            })
            .collect();
        for entry in owned(&before) {
            if seen.insert((
                entry.path.clone(),
                entry.server.clone(),
                entry.track.clone(),
                entry.prefix.clone(),
            )) {
                previous.push(entry);
            }
        }
        if before.rockbox.enabled {
            atomic_file(&dir.join(OWNERSHIP), &serde_json::to_vec(&previous)?)?;
            sync_dir(&dir)?;
        }
        let mut by_path = HashMap::new();
        for (index, entry) in db.entries.iter().enumerate() {
            by_path
                .entry(virtual_path(entry.path())?)
                .and_modify(|value| *value = None)
                .or_insert(Some(index));
        }
        let mut sources_by_path: HashMap<String, Option<(Option<String>, String)>> = HashMap::new();
        let prefix = virtual_path(&before.rockbox.path_prefix)?;
        for owner in &previous {
            if virtual_path(owner.prefix.as_deref().unwrap_or("/"))? != prefix {
                continue;
            }
            let source = (owner.server.clone(), owner.track.clone());
            sources_by_path
                .entry(device_path(&owner.path)?)
                .and_modify(|old| {
                    if old.as_ref() != Some(&source) {
                        *old = None;
                    }
                })
                .or_insert(Some(source));
        }
        Ok(Self {
            dir,
            db,
            before,
            previous,
            by_path,
            original,
            sources_by_path,
            cleared_ratings: Default::default(),
            refreshed_sources: Default::default(),
            staged_backfills: Default::default(),
        })
    }
    fn source_owns(&self, path: &str, server: &Option<String>, track: &str) -> bool {
        self.sources_by_path
            .get(path)
            .and_then(Option::as_ref)
            .is_some_and(|(s, t)| s == server && t == track)
    }
    fn entry_for(&self, path: &str) -> Result<Option<usize>> {
        let local = firmware_path(&self.before.rockbox.path_prefix, path)?;
        match self.by_path.get(&local) {
            Some(Some(index)) => Ok(Some(*index)),
            Some(None) => bail!("Ambiguous Rockbox volume path for {local}"),
            None => Ok(None),
        }
    }
    pub async fn import_ratings(
        &self,
        db: &crate::db::Database,
        providers: &HashMap<String, std::sync::Arc<dyn crate::providers::MediaProvider>>,
        cancel: &crate::sync::SyncOperationManager,
        operation: &str,
    ) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        // Resolve retryable intents before maintenance replaces the admitted snapshot.
        db.conn.lock().unwrap().execute(
            "DELETE FROM rockbox_rating_backfills WHERE device=?1 AND original=?2",
            rusqlite::params![self.before.device_id, self.fingerprint()?],
        )?;
        let mut providers = providers.clone();
        let mut manager = crate::server_manager::ServerManager::new();
        manager.load_from_db(db);
        let manager = std::sync::Arc::new(tokio::sync::RwLock::new(manager));
        let servers = db.list_servers()?;
        let mut ids: HashSet<_> = self
            .before
            .synced_items
            .iter()
            .filter_map(|i| i.server_id.clone())
            .collect();
        {
            let conn = db.conn.lock().unwrap();
            let mut query=conn.prepare("SELECT DISTINCT server FROM rockbox_ratings WHERE device=?1 AND (confirmed IS NULL OR confirmed!=observed)")?;
            for server in query.query_map([&self.before.device_id], |r| r.get::<_, String>(0))? {
                ids.insert(server?);
            }
        }
        for id in ids {
            if !providers.contains_key(&id) {
                if let Ok(provider) =
                    crate::server_manager::get_provider_by_server_id(&manager, db, &id).await
                {
                    providers.insert(id, provider);
                }
            }
        }
        let mut seen = HashSet::new();
        let mut path_counts = HashMap::new();
        let mut source_counts = HashMap::new();
        for item in &self.before.synced_items {
            *path_counts.entry(&item.local_path).or_insert(0) += 1;
            *source_counts
                .entry((&item.server_id, &item.jellyfin_id))
                .or_insert(0) += 1;
        }
        for item in &self.before.synced_items {
            let Some(server) = &item.server_id else {
                warnings.push(format!(
                    "Rockbox rating skipped: {} has no source server",
                    item.local_path
                ));
                continue;
            };
            let key = (server.clone(), item.jellyfin_id.clone());
            if !seen.insert(key) {
                continue;
            }
            let account = providers
                .get(server)
                .and_then(|p| p.numeric_rating_account())
                .or_else(|| {
                    servers
                        .iter()
                        .find(|s| {
                            s.server_id.as_deref() == Some(server)
                                && matches!(s.server_type.as_str(), "subsonic" | "openSubsonic")
                        })
                        .map(|s| crate::providers::feedback::account_scope(&s.username))
                });
            let Some(account) = account else {
                if !providers.contains_key(server) {
                    warnings.push(format!(
                        "Rockbox rating skipped: source {server} unavailable"
                    ));
                }
                continue;
            };
            // Portable IDs are frozen by server upsert, even when its username changes.
            let original_account = original_rating_account(&servers, server, &account);
            if !original_account {
                warnings.push(format!("Rockbox rating skipped: source {server} no longer identifies its original account"));
                continue;
            }
            let index = match self.entry_for(&item.local_path) {
                Ok(Some(index)) => index,
                Ok(None) => continue,
                Err(e) => {
                    warnings.push(e.to_string());
                    continue;
                }
            };
            if !self.source_owns(
                &device_path(&item.local_path)?,
                &item.server_id,
                &item.jellyfin_id,
            ) {
                warnings.push(format!(
                    "Rockbox rating skipped: source ownership changed at {}",
                    item.local_path
                ));
                continue;
            }
            let raw = self.db.entries[index].numbers[16];
            if !(0..=10).contains(&raw) {
                warnings.push(format!("Invalid Rockbox rating for {}", item.local_path));
                continue;
            }
            if path_counts[&item.local_path] != 1
                || source_counts[&(&item.server_id, &item.jellyfin_id)] != 1
            {
                warnings.push(format!(
                    "Ambiguous Rockbox rating ownership for {}",
                    item.local_path
                ));
                continue;
            }
            let key = (server.clone(), account.clone(), item.jellyfin_id.clone());
            if self.reconcile_backfill(db, server, &account, &item.jellyfin_id, raw, &mut warnings)? {
                continue;
            }
            let previous: Option<(i32, Option<i32>, bool)> = db.conn.lock().unwrap().query_row(
                "SELECT observed,confirmed,initialized_zero FROM rockbox_ratings WHERE device=?1 AND server=?2 AND account=?3 AND track=?4",
                rusqlite::params![self.before.device_id,server,account,item.jellyfin_id],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
            // A rebuilt database starts at zero without clearing an older pending device edit.
            if raw == 0 && previous.is_some_and(|(_,_,initialized)| initialized) { continue; }
            if raw == 0 && previous.is_some_and(|(observed,confirmed,_)| observed != 0 || confirmed != Some(0)) {
                self.cleared_ratings.lock().unwrap().insert(key);
            }
            observe_rating(
                db,
                &self.before.device_id,
                server,
                &account,
                &item.jellyfin_id,
                raw,
            )?;
        }
        let pending = {
            let conn = db.conn.lock().unwrap();
            let mut statement=conn.prepare("SELECT server,account,track,observed FROM rockbox_ratings r WHERE device=?1 AND (confirmed IS NULL OR confirmed!=observed) AND NOT EXISTS (SELECT 1 FROM rockbox_rating_backfills b WHERE b.device=r.device AND b.server=r.server AND b.account=r.account AND b.track=r.track)")?;
            statement
                .query_map([&self.before.device_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i32>(3)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (server, account, track, raw) in pending {
            ensure!(
                !cancel.is_cancelled(operation).await,
                "Rockbox ratings import cancelled"
            );
            if raw == 0 {
                self.cleared_ratings.lock().unwrap().insert((server.clone(),account.clone(),track.clone()));
            }
            let Some(provider) = providers
                .get(&server)
                .filter(|p| p.numeric_rating_account().as_deref() == Some(&account))
            else {
                warnings.push(format!(
                    "Rockbox rating pending: source {server} unavailable or account changed"
                ));
                continue;
            };
            let rating = navidrome_rating(raw)?;
            let result = async {
                if provider.read_numeric_rating(&track).await? != rating {
                    provider.set_numeric_rating(&track, rating).await?;
                }
                ensure_rating_readback(provider.as_ref(), &track, rating).await
            }
            .await;
            match result {
                Ok(()) => {
                    db.conn.lock().unwrap().execute("UPDATE rockbox_ratings SET confirmed=?5 WHERE device=?1 AND server=?2 AND account=?3 AND track=?4 AND observed=?5",rusqlite::params![self.before.device_id,server,account,track,raw])?;
                }
                Err(error) => warnings.push(format!("Rockbox rating pending for {track}: {error}")),
            }
        }
        Ok(warnings)
    }
    fn fingerprint(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(&self.original)?))
    }
    fn reconcile_backfill(
        &self, db: &crate::db::Database, server: &str, account: &str, track: &str,
        raw: i32, warnings: &mut Vec<String>,
    ) -> Result<bool> {
        let pending: Option<(i32,String)> = db.conn.lock().unwrap().query_row(
            "SELECT rating,original FROM rockbox_rating_backfills WHERE device=?1 AND server=?2 AND account=?3 AND track=?4",
            rusqlite::params![self.before.device_id,server,account,track],
            |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((rating, original)) = pending else { return Ok(false); };
        // Initialization never writes a positive value; a later positive is a device edit.
        if rating == 0 || raw == rating {
            confirm_backfill(db, &self.before.device_id, server, account, track, rating)?;
            return Ok(false);
        }
        if raw != 0 || self.fingerprint()? != original {
            warnings.push(format!("Rockbox rating recovery uncertain for {track}; leaving device and server unchanged"));
        }
        // An interrupted seed must never be interpreted as a device clear or edit.
        Ok(true)
    }
    pub(crate) async fn prepare_rating_backfills(
        &self,
        mut prepared: Option<PreparedUpdate>,
        after: &crate::device::DeviceManifest,
        db: &crate::db::Database,
        providers: &HashMap<String, std::sync::Arc<dyn crate::providers::MediaProvider>>,
        cancel: &crate::sync::SyncOperationManager,
        operation: &str,
        warn: &mut (dyn FnMut(String) -> Result<()> + Send),
    ) -> Result<Option<PreparedUpdate>> {
        if !after.rockbox.import_ratings { return Ok(prepared); }
        ensure!(virtual_path(&self.before.rockbox.path_prefix)? == virtual_path(&after.rockbox.path_prefix)?, "Rockbox prefix changed during sync; retry");
        if !self.original.contains_key(MASTER) && prepared.is_none() { return Ok(None); }
        let database = &self.db;
        let mut paths = HashMap::new();
        for (index, entry) in database.entries.iter().enumerate() {
            paths.entry(virtual_path(entry.path())?).and_modify(|i| *i = None).or_insert(Some(index));
        }
        let mut path_counts = HashMap::new();
        let mut source_counts = HashMap::new();
        for item in &after.synced_items {
            *path_counts.entry(device_path(&item.local_path)?).or_insert(0) += 1;
            *source_counts.entry((&item.server_id,&item.jellyfin_id)).or_insert(0) += 1;
        }
        let servers = db.list_servers()?;
        let mut providers = providers.clone();
        let mut manager = crate::server_manager::ServerManager::new();
        manager.load_from_db(db);
        let manager = std::sync::Arc::new(tokio::sync::RwLock::new(manager));
        let mut groups: HashMap<(String,String), Vec<(String,usize)>> = HashMap::new();
        let recreated = prepared.is_some() && !self.original.contains_key(MASTER);
        let mut seeds: HashMap<(String,String,String),(usize,i32)> = HashMap::new();
        let mut unavailable = HashSet::new();
        let fingerprint = self.fingerprint()?;
        for item in &after.synced_items {
            let Some(server) = &item.server_id else { continue; };
            if path_counts[&device_path(&item.local_path)?] != 1 || source_counts[&(&item.server_id,&item.jellyfin_id)] != 1 { continue; }
            let path = firmware_path(&after.rockbox.path_prefix, &item.local_path)?;
            let Some(Some(index)) = paths.get(&path) else { continue; };
            if database.entries[*index].numbers[16] != 0 { continue; }
            if !self.source_owns(&device_path(&item.local_path)?, &item.server_id, &item.jellyfin_id)
                && !self.refreshed_sources.contains_key(&(item.server_id.clone(),item.jellyfin_id.clone(),item.local_path.clone())) { continue; }
            if !providers.contains_key(server) && unavailable.insert(server.clone()) {
                match crate::server_manager::get_provider_by_server_id(&manager, db, server).await {
                    Ok(provider) => { providers.insert(server.clone(), provider); },
                    Err(_) => warn(format!("Rockbox rating backfill skipped: source {server} unavailable"))?,
                }
            }
            let Some(account) = providers.get(server).and_then(|p| p.numeric_rating_account()).or_else(|| {
                servers.iter().find(|s| s.server_id.as_deref() == Some(server))
                    .map(|s| crate::providers::feedback::account_scope(&s.username))
            }) else { continue; };
            if !original_rating_account(&servers, server, &account) { continue; }
            if self.cleared_ratings.lock().unwrap().contains(&(server.clone(),account.clone(),item.jellyfin_id.clone())) { continue; }
            let initialized_entry = recreated || self.refreshed_sources
                .get(&(item.server_id.clone(),item.jellyfin_id.clone(),item.local_path.clone())).copied().unwrap_or(false);
            let conn = db.conn.lock().unwrap();
            let observation: Option<(i32,Option<i32>,bool)> = conn.query_row(
                "SELECT observed,confirmed,initialized_zero FROM rockbox_ratings WHERE device=?1 AND server=?2 AND account=?3 AND track=?4",
                rusqlite::params![after.device_id,server,account,item.jellyfin_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if let Some((raw,confirmed,initialized)) = observation {
                if raw == 0 && confirmed != Some(0) { continue; }
                if raw != 0 {
                    if !(initialized_entry || initialized) { continue; }
                    if initialized_entry {seeds.insert((server.clone(),account.clone(),item.jellyfin_id.clone()),(*index,0));}
                    if confirmed != Some(raw) { continue; }
                }
            }
            let pending: Option<String> = conn.query_row(
                "SELECT original FROM rockbox_rating_backfills WHERE device=?1 AND server=?2 AND account=?3 AND track=?4",
                rusqlite::params![after.device_id,server,account,item.jellyfin_id], |r| r.get(0)).optional()?;
            if initialized_entry && pending.is_some() {
                seeds.insert((server.clone(),account.clone(),item.jellyfin_id.clone()),(*index,0));
            } else if pending.is_some_and(|original| original != fingerprint) { continue; }
            if providers.get(server).and_then(|p| p.numeric_rating_account()).as_deref() != Some(&account) { continue; }
            groups.entry((server.clone(),account)).or_default().push((item.jellyfin_id.clone(),*index));
        }
        for ((server,account), candidates) in groups {
            let provider = &providers[&server];
            let requested: HashSet<_> = candidates.iter().map(|(id,_)| id.as_str()).collect();
            let result = async {
                let mut ratings = HashMap::new();
                let mut seen = HashSet::new();
                let mut id_bytes = 0usize;
                let mut offset = 0;
                loop {
                    ensure!(!cancel.is_cancelled(operation).await, "Rockbox ratings sync cancelled");
                    let (page,next) = provider.numeric_ratings_page(offset).await?;
                    ensure!(page.len() <= 1000 && seen.len() + page.len() <= 1_000_000, "Oversized numeric rating page");
                    for (id,rating) in page {
                        id_bytes = id_bytes.checked_add(id.len()).context("Numeric rating lookup overflow")?;
                        ensure!(id_bytes <= 64 * 1024 * 1024 && !id.is_empty() && id.len() <= 4096 && rating <= 5 && seen.insert(id.clone()), "Invalid, oversized or repeated numeric rating");
                        if requested.contains(id.as_str()) { ratings.insert(id,rating); }
                    }
                    if ratings.len() == requested.len() { break; }
                    match next {
                        None => break,
                        Some(next) => { ensure!(next > offset && next <= 1_000_000, "Invalid numeric rating pagination"); offset = next; }
                    }
                }
                Ok::<_,anyhow::Error>(ratings)
            }.await;
            match result {
                Ok(ratings) => for (track,index) in candidates {
                    if let Some(rating) = ratings.get(&track).filter(|r| **r > 0) {
                        seeds.insert((server.clone(),account.clone(),track),(index,i32::from(*rating)*2));
                    }
                },
                Err(error) if matches!(error.downcast_ref::<crate::providers::ProviderError>(), Some(crate::providers::ProviderError::UnsupportedCapability(_))) => {},
                Err(error) => warn(format!("Rockbox rating backfill skipped for {server}: {error}"))?,
            }
        }
        if seeds.is_empty() { return Ok(prepared); }
        ensure!(!cancel.is_cancelled(operation).await, "Rockbox ratings sync cancelled");
        let ratings_only = prepared.is_none();
        if ratings_only {
            prepared = Some(PreparedUpdate {
                dir: self.dir.clone(), files: load_files(&self.dir)?.context("Rockbox database disappeared")?,
                original: self.original.clone(), owners: optional_bytes(&self.dir.join(OWNERSHIP))?.unwrap_or(serde_json::to_vec(&self.previous)?),
            });
        }
        let update = prepared.as_mut().unwrap();
        let master = update.files.get_mut(MASTER).context("Missing Rockbox master")?;
        let slots = if database.version == 15 {22} else {23};
        // Decoding drops deleted rows; a rating-only patch must retain disk row offsets.
        let disk_rows = if ratings_only {
            let count = usize::try_from(word(master,8,database.big)?).context("Invalid Rockbox row count")?;
            ensure!(master.len() == 24 + count * (slots + 1) * 4, "Rockbox master changed during rating lookup");
            let mut rows = Vec::new();
            for index in 0..count {
                if word(master,24 + index * (slots + 1) * 4 + slots * 4,database.big)? & 1 == 0 { rows.push(index); }
            }
            rows
        } else { (0..database.entries.len()).collect() };
        for (index,rating) in seeds.values() {
            let index = disk_rows.get(*index).context("Rockbox rows changed during rating lookup")?;
            let offset = 24 + index * (slots + 1) * 4 + physical(16,database.version) * 4;
            let bytes = if database.big {rating.to_be_bytes()} else {rating.to_le_bytes()};
            master.get_mut(offset..offset+4).context("Rockbox master changed during rating lookup")?.copy_from_slice(&bytes);
        }
        // Persist intent before the filesystem transaction; admission reconciles crashes.
        let mut conn = db.conn.lock().unwrap();
        let transaction = conn.transaction()?;
        let mut staged = Vec::new();
        for ((server,account,track),(_,rating)) in seeds {
            transaction.execute("INSERT INTO rockbox_rating_backfills(device,server,account,track,rating,original) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(device,server,account,track) DO UPDATE SET rating=excluded.rating,original=excluded.original",
                rusqlite::params![after.device_id,server,account,track,rating,fingerprint])?;
            staged.push((server,account,track,rating));
        }
        transaction.commit()?;
        *self.staged_backfills.lock().unwrap() = staged;
        Ok(prepared)
    }
    pub(crate) fn confirm_rating_backfills(&self, db: &crate::db::Database) -> Result<()> {
        let staged = self.staged_backfills.lock().unwrap();
        if staged.is_empty() { return Ok(()); }
        let mut conn = db.conn.lock().unwrap();
        let transaction = conn.transaction()?;
        for (server,account,track,rating) in staged.iter() {
            confirm_backfill_transaction(&transaction,&self.before.device_id,server,account,track,*rating)?;
        }
        transaction.commit()?;
        Ok(())
    }
    pub(crate) async fn prepare_update(
        &mut self,
        after: &crate::device::DeviceManifest,
        io: &dyn crate::device_io::DeviceIO,
        cancel: &crate::sync::SyncOperationManager,
        operation: &str,
        warn: &mut (dyn FnMut(String) -> Result<()> + Send),
    ) -> Result<PreparedUpdate> {
        let root = self.dir.parent().context("Missing device root")?.canonicalize()
            .context("Cannot inspect Rockbox device root")?;
        let mut old_by_source: HashMap<(Option<String>, String), Vec<&OwnedPath>> = HashMap::new();
        let active_prefix = virtual_path(&self.before.rockbox.path_prefix)?;
        ensure!(
            active_prefix == virtual_path(&after.rockbox.path_prefix)?,
            "Rockbox prefix changed during sync; retry"
        );
        for item in &self.previous {
            if virtual_path(item.prefix.as_deref().unwrap_or("/"))? != active_prefix {
                continue;
            }
            old_by_source
                .entry((item.server.clone(), item.track.clone()))
                .or_default()
                .push(item);
        }
        let remaining: HashSet<_> = after
            .synced_items
            .iter()
            .map(|i| i.local_path.as_str())
            .collect();
        let mut removed = HashSet::new();
        let mut retained_history = HashSet::new();
        for old in &self.previous {
            if virtual_path(old.prefix.as_deref().unwrap_or("/"))? != active_prefix {
                continue;
            }
            if !remaining.contains(old.path.as_str()) && io.file_length(&old.path).await
                .with_context(|| format!("Cannot inspect previous Rockbox audio {}",old.path))?.is_none()
            {
                if let Some(index) = self.entry_for(&old.path)? {
                    removed.insert(index);
                }
            }
        }
        let previous_items: HashMap<_, _> = self
            .before
            .synced_items
            .iter()
            .map(|i| (i.local_path.as_str(), i))
            .collect();
        let remaining_sources: HashSet<_> = after
            .synced_items
            .iter()
            .map(|item| (&item.server_id, &item.jellyfin_id))
            .collect();
        for item in &after.synced_items {
            ensure!(
                !cancel.is_cancelled(operation).await,
                "Rockbox update cancelled"
            );
            let local = device_path(&item.local_path)?;
            let index = self.entry_for(&local)?;
            ensure!(
                index.is_none() || !matches!(self.sources_by_path.get(&local), Some(None)),
                "Ambiguous Rockbox source ownership at {local}; resolve the path conflict before updating"
            );
            if index.is_some()
                && !self.source_owns(&local, &item.server_id, &item.jellyfin_id)
                && let Some(Some((server, track))) = self.sources_by_path.get(&local)
            {
                ensure!(
                    !remaining_sources.contains(&(server, track)),
                    "Rockbox history at {local} belongs to another retained source; resolve the path conflict before updating"
                );
            }
            let previous = if let Some(index) =
                index.filter(|_| self.source_owns(&local, &item.server_id, &item.jellyfin_id))
            {
                Some(index)
            } else {
                let mut candidates = HashSet::new();
                if let Some(owners) =
                    old_by_source.get(&(item.server_id.clone(), item.jellyfin_id.clone()))
                {
                    for owner in owners {
                        let old_local = device_path(&owner.path)?;
                        let old_index = self.entry_for(&old_local)?;
                        ensure!(
                            old_index.is_none()
                                || !matches!(self.sources_by_path.get(&old_local), Some(None)),
                            "Ambiguous Rockbox source history at {old_local}; resolve the path conflict before updating"
                        );
                        if self.source_owns(&old_local, &item.server_id, &item.jellyfin_id) {
                            if let Some(index) = old_index {
                                candidates.insert(index);
                            }
                        }
                    }
                }
                ensure!(
                    candidates.len() <= 1,
                    "Ambiguous Rockbox history for source track"
                );
                candidates.into_iter().next()
            };
            let inspected = async {
                let path = io.metadata_path(&local).await
                    .with_context(|| format!("Cannot locate Rockbox audio {local}"))?
                    .context("Rockbox needs mounted audio files")?;
                let path = path.canonicalize()
                    .with_context(|| format!("Cannot resolve Rockbox audio {local}"))?;
                ensure!(path.starts_with(&root), "Managed audio symlink escapes device: {local}");
                let metadata = std::fs::metadata(&path)
                    .with_context(|| format!("Cannot inspect Rockbox audio {local}"))?;
                let mtime = metadata.modified()
                    .with_context(|| format!("Cannot read Rockbox audio timestamp for {local}"))?
                    .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
                    .min(i32::MAX as u64) as i32;
                Ok::<_,anyhow::Error>((metadata,mtime))
            }.await;
            let (metadata,mtime) = match inspected {
                Ok(value) => value,
                Err(error) if error.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                    && (index.is_none() || self.source_owns(&local,&item.server_id,&item.jellyfin_id)) => {
                    // Missing audio is recoverable only while the mounted device remains accessible.
                    ensure!(self.dir.canonicalize().context("Rockbox directory became unavailable")?.starts_with(&root),
                        "Rockbox directory escapes device");
                    if let Some(previous) = previous {
                        removed.remove(&previous);
                        if index.is_none() {retained_history.insert(previous);}
                    }
                    let action = if previous.is_some() {"previous source entry and runtime history retained"} else {"track left unindexed"};
                    warn(format!("Rockbox audio missing for {local}; {action}: {error:#}"))?;
                    continue;
                },
                Err(error) => return Err(error),
            };
            if previous_items.get(item.local_path.as_str()).is_some_and(|old| *old == item)
                && self.source_owns(&local,&item.server_id,&item.jellyfin_id)
                && index.is_some_and(|i| self.db.entries[i].numbers[20].abs_diff(mtime) <= 1) {
                continue;
            }
            let tags = match crate::device_music_scan::probe(
                io,
                &crate::device_io::FileEntry {
                    path: local.clone(),
                    name: item.name.clone(),
                    size: metadata.len(),
                },
                std::sync::Arc::new(crate::device_music_scan::Progress::default()),
            )
            .await
            {
                Ok(tags) => tags,
                Err(error) => {
                    let missing = if error.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
                        true
                    } else if matches!(error.downcast_ref::<ffmpeg_next::Error>(),
                        Some(ffmpeg_next::Error::Other {errno}) if *errno == ffmpeg_next::error::ENOENT) {
                        // FFmpeg may also report a missing auxiliary resource while the audio still exists.
                        match io.file_length(&local).await {
                            Ok(length) => length.is_none(),
                            Err(error) if error.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => true,
                            Err(error) => return Err(error).with_context(|| format!("Cannot recheck Rockbox audio {local}")),
                        }
                    } else { false };
                    let unreadable = matches!(
                        error.downcast_ref::<ffmpeg_next::Error>(),
                        Some(
                            ffmpeg_next::Error::InvalidData
                                | ffmpeg_next::Error::DecoderNotFound
                                | ffmpeg_next::Error::DemuxerNotFound
                                | ffmpeg_next::Error::StreamNotFound
                        )
                    );
                    if !(missing || unreadable) || !(index.is_none()
                        || self.source_owns(&local, &item.server_id, &item.jellyfin_id)) {
                        return Err(error).with_context(|| format!("Cannot read Rockbox audio metadata for {local}"));
                    }
                    if missing {
                        ensure!(self.dir.canonicalize().context("Rockbox directory became unavailable")?.starts_with(&root),
                            "Rockbox directory escapes device");
                    }
                    if let Some(previous) = previous {
                        removed.remove(&previous);
                        if index.is_none() {
                            retained_history.insert(previous);
                        }
                    }
                    let action = if previous.is_some() {
                        "previous source entry and runtime history retained"
                    } else {
                        "track left unindexed"
                    };
                    warn(format!(
                        "Rockbox metadata skipped for {local}; {action}: {error:#}"
                    ))?;
                    continue;
                }
            };
            let mut entry = previous
                .map(|i| self.db.entries[i].clone())
                .unwrap_or_default();
            entry.strings[4] = if let Some(i) = index {
                self.db.entries[i].path().into()
            } else {
                firmware_path(&active_prefix, &local)?.into_bytes()
            };
            for (tag, value) in [
                (0, tags.artist),
                (1, tags.album),
                (2, tags.genre),
                (3, tags.title),
                (5, tags.composer),
                (6, tags.comment),
                (7, tags.album_artist),
                (8, tags.grouping),
            ] {
                let bounded = bounded_metadata(&value);
                if bounded.len() != value.len() {
                    warn(format!(
                        "Rockbox {} shortened for {local}: {} to {} bytes (maximum {MAX_METADATA_TEXT})",
                        tag_name(tag),
                        value.len(),
                        bounded.len()
                    ))?;
                }
                entry.strings[tag] = if bounded.is_empty() {
                    "<Untagged>".into()
                } else {
                    bounded.as_bytes().to_vec()
                };
            }
            entry.strings[12] = if entry.strings[0] == b"<Untagged>" {
                entry.strings[7].clone()
            } else {
                entry.strings[0].clone()
            };
            entry.numbers[9] = tags.year.unwrap_or(0).min(i32::MAX as u32) as i32;
            entry.numbers[10] = tags.disc.unwrap_or(0) as i32;
            entry.numbers[11] = tags.track.unwrap_or(0) as i32;
            entry.numbers[13] = (tags.bitrate.unwrap_or(0) / 1000) as i32;
            entry.numbers[14] = tags.duration_ms.min(i32::MAX as u64) as i32;
            entry.numbers[20] = mtime;
            entry.numbers[19] = self.db.commit.saturating_add(1);
            if let Some(i) = index {
                self.db.entries[i] = entry;
            } else {
                let i = self.db.entries.len();
                self.db.entries.push(entry);
                self.by_path
                    .insert(virtual_path(self.db.entries[i].path())?, Some(i));
            }
            self.refreshed_sources.entry((item.server_id.clone(),item.jellyfin_id.clone(),item.local_path.clone()))
                .or_insert(previous.is_none());
        }
        self.db.entries = std::mem::take(&mut self.db.entries)
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| (!removed.contains(&i)).then_some(e))
            .collect();
        self.db.commit = self.db.commit.saturating_add(1);
        ensure!(
            !cancel.is_cancelled(operation).await,
            "Rockbox update cancelled"
        );
        let dir = self.dir.clone();
        let files = self.db.encode()?;
        let original = self.original.clone();
        let mut owners = owned(after);
        for previous in &self.previous {
            if virtual_path(previous.prefix.as_deref().unwrap_or("/"))? != active_prefix
                || self
                    .entry_for(&previous.path)?
                    .is_some_and(|i| retained_history.contains(&i))
            {
                owners.push(previous.clone());
            }
        }
        Ok(PreparedUpdate {
            dir,
            files,
            original,
            owners: serde_json::to_vec(&owners)?,
        })
    }
}
fn original_rating_account(servers: &[crate::db::ServerConfig], server: &str, account: &str) -> bool {
    servers.iter().find(|s| s.server_id.as_deref() == Some(server)).is_some_and(|s| {
        let mut kinds = vec![s.server_type.as_str()];
        if matches!(s.server_type.as_str(), "subsonic" | "openSubsonic") { kinds.extend(["subsonic","openSubsonic"]); }
        crate::providers::feedback::account_scope(&s.username) == account && kinds.iter().any(|kind| {
            crate::db::derive_server_id(kind,&crate::db::normalized_server_url(&s.url),&s.username,None) == server
        })
    })
}
fn confirm_backfill(db: &crate::db::Database, device: &str, server: &str, account: &str, track: &str, rating: i32) -> Result<()> {
    let mut conn = db.conn.lock().unwrap();
    let transaction = conn.transaction()?;
    confirm_backfill_transaction(&transaction, device, server, account, track, rating)?;
    transaction.commit()?;
    Ok(())
}
fn confirm_backfill_transaction(transaction: &rusqlite::Transaction<'_>, device: &str, server: &str, account: &str, track: &str, rating: i32) -> Result<()> {
    if rating == 0 {
        // Zero acknowledges database initialization, not delivery of an older outbound edit.
        transaction.execute("INSERT INTO rockbox_ratings(device,server,account,track,observed,confirmed,initialized_zero) VALUES(?1,?2,?3,?4,0,0,1) ON CONFLICT(device,server,account,track) DO UPDATE SET initialized_zero=1",
            rusqlite::params![device,server,account,track])?;
    } else {
        transaction.execute("INSERT INTO rockbox_ratings(device,server,account,track,observed,confirmed) VALUES(?1,?2,?3,?4,?5,?5) ON CONFLICT(device,server,account,track) DO UPDATE SET observed=excluded.observed,confirmed=excluded.confirmed,initialized_zero=0",
            rusqlite::params![device,server,account,track,rating])?;
    }
    transaction.execute("DELETE FROM rockbox_rating_backfills WHERE device=?1 AND server=?2 AND account=?3 AND track=?4 AND rating=?5",rusqlite::params![device,server,account,track,rating])?;
    Ok(())
}
fn observe_rating(
    db: &crate::db::Database,
    device: &str,
    server: &str,
    account: &str,
    track: &str,
    raw: i32,
) -> Result<()> {
    ensure!((0..=10).contains(&raw), "Invalid Rockbox rating");
    db.conn.lock().unwrap().execute("INSERT INTO rockbox_ratings(device,server,account,track,observed,confirmed) VALUES(?1,?2,?3,?4,?5,CASE WHEN ?5=0 THEN 0 ELSE NULL END) ON CONFLICT(device,server,account,track) DO UPDATE SET confirmed=CASE WHEN initialized_zero!=0 OR observed!=excluded.observed THEN NULL ELSE confirmed END,observed=excluded.observed,initialized_zero=0",rusqlite::params![device,server,account,track,raw])?;
    Ok(())
}
fn navidrome_rating(raw: i32) -> Result<u8> {
    ensure!((0..=10).contains(&raw), "Invalid Rockbox rating");
    Ok(((raw + 1) / 2) as u8)
}
async fn ensure_rating_readback(
    provider: &dyn crate::providers::MediaProvider,
    track: &str,
    rating: u8,
) -> std::result::Result<(), crate::providers::ProviderError> {
    if provider.read_numeric_rating(track).await? != rating {
        return Err(crate::providers::feedback::malformed());
    }
    Ok(())
}

#[cfg(test)]
fn encode_empty(_version: u8, _big_endian: bool) -> Result<BTreeMap<String, Vec<u8>>> {
    RbDatabase::empty(_version, _big_endian).encode()
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    #[ignore = "requires HIFIMULE_TEST_ROCKBOX_ROOT and HIFIMULE_TEST_ROCKBOX_TRACK; updates a temporary copy only"]
    async fn rockbox_connected_track_updates_only_a_temporary_copy() {
        let source = PathBuf::from(std::env::var_os("HIFIMULE_TEST_ROCKBOX_ROOT").unwrap());
        let local = std::env::var("HIFIMULE_TEST_ROCKBOX_TRACK").unwrap();
        let long_comment =
            std::env::var("HIFIMULE_TEST_ROCKBOX_LONG_COMMENT").as_deref() == Ok("1");
        device_path(&local).unwrap();
        let manifest_bytes = std::fs::read(source.join(".hifimule.json.zst")).unwrap();
        let mut manifest: crate::device::DeviceManifest = serde_json::from_str(
            &crate::device::manifest_codec::read_manifest_json_sync(
                source.join(".hifimule.json.zst"),
            )
            .unwrap(),
        )
        .unwrap();
        let track = manifest
            .synced_items
            .iter()
            .find(|item| item.local_path == local)
            .unwrap()
            .clone();
        manifest.synced_items = vec![track];
        let files = load_files(&source.join(".rockbox")).unwrap().unwrap();
        let original = RbDatabase::decode(&files).unwrap();
        let firmware = firmware_path(&manifest.rockbox.path_prefix, &local).unwrap();
        let had_entry = original
            .entries
            .iter()
            .any(|entry| entry.path() == firmware);
        assert!(long_comment || had_entry);
        let audio = std::fs::read(source.join(&local)).unwrap();
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        save(&dir, &files);
        let path = root.path().join(&local);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &audio).unwrap();
        let mut session = Session::open(root.path(), manifest.clone()).unwrap();
        manifest.synced_items[0]
            .name
            .push_str(" [probe verification]");
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        let warnings = update_session(
            &mut session,
            &manifest,
            &io,
            &crate::sync::SyncOperationManager::new(),
            "test",
        )
        .await
        .unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains(&local));
        let current = load(&dir).unwrap().unwrap();
        assert_eq!(
            current.entries.len(),
            original.entries.len() + usize::from(long_comment && !had_entry)
        );
        for (before, after) in original.entries.iter().zip(&current.entries) {
            if long_comment && before.path() == firmware {
                for tag in [15, 16, 17, 18, 21, 22] {
                    assert_eq!(before.numbers[tag], after.numbers[tag]);
                }
            } else {
                assert_eq!(before.strings, after.strings);
                for tag in (0..23).filter(|tag| !STRINGS.contains(tag)) {
                    assert_eq!(before.numbers[tag], after.numbers[tag]);
                }
            }
            assert_eq!(before.flags & 8, after.flags);
        }
        if long_comment {
            let raw = crate::device_music_scan::probe(
                &io,
                &crate::device_io::FileEntry {
                    path: local.clone(),
                    name: String::new(),
                    size: audio.len() as u64,
                },
                std::sync::Arc::new(crate::device_music_scan::Progress::default()),
            )
            .await
            .unwrap();
            assert_eq!(raw.comment.len(), 10_244);
            let entry = current
                .entries
                .iter()
                .find(|entry| entry.path() == firmware)
                .unwrap();
            let comment = std::str::from_utf8(&entry.strings[6]).unwrap();
            assert!((517..=520).contains(&comment.len()));
            assert!(raw.comment.starts_with(comment));
            assert!(!comment.contains('\0'));
            assert!(warnings[0].contains("comment") && warnings[0].contains("10244"));
        }
        assert_eq!(std::fs::read(&path).unwrap(), audio);
        assert_eq!(
            load_files(&source.join(".rockbox")).unwrap().unwrap(),
            files
        );
        assert_eq!(
            std::fs::read(source.join(".hifimule.json.zst")).unwrap(),
            manifest_bytes
        );
        assert_eq!(std::fs::read(source.join(&local)).unwrap(), audio);
        eprintln!(
            "Verified track warning and {} database entries using a temporary copy; H: database, manifest and audio unchanged",
            current.entries.len()
        );
    }

    #[tokio::test]
    #[ignore = "requires HIFIMULE_TEST_ROCKBOX_ROOT; probes mounted audio without writing device files"]
    async fn rockbox_connected_audio_read_only_probe() {
        let root = PathBuf::from(
            std::env::var_os("HIFIMULE_TEST_ROCKBOX_ROOT").expect("Set HIFIMULE_TEST_ROCKBOX_ROOT"),
        );
        let manifest: crate::device::DeviceManifest = serde_json::from_str(
            &crate::device::manifest_codec::read_manifest_json_sync(
                root.join(".hifimule.json.zst"),
            )
            .unwrap(),
        )
        .unwrap();
        let io = crate::device_io::MscBackend::new(root);
        ffmpeg_next::init().unwrap();
        ffmpeg_next::log::set_level(ffmpeg_next::log::Level::Quiet);
        let progress = std::sync::Arc::new(crate::device_music_scan::Progress::default());
        for (i, item) in manifest.synced_items.iter().enumerate() {
            if i % 1000 == 0 {
                eprintln!("Read-only probe {i}/{}", manifest.synced_items.len());
            }
            let file = crate::device_io::FileEntry {
                path: item.local_path.clone(),
                name: item.name.clone(),
                size: item.size_bytes,
            };
            if let Err(error) = crate::device_music_scan::probe(&io, &file, progress.clone()).await
            {
                panic!(
                    "Read-only probe failed at item {i}, {}: {error:#}",
                    item.local_path
                );
            }
        }
    }

    #[test]
    fn rockbox_rejects_non_utf8_filenames_even_when_metadata_is_opaque() {
        let mut db = one();
        let mut files = db.encode().unwrap();
        files.get_mut("database_4.tcd").unwrap()[21] = 0xff;
        let error = RbDatabase::decode(&files).unwrap_err();
        assert!(error.to_string().contains("filename UTF-8"));
        db.entries[0].strings[4] = vec![b'/', 0xff];
        assert!(db.encode().is_err());
    }

    #[test]
    #[ignore = "requires HIFIMULE_TEST_ROCKBOX_DB; reads the connected database without writing it"]
    fn rockbox_connected_database_read_only_roundtrip() {
        let dir = PathBuf::from(
            std::env::var_os("HIFIMULE_TEST_ROCKBOX_DB")
                .expect("Set HIFIMULE_TEST_ROCKBOX_DB to the database directory"),
        );
        let files = load_files(&dir)
            .unwrap()
            .expect("Existing Rockbox database");
        let db = RbDatabase::decode(&files).unwrap();
        let encoded = db.encode().unwrap();
        let restored = RbDatabase::decode(&encoded).unwrap();
        assert_eq!(restored.entries.len(), db.entries.len());
        assert_eq!((restored.version, restored.big), (db.version, db.big));
        for (original, next) in db.entries.iter().zip(&restored.entries) {
            assert_eq!(original.strings, next.strings);
            for tag in 0..23 {
                if !STRINGS.contains(&tag) {
                    assert_eq!(original.numbers[tag], next.numbers[tag]);
                }
            }
        }
        for (name, bytes) in &files {
            assert_eq!(read_bounded(&dir.join(name)).unwrap(), *bytes);
        }
        eprintln!(
            "Read-only round-trip verified {} Rockbox tracks, metadata bytes and runtime values; device files unchanged",
            db.entries.len()
        );
    }

    #[test]
    fn rockbox_preserves_legacy_metadata_bytes_without_guessing_encodings() {
        for version in [15, 16] {
            for big in [false, true] {
                let mut db = one();
                db.version = version;
                db.big = big;
                db.entries[0].strings[0] = "Blue Oyster Cult".into();
                db.entries[0].strings[6] = "ABCDEF".into();
                let mut files = db.encode().unwrap();
                files.get_mut("database_0.tcd").unwrap()[25] = 0xd6;
                files.get_mut("database_6.tcd").unwrap()[20..26]
                    .copy_from_slice(&[0xd2, 0xee, 0xeb, 0xfc, 0xea, 0xee]);
                let decoded = RbDatabase::decode(&files)
                    .expect("Legacy metadata bytes are valid database records");
                let encoded = decoded.encode().unwrap();
                assert_eq!(encoded["database_0.tcd"], files["database_0.tcd"]);
                assert_eq!(encoded["database_6.tcd"], files["database_6.tcd"]);
                assert_eq!(&decoded.entries[0].numbers[15..17], &[12, 7]);
            }
        }
    }

    async fn update_session(
        session: &mut super::Session,
        after: &crate::device::DeviceManifest,
        io: &dyn crate::device_io::DeviceIO,
        cancel: &crate::sync::SyncOperationManager,
        operation: &str,
    ) -> anyhow::Result<Vec<String>> {
        let mut warnings = Vec::new();
        session
            .prepare_update(after, io, cancel, operation, &mut |warning| {
                warnings.push(warning);
                Ok(())
            })
            .await?
            .publish()
            .await?;
        Ok(warnings)
    }

    use super::*;
    #[test]
    fn rockbox_empty_database_has_release_headers_in_both_endiannesses() {
        for (version, big) in [(15, false), (16, false), (15, true), (16, true)] {
            let files = encode_empty(version, big).unwrap();
            let master = &files["database_idx.tcd"];
            assert_eq!(master.len(), 24);
            let magic = 0x54434800u32 + version as u32;
            assert_eq!(
                &master[..4],
                if big {
                    magic.to_be_bytes()
                } else {
                    magic.to_le_bytes()
                }
            );
            assert_eq!(files.len(), if version == 15 { 10 } else { 11 });
        }
    }
    #[test]
    fn rockbox_release_fixtures_preserve_numeric_history_and_strings() {
        for version in [15, 16] {
            for endian in ["le", "be"] {
                let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("tests/fixtures/rockbox/v{version}-{endian}"));
                let files = std::fs::read_dir(dir)
                    .unwrap()
                    .map(|e| {
                        let e = e.unwrap();
                        (
                            e.file_name().to_string_lossy().into_owned(),
                            std::fs::read(e.path()).unwrap(),
                        )
                    })
                    .collect();
                let db = RbDatabase::decode(&files).unwrap();
                assert_eq!(db.entries[0].path(), "/Music/song.mp3");
                assert_eq!(db.entries[0].strings[3], "Zèbre".as_bytes());
                assert_eq!(
                    &db.entries[0].numbers[15..23],
                    &[12, 7, 765432, 44, 9, 100, 1500, 4500]
                );
                let next = RbDatabase::decode(&db.encode().unwrap()).unwrap();
                assert_eq!(next.entries[0].strings, db.entries[0].strings);
                assert_eq!(&next.entries[0].numbers[13..], &db.entries[0].numbers[13..]);
            }
        }
    }
    #[test]
    fn rockbox_rejects_corrupt_offsets_and_unknown_versions() {
        let mut db = RbDatabase::empty(16, false);
        let mut e = Entry::default();
        e.strings[4] = "/Music/a.mp3".into();
        db.entries.push(e);
        let mut files = db.encode().unwrap();
        files.get_mut(MASTER).unwrap()[24..28].copy_from_slice(&99i32.to_le_bytes());
        assert!(RbDatabase::decode(&files).is_err());
        files.get_mut(MASTER).unwrap()[0] = 0;
        assert!(RbDatabase::decode(&files).is_err());
    }
    fn one() -> RbDatabase {
        let mut db = RbDatabase::empty(16, false);
        let mut e = Entry::default();
        for tag in STRINGS {
            e.strings[tag] = "<Untagged>".into();
        }
        e.strings[4] = "/Music/song.mp3".into();
        e.numbers[16] = 7;
        e.numbers[15] = 12;
        db.entries.push(e);
        db
    }
    fn save(dir: &Path, files: &BTreeMap<String, Vec<u8>>) {
        for (name, data) in files {
            std::fs::write(dir.join(name), data).unwrap();
        }
    }
    #[test]
    fn rockbox_rejects_corrupt_master_size_and_pending_numeric_commit() {
        for at in [4, 20] {
            let mut files = one().encode().unwrap();
            files.get_mut(MASTER).unwrap()[at..at + 4].copy_from_slice(&1i32.to_le_bytes());
            assert!(RbDatabase::decode(&files).is_err());
        }
    }
    #[test]
    fn rockbox_publication_preserves_prior_database_and_invalidates_ram_state() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        save(dir, &encode_empty(16, false).unwrap());
        std::fs::write(dir.join("database_state.tcd"), b"old ram").unwrap();
        publish(dir, &one().encode().unwrap(), None).unwrap();
        let db = load(dir).unwrap().unwrap();
        assert_eq!(db.entries[0].numbers[16], 7);
        assert!(!dir.join("database_state.tcd").exists());
        assert!(!dir.join(JOURNAL).exists());
        assert!(!dir.join(BACKUP).exists());
    }
    #[test]
    fn rockbox_interrupted_publication_rolls_back_and_cleanup_is_restartable() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let old = encode_empty(16, false).unwrap();
        let new = one().encode().unwrap();
        save(dir, &old);
        std::fs::create_dir(dir.join(BACKUP)).unwrap();
        save(&dir.join(BACKUP), &old);
        let mut names: Vec<_> = old
            .keys()
            .filter(|n| n.as_str() != MASTER)
            .cloned()
            .collect();
        names.push(MASTER.into());
        let changes = names
            .iter()
            .map(|name| Change {
                name: name.clone(),
                old: Some(hash(&old[name])),
                new: Some(hash(&new[name])),
            })
            .collect();
        atomic_file(
            &dir.join(JOURNAL),
            &serde_json::to_vec(&Journal {
                prepared: true,
                committed: false,
                changes,
            })
            .unwrap(),
        )
        .unwrap();
        atomic_file(&dir.join(&names[0]), &new[&names[0]]).unwrap();
        recover(dir).unwrap();
        for (name, bytes) in &old {
            assert_eq!(&std::fs::read(dir.join(name)).unwrap(), bytes);
        }
        let changes = vec![Change {
            name: MASTER.into(),
            old: Some("irrelevant".into()),
            new: Some(hash(&old[MASTER])),
        }];
        atomic_file(
            &dir.join(JOURNAL),
            &serde_json::to_vec(&Journal {
                prepared: true,
                committed: true,
                changes,
            })
            .unwrap(),
        )
        .unwrap();
        recover(dir).unwrap();
        assert!(!dir.join(JOURNAL).exists());
        recover(dir).unwrap();
    }
    #[test]
    fn rockbox_recovery_preserves_external_mutations() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        std::fs::create_dir(dir.join(BACKUP)).unwrap();
        std::fs::write(dir.join(MASTER), b"external").unwrap();
        let changes = vec![Change {
            name: MASTER.into(),
            old: None,
            new: Some("ours".into()),
        }];
        atomic_file(
            &dir.join(JOURNAL),
            &serde_json::to_vec(&Journal {
                prepared: true,
                committed: false,
                changes,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(recover(dir).is_err());
        assert_eq!(std::fs::read(dir.join(MASTER)).unwrap(), b"external");
        assert!(dir.join(JOURNAL).exists());
    }
    #[test]
    fn rockbox_recovery_handles_interrupted_backup_preparation() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let files = one().encode().unwrap();
        save(dir, &files);
        let changes = vec![Change {
            name: MASTER.into(),
            old: Some(hash(&files[MASTER])),
            new: Some("future".into()),
        }];
        atomic_file(
            &dir.join(JOURNAL),
            &serde_json::to_vec(&Journal {
                prepared: false,
                committed: false,
                changes,
            })
            .unwrap(),
        )
        .unwrap();
        recover(dir).unwrap();
        assert_eq!(std::fs::read(dir.join(MASTER)).unwrap(), files[MASTER]);
        assert!(!dir.join(JOURNAL).exists());
    }
    #[tokio::test]
    async fn rockbox_ratings_retry_removed_tracks_and_preserve_later_server_edits() {
        use crate::providers::{CredentialKind, MediaProvider, ProviderCredentials};
        use mockito::{Matcher, Server};
        use std::sync::{
            Arc,
            atomic::{AtomicU8, Ordering},
        };
        let mut server = Server::new_async().await;
        let rating = Arc::new(AtomicU8::new(0));
        let _ping = server
            .mock("GET", "/rest/ping.view")
            .match_query(Matcher::Any)
            .with_body(
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome"}}"#,
            )
            .create_async()
            .await;
        let state = rating.clone();
        let reads=server.mock("GET","/rest/getSong.view").match_query(Matcher::UrlEncoded("id".into(),"track".into())).with_body_from_request(move |_|format!(r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1","song":{{"id":"track","userRating":{}}}}}}}"#,state.load(Ordering::SeqCst)).into_bytes()).expect(8).create_async().await;
        let rejected = server
            .mock("GET", "/rest/setRating.view")
            .match_query(Matcher::UrlEncoded("rating".into(), "4".into()))
            .with_status(503)
            .expect(1)
            .create_async()
            .await;
        let provider: Arc<dyn MediaProvider> = Arc::new(
            crate::providers::subsonic::SubsonicProvider::from_stored_config(
                ProviderCredentials {
                    server_url: server.url(),
                    credential: CredentialKind::Password {
                        username: "user".into(),
                        password: "secret".into(),
                    },
                },
                true,
                None,
            )
            .unwrap(),
        );
        let db = crate::db::Database::memory().unwrap();
        let local = db
            .upsert_server(
                &server.url(),
                "openSubsonic",
                "user",
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let portable = db
            .list_servers()
            .unwrap()
            .into_iter()
            .find(|s| s.id == local)
            .unwrap()
            .server_id
            .unwrap();
        let providers = HashMap::from([(portable.clone(), provider)]);
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        let files = one().encode().unwrap();
        save(&dir, &files);
        let mut before = crate::device::DeviceManifest::default();
        before.device_id = "device".into();
        let mut track = item();
        track.server_id = Some(portable);
        before.synced_items = vec![track.clone()];
        before.rockbox.import_ratings = true;
        let mut session = Session::open(root.path(), before).unwrap();
        let cancel = crate::sync::SyncOperationManager::new();
        assert!(
            !session
                .import_ratings(&db, &providers, &cancel, "test")
                .await
                .unwrap()
                .is_empty()
        );
        rejected.assert_async().await;
        rejected.remove_async().await;
        session.before.synced_items.clear();
        let state = rating.clone();
        let accepted = server
            .mock("GET", "/rest/setRating.view")
            .match_query(Matcher::UrlEncoded("rating".into(), "4".into()))
            .with_body_from_request(move |_| {
                state.store(4, Ordering::SeqCst);
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#.into()
            })
            .expect(1)
            .create_async()
            .await;
        assert!(
            session
                .import_ratings(&db, &providers, &cancel, "test")
                .await
                .unwrap()
                .is_empty()
        );
        accepted.assert_async().await;
        rating.store(2, Ordering::SeqCst);
        session.before.synced_items = vec![track];
        assert!(
            session
                .import_ratings(&db, &providers, &cancel, "test")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(rating.load(Ordering::SeqCst), 2);
        session.db.entries[0].numbers[16] = 0;
        let state = rating.clone();
        let clear = server
            .mock("GET", "/rest/setRating.view")
            .match_query(Matcher::UrlEncoded("rating".into(), "0".into()))
            .with_body_from_request(move |_| {
                state.store(0, Ordering::SeqCst);
                r#"{"subsonic-response":{"status":"ok","version":"1.16.1"}}"#.into()
            })
            .expect(1)
            .create_async()
            .await;
        assert!(
            session
                .import_ratings(&db, &providers, &cancel, "test")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(rating.load(Ordering::SeqCst), 0);
        assert!(session.prepare_rating_backfills(None,&session.before,&db,&providers,&cancel,"test",&mut |_| panic!("a device clear must not backfill")).await.unwrap().is_none());
        clear.assert_async().await;
        reads.assert_async().await;
        for (name, bytes) in files {
            assert_eq!(std::fs::read(dir.join(name)).unwrap(), bytes);
        }
    }
    fn item() -> crate::device::SyncedItem {
        serde_json::from_value(serde_json::json!({"providerItemId":"track","name":"Song","localPath":"Music/song.mp3","sizeBytes":1,"syncedAt":"now","serverId":"server"})).unwrap()
    }
    fn backfill_fixture(server: &mockito::Server, version: u8, big: bool, raw: i32) -> (
        tempfile::TempDir, crate::db::Database, crate::device::DeviceManifest,
        HashMap<String,std::sync::Arc<dyn crate::providers::MediaProvider>>,
    ) {
        use crate::providers::{CredentialKind,ProviderCredentials};
        let db = crate::db::Database::memory().unwrap();
        db.upsert_server(&server.url(),"openSubsonic","user",None,None,None,None).unwrap();
        let portable = db.list_servers().unwrap()[0].server_id.clone().unwrap();
        let provider: std::sync::Arc<dyn crate::providers::MediaProvider> = std::sync::Arc::new(
            crate::providers::subsonic::SubsonicProvider::from_stored_config(ProviderCredentials {
                server_url: server.url(), credential: CredentialKind::Password {username:"user".into(),password:"secret".into()},
            },true,None).unwrap());
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        let mut database = one();
        database.version = version; database.big = big; database.entries[0].numbers[16] = raw;
        let mut deleted = database.entries[0].clone(); deleted.flags = 1; deleted.numbers[16] = 7;
        deleted.strings[4] = "/Old/deleted.mp3".into();
        database.entries.insert(0,deleted);
        let mut files = database.encode().unwrap();
        // The writer compacts deletion flags; set one in the native disk fixture.
        let slots = if version == 15 {22} else {23};
        files.get_mut(MASTER).unwrap()[24 + slots * 4..28 + slots * 4]
            .copy_from_slice(&if big {1i32.to_be_bytes()} else {1i32.to_le_bytes()});
        save(&dir,&files);
        let mut manifest = crate::device::DeviceManifest::default();
        manifest.device_id = "backfill-device".into();
        manifest.rockbox.import_ratings = true;
        let mut track = item(); track.server_id = Some(portable.clone()); manifest.synced_items.push(track);
        (root,db,manifest,HashMap::from([(portable,provider)]))
    }
    #[tokio::test]
    async fn rockbox_backfill_patches_only_rating_words_and_does_not_echo() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).expect(4).create_async().await;
        let page = server.mock("GET","/rest/search3.view").match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("query".into(),"".into()),Matcher::UrlEncoded("songOffset".into(),"0".into())]))
            .with_body(r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"track","userRating":4}]}}}"#).expect(4).create_async().await;
        let writes = server.mock("GET","/rest/setRating.view").match_query(Matcher::Any).expect(0).create_async().await;
        let reads = server.mock("GET","/rest/getSong.view").match_query(Matcher::Any).expect(0).create_async().await;
        for version in [15,16] { for big in [false,true] {
            let (root,db,manifest,providers) = backfill_fixture(&server,version,big,0);
            let dir = root.path().join(".rockbox");
            let original = load_files(&dir).unwrap().unwrap();
            let session = Session::open(root.path(),manifest.clone()).unwrap();
            let cancel = crate::sync::SyncOperationManager::new();
            assert!(session.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
            let update = session.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!("unexpected warning")).await.unwrap().unwrap();
            // There is deliberately no audio file: ratings-only must not probe it.
            for (name,bytes) in &original {
                let mut expected = bytes.clone();
                if name == MASTER {
                    let slots = if version == 15 {22} else {23};
                    let offset = 24 + (slots + 1)*4 + physical(16,version)*4;
                    expected[offset..offset+4].copy_from_slice(&if big {8i32.to_be_bytes()} else {8i32.to_le_bytes()});
                }
                assert_eq!(update.files[name],expected);
            }
            update.publish().await.unwrap();
            session.confirm_rating_backfills(&db).unwrap();
            let next = Session::open(root.path(),manifest.clone()).unwrap();
            assert!(next.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
            assert!(next.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!("unexpected warning")).await.unwrap().is_none());
            assert_eq!(load(&dir).unwrap().unwrap().entries[0].numbers[16],8);
            assert_eq!(db.conn.lock().unwrap().query_row("SELECT count(*) FROM rockbox_rating_backfills",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        }}
        ping.assert_async().await; page.assert_async().await; writes.assert_async().await; reads.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_backfill_recovers_unpublished_and_published_intents_without_echo() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let page = server.mock("GET","/rest/search3.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"track","userRating":3}]}}}"#).expect(2).create_async().await;
        let writes = server.mock("GET","/rest/setRating.view").match_query(Matcher::Any).expect(0).create_async().await;
        let (root,db,manifest,providers) = backfill_fixture(&server,16,false,0);
        let cancel = crate::sync::SyncOperationManager::new();
        let session = Session::open(root.path(),manifest.clone()).unwrap();
        let abandoned = session.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().unwrap();
        drop(abandoned);
        let retry = Session::open(root.path(),manifest.clone()).unwrap();
        assert!(retry.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
        let update = retry.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().unwrap();
        update.publish().await.unwrap(); // Simulate process exit before local acknowledgement.
        let recovered = Session::open(root.path(),manifest).unwrap();
        assert!(recovered.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
        let row: (i32,i32) = db.conn.lock().unwrap().query_row("SELECT observed,confirmed FROM rockbox_ratings",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(row,(6,6));
        page.assert_async().await; writes.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_backfill_skips_unsafe_disabled_and_cleared_candidates() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let requests = server.mock("GET","/rest/search3.view").match_query(Matcher::Any).expect(0).create_async().await;
        let (root,db,manifest,providers) = backfill_fixture(&server,16,false,0);
        let cancel = crate::sync::SyncOperationManager::new();
        let account = crate::providers::feedback::account_scope("user");
        let server_id = manifest.synced_items[0].server_id.as_ref().unwrap();
        for case in ["disabled","prefix","account","duplicate","aliased-path","pending-clear","current-clear","uncertain-seed","unsupported-source","unverified-source"] {
            let mut after = manifest.clone();
            let session = Session::open(root.path(),manifest.clone()).unwrap();
            match case {
                "disabled" => after.rockbox.import_ratings = false,
                "prefix" => after.rockbox.path_prefix = "/other".into(),
                "account" => { db.conn.lock().unwrap().execute("UPDATE server_config SET username='different'",[]).unwrap(); },
                "duplicate" => after.synced_items.push(after.synced_items[0].clone()),
                "aliased-path" => {
                    let mut alias = after.synced_items[0].clone();
                    alias.local_path = alias.local_path.replace('/', "\\");
                    alias.jellyfin_id = "other-source".into();
                    after.synced_items.push(alias);
                },
                "unsupported-source" => after.synced_items[0].server_id = None,
                "unverified-source" => after.synced_items[0].jellyfin_id = "other".into(),
                "pending-clear" => { observe_rating(&db,&manifest.device_id,server_id,&account,"track",7).unwrap(); observe_rating(&db,&manifest.device_id,server_id,&account,"track",0).unwrap(); },
                "current-clear" => { session.cleared_ratings.lock().unwrap().insert((server_id.clone(),account.clone(),"track".into())); },
                "uncertain-seed" => {
                    db.conn.lock().unwrap().execute("INSERT INTO rockbox_rating_backfills VALUES(?1,?2,?3,'track',8,'different')",rusqlite::params![manifest.device_id,server_id,account]).unwrap();
                    assert_eq!(session.import_ratings(&db,&providers,&cancel,"test").await.unwrap().len(),1);
                },
                _ => unreachable!(),
            }
            let result = session.prepare_rating_backfills(None,&after,&db,&providers,&cancel,"test",&mut |_| panic!()).await;
            if case == "prefix" { assert!(result.is_err()); } else { assert!(result.unwrap().is_none(),"{case}"); }
            db.conn.lock().unwrap().execute("UPDATE server_config SET username='user'",[]).unwrap();
            db.conn.lock().unwrap().execute("DELETE FROM rockbox_ratings",[]).unwrap();
            db.conn.lock().unwrap().execute("DELETE FROM rockbox_rating_backfills",[]).unwrap();
        }
        requests.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_backfill_rechecks_stable_zero_and_preserves_old_ledger_on_migration() {
        use mockito::Matcher;
        use std::sync::{Arc,atomic::{AtomicU8,Ordering}};
        let mut server = mockito::Server::new_async().await;
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let rating = Arc::new(AtomicU8::new(0));
        let state = rating.clone();
        let page = server.mock("GET","/rest/search3.view").match_query(Matcher::Any)
            .with_body_from_request(move |_| serde_json::json!({"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"track","userRating":state.load(Ordering::SeqCst)}]}}}).to_string().into_bytes())
            .expect(2).create_async().await;
        let (root,db,manifest,providers) = backfill_fixture(&server,16,false,0);
        let cancel = crate::sync::SyncOperationManager::new();
        let session = Session::open(root.path(),manifest.clone()).unwrap();
        assert!(session.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
        assert!(session.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().is_none());
        // Re-running schema initialization preserves an existing outbound baseline.
        db.init_for_test().unwrap(); db.init_for_test().unwrap();
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT observed FROM rockbox_ratings",[],|r|r.get::<_,i32>(0)).unwrap(),0);
        rating.store(2,Ordering::SeqCst);
        let next = Session::open(root.path(),manifest.clone()).unwrap();
        let prepared = next.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().unwrap();
        prepared.publish().await.unwrap(); next.confirm_rating_backfills(&db).unwrap();
        assert_eq!(load(&root.path().join(".rockbox")).unwrap().unwrap().entries[0].numbers[16],4);
        page.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_backfill_releases_retryable_intents_before_unrated_maintenance() {
        use mockito::Matcher;
        use std::sync::{Arc,atomic::{AtomicU8,Ordering}};
        let mut server = mockito::Server::new_async().await;
        let rating = Arc::new(AtomicU8::new(4));
        let state = rating.clone();
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let page = server.mock("GET","/rest/search3.view").match_query(Matcher::Any)
            .with_body_from_request(move |_| serde_json::json!({"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"track","userRating":state.load(Ordering::SeqCst)}]}}}).to_string().into_bytes())
            .expect(3).create_async().await;
        let writes = server.mock("GET","/rest/setRating.view").match_query(Matcher::Any).expect(0).create_async().await;
        let (root,db,mut manifest,providers) = backfill_fixture(&server,16,false,0);
        let cancel = crate::sync::SyncOperationManager::new();
        let session = Session::open(root.path(),manifest.clone()).unwrap();
        session.import_ratings(&db,&providers,&cancel,"test").await.unwrap();
        drop(session.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap());
        rating.store(0,Ordering::SeqCst);
        manifest.rockbox.enabled = true;
        write_audio(root.path(),"Music/song.mp3");
        let mut maintenance = Session::open(root.path(),manifest.clone()).unwrap();
        assert!(maintenance.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT count(*) FROM rockbox_rating_backfills",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        let prepared = maintenance.prepare_update(&manifest,&io,&cancel,"test",&mut |_| panic!()).await.unwrap();
        maintenance.prepare_rating_backfills(Some(prepared),&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().unwrap().publish().await.unwrap();
        rating.store(3,Ordering::SeqCst);
        let next = Session::open(root.path(),manifest.clone()).unwrap();
        assert!(next.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
        next.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().unwrap().publish().await.unwrap();
        next.confirm_rating_backfills(&db).unwrap();
        assert_eq!(load(&root.path().join(".rockbox")).unwrap().unwrap().entries[0].numbers[16],6);
        page.assert_async().await; writes.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_backfill_reports_unavailable_sources() {
        let server = mockito::Server::new_async().await;
        let (root,db,manifest,_) = backfill_fixture(&server,16,false,0);
        db.conn.lock().unwrap().execute("UPDATE server_config SET server_type='unsupported-source'",[]).unwrap();
        let session = Session::open(root.path(),manifest.clone()).unwrap();
        let mut warnings = Vec::new();
        assert!(session.prepare_rating_backfills(None,&manifest,&db,&HashMap::new(),&crate::sync::SyncOperationManager::new(),"test",&mut |w|{warnings.push(w);Ok(())}).await.unwrap().is_none());
        assert_eq!(warnings.len(),1);
        assert!(warnings[0].contains("unavailable"));
    }
    #[tokio::test]
    async fn rockbox_backfill_restores_server_ratings_when_database_is_recreated() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let page = server.mock("GET","/rest/search3.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"track","userRating":4}]}}}"#).expect(6).create_async().await;
        let writes = server.mock("GET","/rest/setRating.view").match_query(Matcher::Any).expect(0).create_async().await;
        for version in [15,16] {for history in [None,Some(0),Some(9)] {
            let (root,db,mut manifest,providers) = backfill_fixture(&server,version,false,0);
            manifest.rockbox.enabled = true;
            manifest.rockbox.format = if version == 15 {"3.15"} else {"4.0"}.into();
            let dir = root.path().join(".rockbox");
            for name in load_files(&dir).unwrap().unwrap().keys() {std::fs::remove_file(dir.join(name)).unwrap();}
            let source = manifest.synced_items[0].server_id.as_ref().unwrap();
            let account = crate::providers::feedback::account_scope("user");
            if let Some(raw) = history {
                observe_rating(&db,&manifest.device_id,source,&account,"track",raw).unwrap();
                db.conn.lock().unwrap().execute("UPDATE rockbox_ratings SET confirmed=observed",[]).unwrap();
            }
            write_audio(root.path(),"Music/song.mp3");
            let mut session = Session::open(root.path(),manifest.clone()).unwrap();
            let cancel = crate::sync::SyncOperationManager::new();
            assert!(session.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            let prepared = session.prepare_update(&manifest,&io,&cancel,"test",&mut |_| panic!()).await.unwrap();
            session.prepare_rating_backfills(Some(prepared),&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().unwrap().publish().await.unwrap();
            session.confirm_rating_backfills(&db).unwrap();
            assert_eq!(load(&dir).unwrap().unwrap().entries[0].numbers[16],8);
        }}
        page.assert_async().await; writes.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_recreated_zero_baseline_survives_lookup_failure_and_interrupted_ack() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let unavailable = server.mock("GET","/rest/search3.view").match_query(Matcher::Any)
            .with_status(503).expect(2).create_async().await;
        let writes = server.mock("GET","/rest/setRating.view").match_query(Matcher::Any).expect(0).create_async().await;
        let (root,db,mut manifest,providers) = backfill_fixture(&server,16,false,0);
        manifest.rockbox.enabled = true;
        let dir = root.path().join(".rockbox");
        for name in load_files(&dir).unwrap().unwrap().keys() {std::fs::remove_file(dir.join(name)).unwrap();}
        let source = manifest.synced_items[0].server_id.as_ref().unwrap();
        let account = crate::providers::feedback::account_scope("user");
        observe_rating(&db,&manifest.device_id,source,&account,"track",9).unwrap();
        db.conn.lock().unwrap().execute("UPDATE rockbox_ratings SET confirmed=9",[]).unwrap();
        write_audio(root.path(),"Music/song.mp3");
        let cancel = crate::sync::SyncOperationManager::new();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        for publish in [false,true] {
            let mut session = Session::open(root.path(),manifest.clone()).unwrap();
            assert!(session.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
            let prepared = session.prepare_update(&manifest,&io,&cancel,"test",&mut |_| panic!()).await.unwrap();
            let mut warnings = Vec::new();
            let prepared = session.prepare_rating_backfills(Some(prepared),&manifest,&db,&providers,&cancel,"test",&mut |w|{warnings.push(w);Ok(())}).await.unwrap().unwrap();
            assert_eq!(warnings.len(),1);
            let row: (i32,i32,bool) = db.conn.lock().unwrap().query_row("SELECT observed,confirmed,initialized_zero FROM rockbox_ratings",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            assert_eq!(row,(9,9,false));
            if publish {prepared.publish().await.unwrap();} // Exit before local acknowledgement.
        }
        unavailable.assert_async().await; unavailable.remove_async().await;
        let page = server.mock("GET","/rest/search3.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"track","userRating":4}]}}}"#).expect(1).create_async().await;
        let next = Session::open(root.path(),manifest.clone()).unwrap();
        assert!(next.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
        let row: (i32,i32,bool) = db.conn.lock().unwrap().query_row("SELECT observed,confirmed,initialized_zero FROM rockbox_ratings",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(row,(9,9,true));
        next.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().unwrap().publish().await.unwrap();
        next.confirm_rating_backfills(&db).unwrap();
        assert_eq!(load(&dir).unwrap().unwrap().entries[0].numbers[16],8);
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT initialized_zero FROM rockbox_ratings",[],|r|r.get::<_,bool>(0)).unwrap(),false);
        page.assert_async().await; writes.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_recreation_preserves_pending_positive_and_clear_intents() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let reads = server.mock("GET","/rest/getSong.view").match_query(Matcher::Any).with_status(503).expect(4).create_async().await;
        let pages = server.mock("GET","/rest/search3.view").match_query(Matcher::Any).expect(0).create_async().await;
        let writes = server.mock("GET","/rest/setRating.view").match_query(Matcher::Any).expect(0).create_async().await;
        for raw in [0,9] {
            let (root,db,mut manifest,providers) = backfill_fixture(&server,16,false,0);
            manifest.rockbox.enabled = true;
            let dir = root.path().join(".rockbox");
            for name in load_files(&dir).unwrap().unwrap().keys() {std::fs::remove_file(dir.join(name)).unwrap();}
            let source = manifest.synced_items[0].server_id.as_ref().unwrap();
            let account = crate::providers::feedback::account_scope("user");
            observe_rating(&db,&manifest.device_id,source,&account,"track",9).unwrap();
            observe_rating(&db,&manifest.device_id,source,&account,"track",raw).unwrap();
            write_audio(root.path(),"Music/song.mp3");
            let cancel = crate::sync::SyncOperationManager::new();
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            let mut session = Session::open(root.path(),manifest.clone()).unwrap();
            assert_eq!(session.import_ratings(&db,&providers,&cancel,"test").await.unwrap().len(),1);
            let prepared = session.prepare_update(&manifest,&io,&cancel,"test",&mut |_| panic!()).await.unwrap();
            session.prepare_rating_backfills(Some(prepared),&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().unwrap().publish().await.unwrap();
            session.confirm_rating_backfills(&db).unwrap();
            let next = Session::open(root.path(),manifest.clone()).unwrap();
            assert_eq!(next.import_ratings(&db,&providers,&cancel,"test").await.unwrap().len(),1);
            let row: (i32,Option<i32>) = db.conn.lock().unwrap().query_row("SELECT observed,confirmed FROM rockbox_ratings",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
            assert_eq!(row,(raw,None));
            assert!(next.prepare_rating_backfills(None,&manifest,&db,&providers,&cancel,"test",&mut |_| panic!()).await.unwrap().is_none());
            assert_eq!(load(&dir).unwrap().unwrap().entries[0].numbers[16],0);
        }
        reads.assert_async().await; pages.assert_async().await; writes.assert_async().await;
    }
    #[test]
    fn rockbox_initialized_zero_migration_preserves_pending_rows_and_new_device_edits() {
        let db = crate::db::Database::memory().unwrap();
        db.conn.lock().unwrap().execute("ALTER TABLE rockbox_ratings DROP COLUMN initialized_zero",[]).unwrap();
        db.conn.lock().unwrap().execute("INSERT INTO rockbox_ratings VALUES('device','server','account','track',9,NULL)",[]).unwrap();
        db.init_for_test().unwrap(); db.init_for_test().unwrap();
        let row: (i32,Option<i32>,bool) = db.conn.lock().unwrap().query_row("SELECT observed,confirmed,initialized_zero FROM rockbox_ratings",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(row,(9,None,false));
        confirm_backfill(&db,"device","server","account","track",0).unwrap();
        db.conn.lock().unwrap().execute("UPDATE rockbox_ratings SET confirmed=9",[]).unwrap();
        observe_rating(&db,"device","server","account","track",9).unwrap();
        let row: (i32,Option<i32>,bool) = db.conn.lock().unwrap().query_row("SELECT observed,confirmed,initialized_zero FROM rockbox_ratings",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(row,(9,None,false));
    }
    #[tokio::test]
    async fn rockbox_recreated_zero_recovery_imports_a_new_device_rating() {
        use mockito::Matcher;
        use std::sync::{Arc,atomic::{AtomicU8,Ordering}};
        let mut server = mockito::Server::new_async().await;
        let rating = Arc::new(AtomicU8::new(2));
        let read_state = rating.clone();
        let write_state = rating.clone();
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let _reads = server.mock("GET","/rest/getSong.view").match_query(Matcher::Any)
            .with_body_from_request(move |_| serde_json::json!({"subsonic-response":{"status":"ok","song":{"id":"track","userRating":read_state.load(Ordering::SeqCst)}}}).to_string().into_bytes()).create_async().await;
        let write = server.mock("GET","/rest/setRating.view").match_query(Matcher::UrlEncoded("rating".into(),"5".into()))
            .with_body_from_request(move |_| {write_state.store(5,Ordering::SeqCst);br#"{"subsonic-response":{"status":"ok"}}"#.to_vec()}).expect(1).create_async().await;
        // Initialization was published without acknowledgement, then the user rated the track.
        let (root,db,manifest,providers) = backfill_fixture(&server,16,false,9);
        let source = manifest.synced_items[0].server_id.as_ref().unwrap();
        let account = crate::providers::feedback::account_scope("user");
        observe_rating(&db,&manifest.device_id,source,&account,"track",9).unwrap();
        db.conn.lock().unwrap().execute("UPDATE rockbox_ratings SET confirmed=9",[]).unwrap();
        db.conn.lock().unwrap().execute("INSERT INTO rockbox_rating_backfills VALUES(?1,?2,?3,'track',0,'before-create')",rusqlite::params![manifest.device_id,source,account]).unwrap();
        let session = Session::open(root.path(),manifest).unwrap();
        assert!(session.import_ratings(&db,&providers,&crate::sync::SyncOperationManager::new(),"test").await.unwrap().is_empty());
        assert_eq!(rating.load(Ordering::SeqCst),5);
        let row: (i32,i32,bool) = db.conn.lock().unwrap().query_row("SELECT observed,confirmed,initialized_zero FROM rockbox_ratings",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(row,(9,9,false));
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT count(*) FROM rockbox_rating_backfills",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        write.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_restored_missing_audio_receives_server_rating_in_an_existing_database() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let page = server.mock("GET","/rest/search3.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"track","userRating":4}]}}}"#).expect(2).create_async().await;
        let reads = server.mock("GET","/rest/getSong.view").match_query(Matcher::Any).expect(0).create_async().await;
        let writes = server.mock("GET","/rest/setRating.view").match_query(Matcher::Any).expect(0).create_async().await;
        let (root,db,mut manifest,providers) = backfill_fixture(&server,16,false,0);
        manifest.rockbox.enabled = true;
        let dir = root.path().join(".rockbox");
        for name in load_files(&dir).unwrap().unwrap().keys() {std::fs::remove_file(dir.join(name)).unwrap();}
        let server_id = manifest.synced_items[0].server_id.as_ref().unwrap().clone();
        let account = crate::providers::feedback::account_scope("user");
        observe_rating(&db,&manifest.device_id,&server_id,&account,"track",9).unwrap();
        db.conn.lock().unwrap().execute("UPDATE rockbox_ratings SET confirmed=9",[]).unwrap();
        let mut good = item();good.local_path="Music/good.mp3".into();good.jellyfin_id="good".into();good.server_id=Some(server_id);
        manifest.synced_items.push(good);
        write_audio(root.path(),"Music/good.mp3");
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        let cancel = crate::sync::SyncOperationManager::new();
        for restored in [false,true] {
            if restored {write_audio(root.path(),"Music/song.mp3");}
            let mut session=Session::open(root.path(),manifest.clone()).unwrap();
            assert!(session.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
            let mut warnings=Vec::new();let mut warn=|w|{warnings.push(w);Ok(())};
            let prepared=session.prepare_update(&manifest,&io,&cancel,"test",&mut warn).await.unwrap();
            session.prepare_rating_backfills(Some(prepared),&manifest,&db,&providers,&cancel,"test",&mut warn).await.unwrap().unwrap().publish().await.unwrap();
            session.confirm_rating_backfills(&db).unwrap();
            assert_eq!(warnings.len(),if restored {0} else {1});
        }
        let database=load(&dir).unwrap().unwrap();
        assert_eq!(database.entries.iter().find(|e|e.path()=="/Music/song.mp3").unwrap().numbers[16],8);
        let next=Session::open(root.path(),manifest).unwrap();
        assert!(next.import_ratings(&db,&providers,&cancel,"test").await.unwrap().is_empty());
        page.assert_async().await;reads.assert_async().await;writes.assert_async().await;
    }
    #[tokio::test]
    async fn rockbox_backfill_includes_newly_indexed_tracks_but_not_failed_metadata() {
        use mockito::Matcher;
        let mut server = mockito::Server::new_async().await;
        let _ping = server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","type":"navidrome"}}"#).create_async().await;
        let page = server.mock("GET","/rest/search3.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","searchResult3":{"song":[{"id":"new","userRating":5}]}}}"#).expect(1).create_async().await;
        for valid in [true,false] {
            let (root,db,mut manifest,providers) = backfill_fixture(&server,16,false,0);
            let mut session = Session::open(root.path(),manifest.clone()).unwrap();
            manifest.rockbox.enabled = true;
            manifest.synced_items[0].jellyfin_id = "new".into();
            manifest.synced_items[0].local_path = "Music/new.wav".into();
            write_audio(root.path(),"Music/new.wav");
            if !valid { std::fs::write(root.path().join("Music/new.wav"),b"invalid audio").unwrap(); }
            let cancel = crate::sync::SyncOperationManager::new();
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            let mut warnings = Vec::new();
            let mut warn = |w| {warnings.push(w);Ok(())};
            let prepared = session.prepare_update(&manifest,&io,&cancel,"test",&mut warn).await.unwrap();
            let prepared = session.prepare_rating_backfills(Some(prepared),&manifest,&db,&providers,&cancel,"test",&mut warn).await.unwrap().unwrap();
            prepared.publish().await.unwrap();
            session.confirm_rating_backfills(&db).unwrap();
            let database = load(&root.path().join(".rockbox")).unwrap().unwrap();
            if valid {
                assert!(warnings.is_empty());
                assert_eq!(database.entries.len(),1);
                assert_eq!(database.entries[0].path(),"/Music/new.wav");
                assert_eq!(database.entries[0].numbers[16],10);
            } else {
                assert_eq!(warnings.len(),1);
                assert!(database.entries.is_empty());
            }
        }
        page.assert_async().await;
    }
    #[test]
    fn rockbox_review_1_matches_only_configured_prefix_on_component_boundaries() {
        for (prefix, path, want) in [
            ("/<microSD0>", "/<microSD1>/Music/song.mp3", None),
            ("/foo", "/foobar/Music/song.mp3", None),
            ("/foo", "/foo/Music/song.mp3", Some(0)),
            ("/foo/bar/", "/foo/bar/Music/song.mp3", Some(0)),
        ] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox");
            std::fs::create_dir(&dir).unwrap();
            let mut db = one();
            db.entries[0].strings[4] = path.into();
            save(&dir, &db.encode().unwrap());
            let mut manifest = crate::device::DeviceManifest::default();
            manifest.rockbox.path_prefix = prefix.into();
            let session = Session::open(root.path(), manifest).unwrap();
            assert_eq!(
                session.entry_for("Music/song.mp3").unwrap(),
                want,
                "prefix={prefix}, path={path}"
            );
        }
    }
    fn write_audio(root: &Path, local: &str) {
        let path = root.join(local);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&1636u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        for value in [1u16, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&1600u32.to_le_bytes());
        bytes.resize(1644, 0);
        std::fs::write(path, bytes).unwrap();
    }
    #[tokio::test]
    async fn rockbox_bounds_new_metadata_without_changing_audio_or_existing_tags() {
        use id3::TagLike;
        for version in [15, 16] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox");
            std::fs::create_dir(&dir).unwrap();
            let mut db = one();
            db.version = version;
            let mut foreign = db.entries[0].clone();
            foreign.strings[4] = "/Other/unmanaged.mp3".into();
            foreign.strings[6] = vec![0xe9; 6000];
            db.entries.push(foreign);
            save(&dir, &db.encode().unwrap());
            write_audio(root.path(), "Music/song.mp3");
            let path = root.path().join("Music/song.mp3");
            let wave = std::fs::read(&path).unwrap();
            let mut tags = id3::Tag::new();
            tags.set_title("Title\0ignored");
            tags.set_artist(format!("{}é", "a".repeat(519)));
            tags.set_album("b".repeat(520));
            tags.set_album_artist("Album artist");
            tags.set_genre(format!("{}😀", "c".repeat(516)));
            tags.add_frame(id3::Frame::text("TCOM", format!("{}😀", "d".repeat(517))));
            tags.add_frame(id3::Frame::text("TIT1", "\0ignored"));
            tags.add_frame(id3::frame::Comment {
                lang: "eng".into(),
                description: String::new(),
                text: "x".repeat(10_244),
            });
            let mut bytes = Vec::new();
            tags.write_to(&mut bytes, id3::Version::Id3v24).unwrap();
            bytes.extend_from_slice(&wave);
            std::fs::write(&path, &bytes).unwrap();
            let mut manifest = crate::device::DeviceManifest::default();
            manifest.rockbox.enabled = true;
            manifest.synced_items = vec![item()];
            let mut session = Session::open(root.path(), manifest.clone()).unwrap();
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            let warnings = update_session(
                &mut session,
                &manifest,
                &io,
                &crate::sync::SyncOperationManager::new(),
                "test",
            )
            .await
            .unwrap();
            let current = load(&dir).unwrap().unwrap();
            let entry = &current.entries[0];
            for (tag, expected) in [
                (0, "a".repeat(519)),
                (1, "b".repeat(520)),
                (2, format!("{}😀", "c".repeat(516))),
                (3, "Title".into()),
                (5, "d".repeat(517)),
                (6, "x".repeat(520)),
                (7, "Album artist".into()),
                (8, "<Untagged>".into()),
            ] {
                assert_eq!(entry.strings[tag], expected.as_bytes());
                assert!(std::str::from_utf8(&entry.strings[tag]).is_ok());
            }
            assert_eq!(&entry.numbers[15..17], &[12, 7]);
            assert_eq!(current.entries[1].strings[6], vec![0xe9; 6000]);
            assert!(
                warnings.iter().any(|warning| warning.contains("comment")
                    && warning.contains("10244")
                    && warning.contains("520")
                    && warning.contains("Music/song.mp3")),
                "{warnings:?}"
            );
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[tokio::test]
    async fn rockbox_indexes_id3_prefixed_wave_without_changing_audio_or_history() {
        use id3::TagLike;
        for padding in [0, 8 * 1024 * 1024] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox");
            std::fs::create_dir(&dir).unwrap();
            save(&dir, &one().encode().unwrap());
            write_audio(root.path(), "Music/song.mp3");
            let path = root.path().join("Music/song.mp3");
            let wave = std::fs::read(&path).unwrap();
            let mut tags = id3::Tag::new();
            for (id, value) in [
                ("TIT2", "My Forbidden Lover"),
                ("TPE1", "Chic"),
                ("TALB", "Risqué"),
                ("TPE2", "Album artist"),
                ("TCON", "Disco"),
                ("TCOM", "Composer"),
                ("TIT1", "Group"),
                ("TDRC", "1979-07-30"),
                ("TRCK", "4/8"),
                ("TPOS", "1/2"),
            ] {
                tags.add_frame(id3::Frame::text(id, value));
            }
            tags.add_frame(id3::frame::Comment {
                lang: "eng".into(),
                description: "".into(),
                text: "Comment".into(),
            });
            let mut bytes = Vec::new();
            tags.write_to(&mut bytes, id3::Version::Id3v24).unwrap();
            bytes.resize(bytes.len() + padding, 0);
            let size = bytes.len() - 10;
            for i in 0..4 {
                bytes[6 + i] = ((size >> (7 * (3 - i))) & 127) as u8;
            }
            bytes.extend_from_slice(&wave);
            std::fs::write(&path, &bytes).unwrap();
            let mut manifest = crate::device::DeviceManifest::default();
            manifest.rockbox.enabled = true;
            manifest.synced_items = vec![item()];
            let mut session = Session::open(root.path(), manifest.clone()).unwrap();
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            update_session(
                &mut session,
                &manifest,
                &io,
                &crate::sync::SyncOperationManager::new(),
                "test",
            )
            .await
            .unwrap();
            let current = load(&dir).unwrap().unwrap();
            let entry = &current.entries[0];
            for (tag, value) in [
                (0, "Chic"),
                (1, "Risqué"),
                (2, "Disco"),
                (3, "My Forbidden Lover"),
                (5, "Composer"),
                (6, "Comment"),
                (7, "Album artist"),
                (8, "Group"),
            ] {
                assert_eq!(entry.strings[tag], value.as_bytes());
            }
            assert_eq!(&entry.numbers[9..12], &[1979, 1, 4]);
            assert_eq!(entry.numbers[14], 100);
            assert_eq!(&entry.numbers[15..17], &[12, 7]);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[tokio::test]
    async fn rockbox_missing_audio_warns_and_does_not_infer_an_alternate_filename() {
        for format in ["3.15", "4.0"] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox");
            std::fs::create_dir(&dir).unwrap();
            write_audio(root.path(), "Music/song.mp3");
            let original = std::fs::read(root.path().join("Music/song.mp3")).unwrap();
            write_audio(root.path(), "Music/good.mp3");
            let mut manifest = crate::device::DeviceManifest::default();
            manifest.rockbox.enabled = true;
            manifest.rockbox.format = format.into();
            let mut missing = item();
            missing.local_path = "Music/song [cbe52c53].mp3".into();
            let mut good = item(); good.local_path = "Music/good.mp3".into(); good.jellyfin_id = "good".into();
            manifest.synced_items = vec![missing,good];
            let mut session = Session::open(root.path(),manifest.clone()).unwrap();
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            let warnings = update_session(&mut session,&manifest,&io,&crate::sync::SyncOperationManager::new(),"test").await.unwrap();
            assert_eq!(warnings.len(),1);
            assert!(warnings[0].contains("Music/song [cbe52c53].mp3"));
            let current = load(&dir).unwrap().unwrap();
            assert_eq!(current.entries.len(),1);
            assert_eq!(current.entries[0].path(),"/Music/good.mp3");
            assert_eq!(std::fs::read(root.path().join("Music/song.mp3")).unwrap(),original);
        }
    }
    #[tokio::test]
    async fn rockbox_missing_audio_keeps_same_or_moved_source_history_for_retry() {
        for moved in [false,true] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox");
            std::fs::create_dir(&dir).unwrap();
            save(&dir,&one().encode().unwrap());
            let mut before = crate::device::DeviceManifest::default();
            before.rockbox.enabled = true;
            before.synced_items = vec![item()];
            let mut session = Session::open(root.path(),before.clone()).unwrap();
            let mut after = before;
            if moved {after.synced_items[0].local_path = "Music/moved.mp3".into();}
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            let cancel = crate::sync::SyncOperationManager::new();
            let warnings = update_session(&mut session,&after,&io,&cancel,"test").await.unwrap();
            assert_eq!(warnings.len(),1);
            let current = load(&dir).unwrap().unwrap();
            assert_eq!(current.entries[0].strings,one().entries[0].strings);
            for tag in (0..23).filter(|tag| !STRINGS.contains(tag)) {
                assert_eq!(current.entries[0].numbers[tag],one().entries[0].numbers[tag]);
            }
            write_audio(root.path(),&after.synced_items[0].local_path);
            let mut retry = Session::open(root.path(),after.clone()).unwrap();
            assert!(update_session(&mut retry,&after,&io,&cancel,"test").await.unwrap().is_empty());
            let current = load(&dir).unwrap().unwrap();
            assert_eq!(current.entries.len(),1);
            assert_eq!(&current.entries[0].numbers[15..17],&[12,7]);
            assert_eq!(current.entries[0].path(),format!("/{}",after.synced_items[0].local_path));
        }
    }
    #[tokio::test]
    async fn rockbox_missing_audio_cannot_claim_another_source_or_hide_warning_failure() {
        for changed_source in [false,true] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox"); std::fs::create_dir(&dir).unwrap();
            let original = one().encode().unwrap(); save(&dir,&original);
            let mut before = crate::device::DeviceManifest::default();
            before.rockbox.enabled = true; before.synced_items = vec![item()];
            let mut after = before.clone();
            if changed_source {after.synced_items[0].jellyfin_id = "other".into();}
            let mut session = Session::open(root.path(),before).unwrap();
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            let result = session.prepare_update(&after,&io,&crate::sync::SyncOperationManager::new(),"test",
                &mut |_| anyhow::bail!("warning storage failed")).await;
            assert!(result.is_err());
            let error = result.err().unwrap();
            if changed_source {assert!(format!("{error:#}").contains("Music/song.mp3"));}
            else {assert!(error.to_string().contains("warning storage failed"));}
            assert_eq!(load_files(&dir).unwrap().unwrap(),original);
        }
    }
    #[derive(Debug)]
    struct ProbeFileDisappears {
        backend: crate::device_io::MscBackend,
        remove: bool,
        recheck_error: Option<std::io::ErrorKind>,
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl crate::device_io::DeviceIO for ProbeFileDisappears {
        async fn metadata_path(&self,path:&str)->Result<Option<PathBuf>> {
            use std::sync::atomic::Ordering;
            if self.calls.fetch_add(1,Ordering::SeqCst) == 1 {
                if self.remove {
                    let exact = self.backend.metadata_path(path).await?.unwrap();
                    std::fs::remove_file(exact)?;
                }
                return Err(ffmpeg_next::Error::Other {errno:ffmpeg_next::error::ENOENT}.into());
            }
            self.backend.metadata_path(path).await
        }
        async fn file_length(&self,path:&str)->Result<Option<u64>> {
            if self.calls.load(std::sync::atomic::Ordering::SeqCst) >= 2 {
                if let Some(kind) = self.recheck_error {return Err(std::io::Error::from(kind).into());}
            }
            self.backend.file_length(path).await
        }
        async fn read_file(&self,_:&str)->Result<Vec<u8>> {unreachable!()}
        async fn write_file(&self,_:&str,_:&[u8])->Result<()> {unreachable!()}
        async fn write_with_verify(&self,_:&str,_:&[u8])->Result<()> {unreachable!()}
        async fn delete_file(&self,_:&str)->Result<()> {unreachable!()}
        async fn list_files(&self,_:&str)->Result<Vec<crate::device_io::FileEntry>> {unreachable!()}
        async fn free_space(&self)->Result<u64> {unreachable!()}
        async fn ensure_dir(&self,_:&str)->Result<()> {unreachable!()}
        async fn cleanup_empty_subdirs(&self,_:&str)->Result<()> {unreachable!()}
    }
    #[tokio::test]
    async fn rockbox_ffmpeg_missing_file_race_requires_exact_audio_to_be_missing() {
        for history in ["fresh","same","moved"] {
            for (remove,recheck_error) in [(false,None),(true,None),
                (true,Some(std::io::ErrorKind::NotFound)),(true,Some(std::io::ErrorKind::PermissionDenied))] {
                let root = tempfile::tempdir().unwrap();
                let dir = root.path().join(".rockbox"); std::fs::create_dir(&dir).unwrap();
                let original = one().encode().unwrap();
                if history != "fresh" {save(&dir,&original);}
                let mut before = crate::device::DeviceManifest::default();
                before.rockbox.enabled = true; before.synced_items = vec![item()];
                let mut after = before.clone();
                if history == "moved" {after.synced_items[0].local_path = "Music/moved.mp3".into();}
                let local = &after.synced_items[0].local_path;
                write_audio(root.path(),local);
                let mut session = Session::open(root.path(),before).unwrap();
                let io = ProbeFileDisappears {
                    backend:crate::device_io::MscBackend::new(root.path().to_path_buf()),
                    remove,recheck_error,calls:std::sync::atomic::AtomicUsize::new(0),
                };
                let result = update_session(&mut session,&after,&io,&crate::sync::SyncOperationManager::new(),"test").await;
                if remove && recheck_error != Some(std::io::ErrorKind::PermissionDenied) {
                    let warnings = result.unwrap_or_else(|error| panic!("{history}: {error:#}"));
                    assert_eq!(warnings.len(),1);
                    assert!(warnings[0].contains(local));
                    let current = load(&dir).unwrap().unwrap();
                    if history == "fresh" {assert!(current.entries.is_empty());}
                    else {assert_eq!(current.entries,RbDatabase::decode(&original).unwrap().entries);}
                } else {
                    let error = result.err().expect("FFmpeg ENOENT with existing audio must remain fatal");
                    assert!(format!("{error:#}").contains(local));
                    if recheck_error == Some(std::io::ErrorKind::PermissionDenied) {
                        assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(),std::io::ErrorKind::PermissionDenied);
                    } else {
                        assert!(matches!(error.downcast_ref::<ffmpeg_next::Error>(),Some(ffmpeg_next::Error::Other {errno}) if *errno == ffmpeg_next::error::ENOENT));
                    }
                    if history == "fresh" {assert!(!dir.join(MASTER).exists());}
                    else {assert_eq!(load_files(&dir).unwrap().unwrap(),original);}
                }
            }
        }
    }
    #[derive(Debug)]
    struct MetadataFailure(std::io::ErrorKind);
    #[async_trait::async_trait]
    impl crate::device_io::DeviceIO for MetadataFailure {
        async fn metadata_path(&self,_:&str)->Result<Option<PathBuf>> {Err(std::io::Error::from(self.0).into())}
        async fn read_file(&self,_:&str)->Result<Vec<u8>> {unreachable!()}
        async fn write_file(&self,_:&str,_:&[u8])->Result<()> {unreachable!()}
        async fn write_with_verify(&self,_:&str,_:&[u8])->Result<()> {unreachable!()}
        async fn delete_file(&self,_:&str)->Result<()> {unreachable!()}
        async fn list_files(&self,_:&str)->Result<Vec<crate::device_io::FileEntry>> {unreachable!()}
        async fn free_space(&self)->Result<u64> {unreachable!()}
        async fn ensure_dir(&self,_:&str)->Result<()> {unreachable!()}
        async fn cleanup_empty_subdirs(&self,_:&str)->Result<()> {unreachable!()}
    }
    #[tokio::test]
    async fn rockbox_missing_audio_does_not_hide_other_io_or_lost_device_root() {
        for kind in [std::io::ErrorKind::PermissionDenied,std::io::ErrorKind::Other,std::io::ErrorKind::NotFound] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox"); std::fs::create_dir(&dir).unwrap();
            let mut manifest = crate::device::DeviceManifest::default();
            manifest.rockbox.enabled = true; manifest.synced_items = vec![item()];
            let mut session = Session::open(root.path(),manifest.clone()).unwrap();
            if kind == std::io::ErrorKind::NotFound {
                std::fs::remove_file(dir.join(OWNERSHIP)).unwrap();std::fs::remove_dir(&dir).unwrap();
            }
            let error = session.prepare_update(&manifest,&MetadataFailure(kind),&crate::sync::SyncOperationManager::new(),"test",
                &mut |_| panic!("fatal IO must not become a warning")).await.err().unwrap();
            assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(),kind);
            assert!(format!("{error:#}").contains(if kind == std::io::ErrorKind::NotFound {"Rockbox directory"} else {"Music/song.mp3"}));
            assert!(!dir.join(MASTER).exists());
        }
    }
    #[test]
    fn rockbox_file_io_errors_identify_the_operation_and_path() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("missing/database_idx.tcd");
        for error in [read_bounded(&path).unwrap_err(),atomic_file(&path,b"new").unwrap_err()] {
            assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(),std::io::ErrorKind::NotFound);
            assert!(format!("{error:#}").contains("database_idx.tcd"));
            assert!(error.to_string().contains("Rockbox file"));
        }
    }
    #[tokio::test]
    async fn rockbox_unreadable_audio_keeps_history_and_updates_other_tracks() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        let original = one().encode().unwrap();
        save(&dir, &original);
        write_audio(root.path(), "Music/song.mp3");
        std::fs::write(root.path().join("Music/song.mp3"), b"not audio").unwrap();
        let mut manifest = crate::device::DeviceManifest::default();
        manifest.rockbox.enabled = true;
        manifest.synced_items = vec![item()];
        let mut session = Session::open(root.path(), manifest.clone()).unwrap();
        write_audio(root.path(), "Music/good.mp3");
        let mut good = item();
        good.local_path = "Music/good.mp3".into();
        good.jellyfin_id = "good-track".into();
        manifest.synced_items.push(good);
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        let warnings = update_session(
            &mut session,
            &manifest,
            &io,
            &crate::sync::SyncOperationManager::new(),
            "test",
        )
        .await
        .unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("Music/song.mp3"));
        assert!(warnings[0].contains("Invalid data"));
        let current = load(&dir).unwrap().unwrap();
        assert_eq!(current.entries.len(), 2);
        let retained = current
            .entries
            .iter()
            .find(|e| e.path() == "/Music/song.mp3")
            .unwrap();
        assert_eq!(retained.strings, one().entries[0].strings);
        for tag in (0..23).filter(|tag| !STRINGS.contains(tag)) {
            assert_eq!(retained.numbers[tag], one().entries[0].numbers[tag]);
        }
        assert!(
            current
                .entries
                .iter()
                .any(|e| e.path() == "/Music/good.mp3")
        );
        assert_eq!(
            std::fs::read(root.path().join("Music/song.mp3")).unwrap(),
            b"not audio"
        );
        for _ in 0..3 {
            let owners: Vec<OwnedPath> =
                serde_json::from_slice(&std::fs::read(dir.join(OWNERSHIP)).unwrap()).unwrap();
            assert_eq!(
                owners.len(),
                2,
                "Unreadable same-path retries must not duplicate ownership rows"
            );
            let mut resumed = Session::open(root.path(), manifest.clone()).unwrap();
            let warnings = update_session(
                &mut resumed,
                &manifest,
                &io,
                &crate::sync::SyncOperationManager::new(),
                "test",
            )
            .await
            .unwrap();
            assert_eq!(warnings.len(), 1);
        }
    }

    #[tokio::test]
    async fn rockbox_unreadable_moved_audio_retains_history_until_a_readable_retry() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        save(&dir, &one().encode().unwrap());
        write_audio(root.path(), "Music/new.mp3");
        std::fs::write(root.path().join("Music/new.mp3"), b"not audio").unwrap();
        let mut before = crate::device::DeviceManifest::default();
        before.rockbox.enabled = true;
        before.synced_items = vec![item()];
        let mut session = Session::open(root.path(), before.clone()).unwrap();
        let mut after = before;
        after.synced_items[0].local_path = "Music/new.mp3".into();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        update_session(
            &mut session,
            &after,
            &io,
            &crate::sync::SyncOperationManager::new(),
            "test",
        )
        .await
        .unwrap();
        assert_eq!(
            load(&dir).unwrap().unwrap().entries[0].path(),
            "/Music/song.mp3"
        );
        write_audio(root.path(), "Music/new.mp3");
        let mut resumed = Session::open(root.path(), after.clone()).unwrap();
        update_session(
            &mut resumed,
            &after,
            &io,
            &crate::sync::SyncOperationManager::new(),
            "test",
        )
        .await
        .unwrap();
        let current = load(&dir).unwrap().unwrap();
        assert_eq!(current.entries.len(), 1);
        assert_eq!(current.entries[0].path(), "/Music/new.mp3");
        assert_eq!(&current.entries[0].numbers[15..17], &[12, 7]);
    }

    #[tokio::test]
    async fn rockbox_unreadable_new_audio_is_left_unindexed() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        write_audio(root.path(), "Music/song.mp3");
        std::fs::write(root.path().join("Music/song.mp3"), b"not audio").unwrap();
        let mut manifest = crate::device::DeviceManifest::default();
        manifest.rockbox.enabled = true;
        manifest.synced_items = vec![item()];
        let mut session = Session::open(root.path(), manifest.clone()).unwrap();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        update_session(
            &mut session,
            &manifest,
            &io,
            &crate::sync::SyncOperationManager::new(),
            "test",
        )
        .await
        .unwrap();
        assert!(load(&dir).unwrap().unwrap().entries.is_empty());
    }

    #[tokio::test]
    async fn rockbox_unreadable_audio_does_not_transfer_history_to_a_changed_source() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        let files = one().encode().unwrap();
        save(&dir, &files);
        write_audio(root.path(), "Music/song.mp3");
        std::fs::write(root.path().join("Music/song.mp3"), b"not audio").unwrap();
        let mut before = crate::device::DeviceManifest::default();
        before.rockbox.enabled = true;
        before.synced_items = vec![item()];
        let mut session = Session::open(root.path(), before.clone()).unwrap();
        let mut after = before;
        after.synced_items[0].jellyfin_id = "different-source".into();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        assert!(
            update_session(
                &mut session,
                &after,
                &io,
                &crate::sync::SyncOperationManager::new(),
                "test"
            )
            .await
            .is_err()
        );
        assert_eq!(load_files(&dir).unwrap().unwrap(), files);
    }

    #[tokio::test]
    async fn rockbox_unreadable_moved_history_cannot_be_overwritten_by_path_reuse() {
        for persisted in [false, true] {
            for reverse in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let dir = root.path().join(".rockbox");
                std::fs::create_dir(&dir).unwrap();
                save(&dir, &one().encode().unwrap());
                write_audio(root.path(), "Music/new.mp3");
                std::fs::write(root.path().join("Music/new.mp3"), b"not audio").unwrap();
                let mut before = crate::device::DeviceManifest::default();
                before.rockbox.enabled = true;
                before.synced_items = vec![item()];
                let mut after = before.clone();
                after.synced_items[0].local_path = "Music/new.mp3".into();
                let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
                if persisted {
                    let mut first = Session::open(root.path(), before.clone()).unwrap();
                    update_session(
                        &mut first,
                        &after,
                        &io,
                        &crate::sync::SyncOperationManager::new(),
                        "test",
                    )
                    .await
                    .unwrap();
                    before = after.clone();
                }
                let files = load_files(&dir).unwrap().unwrap();
                let mut session = Session::open(root.path(), before).unwrap();
                write_audio(root.path(), "Music/song.mp3");
                let mut other = item();
                other.jellyfin_id = "other-source".into();
                after.synced_items.push(other);
                if reverse {
                    after.synced_items.reverse();
                }
                assert!(
                    update_session(
                        &mut session,
                        &after,
                        &io,
                        &crate::sync::SyncOperationManager::new(),
                        "test"
                    )
                    .await
                    .is_err(),
                    "persisted={persisted} reverse={reverse}"
                );
                assert_eq!(load_files(&dir).unwrap().unwrap(), files);
                let mut resumed = Session::open(root.path(), after.clone()).unwrap();
                assert!(
                    update_session(
                        &mut resumed,
                        &after,
                        &io,
                        &crate::sync::SyncOperationManager::new(),
                        "test"
                    )
                    .await
                    .is_err(),
                    "reopened persisted={persisted} reverse={reverse}"
                );
                assert_eq!(load_files(&dir).unwrap().unwrap(), files);
                std::fs::remove_file(root.path().join("Music/song.mp3")).unwrap();
                write_audio(root.path(), "Music/new.mp3");
                after
                    .synced_items
                    .retain(|item| item.jellyfin_id != "other-source");
                let mut repaired = Session::open(root.path(), after.clone()).unwrap();
                assert!(
                    update_session(
                        &mut repaired,
                        &after,
                        &io,
                        &crate::sync::SyncOperationManager::new(),
                        "test"
                    )
                    .await
                    .is_err(),
                    "ambiguous history must remain protected after the conflicting file is removed"
                );
                assert_eq!(load_files(&dir).unwrap().unwrap(), files);
            }
        }
    }

    #[tokio::test]
    async fn rockbox_warning_storage_failure_prevents_publication() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        let files = one().encode().unwrap();
        save(&dir, &files);
        write_audio(root.path(), "Music/song.mp3");
        std::fs::write(root.path().join("Music/song.mp3"), b"not audio").unwrap();
        let mut manifest = crate::device::DeviceManifest::default();
        manifest.rockbox.enabled = true;
        manifest.synced_items = vec![item()];
        let mut session = Session::open(root.path(), manifest.clone()).unwrap();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        let result = session
            .prepare_update(
                &manifest,
                &io,
                &crate::sync::SyncOperationManager::new(),
                "test",
                &mut |_| anyhow::bail!("warning storage unavailable"),
            )
            .await;
        assert!(
            matches!(result, Err(error) if error.to_string().contains("warning storage unavailable"))
        );
        assert_eq!(load_files(&dir).unwrap().unwrap(), files);
    }
    #[tokio::test]
    async fn rockbox_review_1_prefix_cycles_preserve_foreign_entries_and_remove_only_selected_volume()
     {
        for prefix in ["/foo", "/foo/bar", "/<microSD0>"] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox");
            std::fs::create_dir(&dir).unwrap();
            write_audio(root.path(), "Music/song.mp3");
            let own = format!("{prefix}/Music/song.mp3");
            let mut db = one();
            db.entries[0].strings[4] = own.clone().into_bytes();
            let mut foreign = db.entries[0].clone();
            foreign.strings[4] = "/<microSD1>/Music/song.mp3".into();
            foreign.numbers[16] = 9;
            db.entries.push(foreign);
            save(&dir, &db.encode().unwrap());
            let mut manifest = crate::device::DeviceManifest::default();
            manifest.device_id = "device".into();
            manifest.rockbox.enabled = true;
            manifest.rockbox.path_prefix = prefix.into();
            manifest.synced_items = vec![item()];
            let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
            let cancel = crate::sync::SyncOperationManager::new();
            for _ in 0..2 {
                let mut session = Session::open(root.path(), manifest.clone()).unwrap();
                update_session(&mut session, &manifest, &io, &cancel, "test")
                    .await
                    .unwrap();
                let current = load(&dir).unwrap().unwrap();
                assert_eq!(current.entries.len(), 2);
                assert_eq!(
                    current
                        .entries
                        .iter()
                        .find(|e| e.path() == own)
                        .unwrap()
                        .numbers[16],
                    7
                );
            }
            std::fs::remove_file(root.path().join("Music/song.mp3")).unwrap();
            manifest.synced_items.clear();
            let mut session = Session::open(root.path(), manifest.clone()).unwrap();
            update_session(&mut session, &manifest, &io, &cancel, "test")
                .await
                .unwrap();
            let remaining = load(&dir).unwrap().unwrap();
            assert_eq!(remaining.entries.len(), 1);
            assert_eq!(remaining.entries[0].path(), "/<microSD1>/Music/song.mp3");
            assert_eq!(remaining.entries[0].numbers[16], 9);
        }
    }
    #[tokio::test]
    async fn rockbox_removal_evidence_survives_checkpoint_and_cancelled_publication() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        let mut db = one();
        let mut unmanaged = db.entries[0].clone();
        unmanaged.strings[4] = "/Other/unmanaged.mp3".into();
        unmanaged.numbers[16] = 9;
        db.entries.push(unmanaged);
        save(&dir, &db.encode().unwrap());
        let mut before = crate::device::DeviceManifest::default();
        before.device_id = "device".into();
        before.rockbox.enabled = true;
        before.synced_items = vec![item()];
        drop(Session::open(root.path(), before.clone()).unwrap());
        before.synced_items.clear();
        let mut resumed = Session::open(root.path(), before.clone()).unwrap();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        update_session(
            &mut resumed,
            &before,
            &io,
            &crate::sync::SyncOperationManager::new(),
            "test",
        )
        .await
        .unwrap();
        let after = load(&dir).unwrap().unwrap();
        assert_eq!(after.entries.len(), 1);
        assert_eq!(after.entries[0].path(), "/Other/unmanaged.mp3");
        assert_eq!(after.entries[0].numbers[16], 9);
    }
    #[tokio::test]
    async fn rockbox_refuses_database_changed_after_sync_admission() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        save(&dir, &one().encode().unwrap());
        let mut manifest = crate::device::DeviceManifest::default();
        manifest.rockbox.enabled = true;
        let mut session = Session::open(root.path(), manifest.clone()).unwrap();
        let mut newer = one();
        newer.entries[0].numbers[16] = 10;
        let files = newer.encode().unwrap();
        save(&dir, &files);
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        assert!(
            update_session(
                &mut session,
                &manifest,
                &io,
                &crate::sync::SyncOperationManager::new(),
                "test"
            )
            .await
            .is_err()
        );
        assert_eq!(load(&dir).unwrap().unwrap().entries[0].numbers[16], 10);
        assert!(!dir.join(JOURNAL).exists());
    }
    #[test]
    fn rockbox_rating_observations_preserve_initial_zero_and_latest_pending_intent() {
        let db = crate::db::Database::memory().unwrap();
        observe_rating(&db, "device", "server", "account", "track", 0).unwrap();
        let pending = || {
            db.conn.lock().unwrap().query_row("SELECT count(*) FROM rockbox_ratings WHERE confirmed IS NULL OR confirmed!=observed",[],|r|r.get::<_,i64>(0)).unwrap()
        };
        assert_eq!(pending(), 0);
        observe_rating(&db, "device", "server", "account", "track", 7).unwrap();
        assert_eq!(pending(), 1);
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE rockbox_ratings SET confirmed=observed", [])
            .unwrap();
        observe_rating(&db, "device", "server", "account", "track", 7).unwrap();
        assert_eq!(pending(), 0);
        observe_rating(&db, "device", "server", "account", "track", 0).unwrap();
        assert_eq!(pending(), 1);
        observe_rating(&db, "device", "server", "account", "track", 9).unwrap();
        let raw = db
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT observed FROM rockbox_ratings", [], |r| {
                r.get::<_, i32>(0)
            })
            .unwrap();
        assert_eq!(raw, 9);
        for (raw, want) in [(0, 0), (1, 1), (2, 1), (3, 2), (9, 5), (10, 5)] {
            assert_eq!(navidrome_rating(raw).unwrap(), want);
        }
    }
    #[test]
    fn rockbox_review_2_clear_remains_pending_after_uncertain_positive_write() {
        let db = crate::db::Database::memory().unwrap();
        observe_rating(&db, "device", "server", "account", "track", 0).unwrap();
        observe_rating(&db, "device", "server", "account", "track", 7).unwrap();
        // The server accepted 4, but its response/readback was lost: no confirmation is recorded.
        observe_rating(&db, "device", "server", "account", "track", 0).unwrap();
        let pending=db.conn.lock().unwrap().query_row("SELECT count(*) FROM rockbox_ratings WHERE confirmed IS NULL OR confirmed!=observed",[],|r|r.get::<_,i64>(0)).unwrap();
        assert_eq!(pending, 1);
    }
    #[tokio::test]
    async fn rockbox_review_3_new_source_at_same_path_does_not_inherit_runtime_history() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        write_audio(root.path(), "Music/song.mp3");
        let mut db = one();
        db.entries[0].numbers[17] = 99;
        db.entries[0].numbers[21] = 1500;
        save(&dir, &db.encode().unwrap());
        let mut before = crate::device::DeviceManifest::default();
        before.device_id = "device".into();
        before.rockbox.enabled = true;
        before.synced_items = vec![item()];
        let mut session = Session::open(root.path(), before.clone()).unwrap();
        let mut after = before;
        after.synced_items[0].server_id = Some("other-server".into());
        after.synced_items[0].jellyfin_id = "new-track".into();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        update_session(
            &mut session,
            &after,
            &io,
            &crate::sync::SyncOperationManager::new(),
            "test",
        )
        .await
        .unwrap();
        let current = load(&dir).unwrap().unwrap();
        assert_eq!(&current.entries[0].numbers[15..19], &[0, 0, 0, 0]);
        assert_eq!(&current.entries[0].numbers[21..23], &[0, 0]);
    }
    #[tokio::test]
    async fn rockbox_review_4_interrupted_move_preserves_unique_indexed_history() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".rockbox");
        std::fs::create_dir(&dir).unwrap();
        write_audio(root.path(), "Music/new.mp3");
        let mut db = one();
        db.entries[0].numbers[21] = 1500;
        db.entries[0].numbers[22] = 4500;
        save(&dir, &db.encode().unwrap());
        let mut before = crate::device::DeviceManifest::default();
        before.device_id = "device".into();
        before.rockbox.enabled = true;
        before.synced_items = vec![item()];
        drop(Session::open(root.path(), before.clone()).unwrap());
        before.synced_items[0].local_path = "Music/new.mp3".into();
        let mut session = Session::open(root.path(), before.clone()).unwrap();
        let io = crate::device_io::MscBackend::new(root.path().to_path_buf());
        update_session(
            &mut session,
            &before,
            &io,
            &crate::sync::SyncOperationManager::new(),
            "test",
        )
        .await
        .unwrap();
        let current = load(&dir).unwrap().unwrap();
        assert_eq!(current.entries.len(), 1);
        assert_eq!(current.entries[0].path(), "/Music/new.mp3");
        assert_eq!(&current.entries[0].numbers[15..17], &[12, 7]);
        assert_eq!(&current.entries[0].numbers[21..23], &[1500, 4500]);
    }
    #[tokio::test]
    async fn rockbox_review_5_account_switch_cannot_establish_a_new_observation_binding() {
        use crate::providers::{CredentialKind, MediaProvider, ProviderCredentials};
        use mockito::{Matcher, Server};
        use std::sync::Arc;
        for established in [false, true] {
            let mut server = Server::new_async().await;
            let _ping=server.mock("GET","/rest/ping.view").match_query(Matcher::Any).with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome"}}"#).create_async().await;
            let _read=server.mock("GET","/rest/getSong.view").match_query(Matcher::Any).with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1","song":{"id":"track","userRating":4}}}"#).create_async().await;
            let provider = |user: &str| -> Arc<dyn MediaProvider> {
                Arc::new(
                    crate::providers::subsonic::SubsonicProvider::from_stored_config(
                        ProviderCredentials {
                            server_url: server.url(),
                            credential: CredentialKind::Password {
                                username: user.into(),
                                password: "secret".into(),
                            },
                        },
                        true,
                        None,
                    )
                    .unwrap(),
                )
            };
            let db = crate::db::Database::memory().unwrap();
            let local = db
                .upsert_server(
                    &server.url(),
                    "openSubsonic",
                    "alice",
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
            let portable = db
                .list_servers()
                .unwrap()
                .into_iter()
                .find(|s| s.id == local)
                .unwrap()
                .server_id
                .unwrap();
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(".rockbox");
            std::fs::create_dir(&dir).unwrap();
            save(&dir, &one().encode().unwrap());
            let mut before = crate::device::DeviceManifest::default();
            before.device_id = "device".into();
            before.rockbox.import_ratings = true;
            let mut track = item();
            track.server_id = Some(portable.clone());
            before.synced_items = vec![track];
            let session = Session::open(root.path(), before).unwrap();
            let cancel = crate::sync::SyncOperationManager::new();
            if established {
                assert!(
                    session
                        .import_ratings(
                            &db,
                            &HashMap::from([(portable.clone(), provider("alice"))]),
                            &cancel,
                            "test"
                        )
                        .await
                        .unwrap()
                        .is_empty()
                );
            }
            db.upsert_server(&server.url(), "openSubsonic", "bob", None, None, None, None)
                .unwrap();
            assert_eq!(
                db.list_servers().unwrap()[0].server_id.as_deref(),
                Some(portable.as_str())
            );
            let warnings = session
                .import_ratings(
                    &db,
                    &HashMap::from([(portable.clone(), provider("bob"))]),
                    &cancel,
                    "test",
                )
                .await
                .unwrap();
            let bob = crate::providers::feedback::account_scope("bob");
            let count = db
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM rockbox_ratings WHERE account=?1",
                    [bob],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap();
            assert_eq!(count, 0, "established={established}");
            assert!(!warnings.is_empty());
            db.upsert_server(
                &server.url(),
                "openSubsonic",
                "alice",
                None,
                None,
                None,
                None,
            )
            .unwrap();
            assert!(
                session
                    .import_ratings(
                        &db,
                        &HashMap::from([(portable, provider("alice"))]),
                        &cancel,
                        "test"
                    )
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    }
    #[test]
    fn rockbox_review_6_orphaned_backup_stage_is_recoverable_without_deleting_unrelated_files() {
        for unrelated in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path();
            let files = one().encode().unwrap();
            save(dir, &files);
            let backup = dir.join(BACKUP);
            std::fs::create_dir(&backup).unwrap();
            std::fs::write(
                backup.join(format!(".hifimule-rbdb-stage-{MASTER}")),
                b"partial backup write",
            )
            .unwrap();
            if unrelated {
                std::fs::write(backup.join("notes.txt"), b"user data").unwrap();
            }
            let changes = vec![Change {
                name: MASTER.into(),
                old: Some(hash(&files[MASTER])),
                new: Some("future".into()),
            }];
            atomic_file(
                &dir.join(JOURNAL),
                &serde_json::to_vec(&Journal {
                    prepared: false,
                    committed: false,
                    changes,
                })
                .unwrap(),
            )
            .unwrap();
            let result = recover(dir);
            if unrelated {
                assert!(result.is_err());
                assert_eq!(
                    std::fs::read(backup.join("notes.txt")).unwrap(),
                    b"user data"
                );
                std::fs::remove_file(backup.join("notes.txt")).unwrap();
                recover(dir).unwrap();
            } else {
                result.unwrap();
            }
            assert!(!backup.exists());
            assert!(!dir.join(JOURNAL).exists());
            publish(dir, &files, None).unwrap();
        }
    }
    #[test]
    fn rockbox_review_8_codec_budgets_cover_shared_string_expansion_and_encoded_output() {
        let mut db = one();
        db.entries[0].strings[0] = "a".repeat(4096).into_bytes();
        let base = db.entries[0].clone();
        db.entries = (0..8)
            .map(|i| {
                let mut e = base.clone();
                e.strings[4] = format!("/Music/{i}.mp3").into_bytes();
                e
            })
            .collect();
        let files = db.encode().unwrap();
        assert!(RbDatabase::decode(&files).is_ok());
        assert!(RbDatabase::decode_with_budget(&files, 20_000).is_err());
        assert!(RbDatabase::decode_with_budget(&files, 1).is_err());
        assert!(db.encode_with_limits(100, 100_000).is_err());
        assert!(db.encode_with_limits(1_000, 100_000).is_err());
        assert!(db.encode_with_limits(100_000, 100).is_err());
    }
    #[test]
    fn rockbox_settings_reject_unsafe_paths_formats_and_custom_locations() {
        for prefix in ["Music", "/../Music", "/a/./b", "/a\\b", "/C:/Music"] {
            let s = Settings {
                path_prefix: prefix.into(),
                ..Default::default()
            };
            assert!(s.validate().is_err());
        }
        let root = tempfile::tempdir().unwrap();
        assert!(mounted_dir(root.path()).is_err());
        std::fs::create_dir(root.path().join(".rockbox")).unwrap();
        std::fs::write(
            root.path().join(".rockbox/config.cfg"),
            b"database path: /other",
        )
        .unwrap();
        assert!(mounted_dir(root.path()).is_err());
    }
}
