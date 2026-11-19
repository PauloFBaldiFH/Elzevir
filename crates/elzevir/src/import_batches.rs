// SPDX-License-Identifier: GPL-3.0-or-later
//! Private, revisioned staging and the on-disk confirmation boundary.

use std::io::{self, BufRead, BufReader, ErrorKind, Read, Seek, Write};
use std::path::Path;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder, OpenOptions};
use elzevir_app::import::batches::{BatchState as State, BatchSummary, MAX_BATCH_MANIFEST_BYTES};
use elzevir_app::import::containers::{self, Inventory};
use elzevir_app::import::edits::{Recipe, RenderReport, render_page};
use elzevir_app::import::preview::MAX_IMPORT_FILE_BYTES;
use elzevir_app::import::sheet::TableSettings;
use elzevir_app::import::table::{Delimiter, Encoding};
use elzevir_app::import_detection::{
    Detection, Evidence, ImportFormat, MAX_DETECTION_BYTES, detect_format, select_prepared,
    table_delimiter,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_STAGED_BATCHES: usize = 10;
const MAX_STAGED_BATCH_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_STAGED_TOTAL_BYTES: u64 = 4 * MAX_STAGED_BATCH_BYTES;
const MANIFEST_RESERVATION: u64 = 2 * MAX_BATCH_MANIFEST_BYTES;
const VERSION: u32 = 1;
const ROOT: &str = "imports";
const LOCK: &str = ".lock";
const SOURCE: &str = "original";
const MANIFEST: &str = "manifest.json";
const NEXT: &str = "manifest.next";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Batch {
    version: u32,
    institution: String,
    operator: u64,
    id: String,
    format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected: Option<ImportFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    table_separator: Option<Delimiter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) archive: Option<Inventory>,
    sha256: String,
    pub(crate) revision: u64,
    pub(crate) state: State,
    pub(crate) recipe: Recipe,
    pub(crate) preview_sha256: Option<String>,
    pub(crate) receipt: Option<serde_json::Value>,
}

pub(crate) struct Batches {
    root: Dir,
    _lock: std::fs::File,
    institution: String,
    operator: u64,
}

impl Batches {
    pub(crate) fn open(data: &Path, operator: u64, create: bool) -> Result<Self, String> {
        if operator == 0 || !std::fs::symlink_metadata(data).is_ok_and(|meta| meta.is_dir()) {
            return Err(
                "an existing institution directory and positive operator are required".into(),
            );
        }
        let absolute = data.canonicalize().map_err(io_error)?;
        let institution = format!(
            "{:x}",
            Sha256::digest(absolute.as_os_str().as_encoded_bytes())
        );
        let parent = absolute
            .parent()
            .ok_or("institution directory needs a parent")?;
        let name = absolute
            .file_name()
            .ok_or("institution directory needs a name")?;
        let parent =
            Dir::open_ambient_dir(parent, cap_std::ambient_authority()).map_err(io_error)?;
        let data = parent.open_dir_nofollow(name).map_err(io_error)?;
        if create {
            match create_dir(&data, ROOT) {
                Ok(()) => sync_dir(&data)?,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_error(error)),
            }
        }
        let root = open_private_dir(&data, ROOT)?;
        let mut options = private_options();
        options.read(true).write(true).create(true);
        let lock = root.open_with(LOCK, &options).map_err(io_error)?.into_std();
        if !lock.metadata().map_err(io_error)?.is_file() {
            return Err("import lock must be a regular file".into());
        }
        lock.try_lock()
            .map_err(|_| "import batches are busy or cannot be locked")?;
        Ok(Self {
            root,
            _lock: lock,
            institution,
            operator,
        })
    }

    pub(crate) fn stage(
        &self,
        source: impl Read,
        selected: Option<ImportFormat>,
    ) -> Result<Batch, String> {
        let (count, bytes) = self.usage()?;
        if count >= MAX_STAGED_BATCHES {
            return Err("import batch count limit exceeded".into());
        }
        if bytes > MAX_STAGED_TOTAL_BYTES - MANIFEST_RESERVATION {
            return Err("no import quota available for a new batch".into());
        }
        let id = crate::backup_archive::identifier()?;
        create_dir(&self.root, &id).map_err(io_error)?;
        match sync_dir(&self.root).and_then(|()| self.capture(&id, source, selected)) {
            Ok(batch) => Ok(batch),
            Err(error) => {
                self.root
                    .remove_dir_all(&id)
                    .map_err(|_| format!("{error}. Private batch {id} requires cleanup"))?;
                Err(error)
            }
        }
    }

    fn capture(
        &self,
        id: &str,
        source: impl Read,
        selected: Option<ImportFormat>,
    ) -> Result<Batch, String> {
        let mut batch = Batch {
            version: VERSION,
            institution: self.institution.clone(),
            operator: self.operator,
            id: id.into(),
            format: String::new(),
            selected: None,
            table_separator: None,
            archive: None,
            sha256: String::new(),
            revision: 1,
            state: State::Preparing,
            recipe: Recipe::default(),
            preview_sha256: None,
            receipt: None,
        };
        self.save(&batch)?;
        let dir = self.batch_dir(id)?;
        let available = self.available(&dir)?.min(MAX_IMPORT_FILE_BYTES as u64);
        let mut output = create_file(&dir, SOURCE)?;
        let mut source = HashInput::new(source);
        io::copy(&mut (&mut source).take(available), &mut output).map_err(io_error)?;
        if source.read(&mut [0]).map_err(io_error)? != 0 {
            return Err("source exceeds the available import quota".into());
        }
        output.sync_all().map_err(io_error)?;
        drop(output);
        self.choose_format(&mut batch, selected)?;
        batch.sha256 = source.digest();
        batch.state = State::Review;
        self.save(&batch)?;
        Ok(batch)
    }

    pub(crate) fn choose_format(
        &self,
        batch: &mut Batch,
        selected: Option<ImportFormat>,
    ) -> Result<(), String> {
        let input = read_file(&self.batch_dir(&batch.id)?, SOURCE)?;
        let mut prefix = Vec::new();
        input
            .take(MAX_DETECTION_BYTES as u64)
            .read_to_end(&mut prefix)
            .map_err(io_error)?;
        let detected = detect_format(&prefix);
        if matches!(
            detected.evidence,
            Evidence::ZipSignature | Evidence::GzipSignature
        ) {
            if selected.is_some_and(|format| format != ImportFormat::Archive) {
                return Err(
                    "select the member from the container inventory before choosing its reader"
                        .into(),
                );
            }
            let input = read_file(&self.batch_dir(&batch.id)?, SOURCE)?;
            batch.archive = Some(containers::inspect(&read_archive(input)?).map_err(io_error)?);
            batch.format = "Archive".into();
            batch.selected = selected;
            batch.table_separator = None;
            return Ok(());
        }
        if detected.format == ImportFormat::KohaFullBackup {
            return Err(elzevir_app::import::koha_sql::DumpProblem::FullBackup.to_string());
        }
        let table = is_delimited(detected, selected);
        let format = if table {
            ImportFormat::Delimited
        } else {
            select_prepared(detected, selected)
                .ok_or_else(|| {
                    format!("unsupported or inconclusive input: {:?}", detected.evidence)
                })?
                .format
        };
        batch.format = format!("{format:?}");
        batch.archive = None;
        batch.selected = selected;
        batch.table_separator = table
            .then(|| {
                table_delimiter(&prefix).or_else(|| {
                    (detected.evidence == Evidence::EmptyInput).then_some(Delimiter::Comma)
                })
            })
            .flatten();
        Ok(())
    }

    pub(crate) fn load(&self, id: &str) -> Result<Batch, String> {
        let dir = self.batch_dir(id)?;
        let batch: Batch = load_json(read_file(&dir, MANIFEST)?)?;
        if batch.version != VERSION
            || batch.id != id
            || batch.operator != self.operator
            || batch.institution != self.institution
            || batch.revision == 0
        {
            return Err("incompatible batch or operator".into());
        }
        Ok(batch)
    }

    pub(crate) fn list(&self) -> Result<Vec<BatchSummary>, String> {
        self.usage()?;
        let mut batches = Vec::new();
        for entry in self.root.entries().map_err(io_error)? {
            let name = entry.map_err(io_error)?.file_name();
            // An incomplete manifest or another operator's batch is not visible.
            if let Some(id) = name.to_str()
                && let Ok(batch) = self.load(id)
            {
                batches.push(batch.summary());
            }
        }
        batches.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        Ok(batches)
    }

    pub(crate) fn save(&self, batch: &Batch) -> Result<(), String> {
        self.save_reserved(batch, 0)
    }

    pub(crate) fn save_reserved(&self, batch: &Batch, reserved: u64) -> Result<(), String> {
        let dir = self.batch_dir(&batch.id)?;
        remove_if_present(&dir, NEXT)?;
        let bytes = serde_json::to_vec(batch).map_err(|_| "cannot encode batch manifest")?;
        if bytes.len() as u64 > MAX_BATCH_MANIFEST_BYTES || reserved > self.available(&dir)? {
            return Err("batch manifest exceeds the available import quota".into());
        }
        let mut file = create_file(&dir, NEXT)?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(io_error)?;
        drop(file);
        dir.rename(NEXT, &dir, MANIFEST).map_err(io_error)?;
        sync_dir(&dir)
    }

    pub(crate) fn render(
        &self,
        batch: &Batch,
        from: Option<u64>,
    ) -> Result<(std::fs::File, RenderReport, String), String> {
        let dir = self.batch_dir(&batch.id)?;
        let source = read_file(&dir, SOURCE)?;
        if source.metadata().map_err(io_error)?.len() > MAX_IMPORT_FILE_BYTES as u64 {
            return Err("original exceeds the import file limit".into());
        }
        let source = HashInput::new(source);
        let mut input = BufReader::new(source);
        let result = self.render_member(batch, from, &mut input, batch.selected)?;
        if input.into_inner().digest() != batch.sha256
            || format!("{:?}", result.1.detection.format) != batch.format
        {
            return Err("original identity changed".into());
        }
        Ok(result)
    }

    pub(crate) fn render_member(
        &self,
        batch: &Batch,
        from: Option<u64>,
        input: impl BufRead,
        selected: Option<ImportFormat>,
    ) -> Result<(std::fs::File, RenderReport, String), String> {
        let dir = self.batch_dir(&batch.id)?;
        let mut output = HashInput::new(tempfile::tempfile().map_err(io_error)?);
        let recipe = serde_json::to_vec(&(&batch.sha256, batch.revision, &batch.recipe))
            .map_err(|_| "cannot encode preview identity")?;
        output.hash.update(recipe);
        let limit = usize::try_from(self.available(&dir)?)
            .unwrap_or(usize::MAX)
            .min(MAX_IMPORT_FILE_BYTES);
        let report = render_page(input, &mut output, &batch.recipe, limit, from, selected)
            .map_err(|error| error.to_string())?;
        let stamp = format!("{:x}", output.hash.finalize());
        output.input.rewind().map_err(io_error)?;
        Ok((output.input, report, stamp))
    }

    pub(crate) fn archive_source(&self, batch: &Batch) -> Result<Vec<u8>, String> {
        self.with_source(batch, |input| read_archive(input))
    }

    pub(crate) fn with_source<T>(
        &self,
        batch: &Batch,
        read: impl FnOnce(&mut dyn BufRead) -> Result<T, String>,
    ) -> Result<T, String> {
        let source = read_file(&self.batch_dir(&batch.id)?, SOURCE)?;
        if source.metadata().map_err(io_error)?.len() > MAX_IMPORT_FILE_BYTES as u64 {
            return Err("original exceeds the import file limit".into());
        }
        let mut input = BufReader::new(HashInput::new(source));
        let value = read(&mut input)?;
        if input.into_inner().digest() != batch.sha256 {
            return Err("original identity changed".into());
        }
        Ok(value)
    }

    pub(crate) fn table_stamp(
        batch: &Batch,
        prepared: &elzevir_app::import::sheet::PreparedTable,
    ) -> Result<String, String> {
        let mut output = HashInput::new(io::sink());
        serde_json::to_writer(
            &mut output,
            &("table", &batch.sha256, batch.revision, &batch.recipe),
        )
        .map_err(|error| error.to_string())?;
        for row in prepared.records() {
            serde_json::to_writer(&mut output, &(row.row, &row.record))
                .map_err(|error| error.to_string())?;
        }
        Ok(output.digest())
    }

    pub(crate) fn clean(&self, batch: &Batch) -> Result<(), String> {
        let dir = self.batch_dir(&batch.id)?;
        remove_if_present(&dir, SOURCE)?;
        remove_if_present(&dir, NEXT)?;
        if dir.entries().map_err(io_error)?.count() != 1 {
            return Err("unexpected batch files prevented cleanup".into());
        }
        dir.remove_file(MANIFEST).map_err(io_error)?;
        drop(dir);
        if let Err(error) = self.root.remove_dir(&batch.id) {
            self.save(batch)?;
            return Err(io_error(error));
        }
        sync_dir(&self.root)
    }

    fn batch_dir(&self, id: &str) -> Result<Dir, String> {
        if !valid_id(id) {
            return Err("invalid batch identity".into());
        }
        open_private_dir(&self.root, id)
    }

    fn usage(&self) -> Result<(usize, u64), String> {
        let (mut count, mut bytes) = (0, 0_u64);
        for entry in self.root.entries().map_err(io_error)? {
            let name = entry.map_err(io_error)?.file_name();
            if name == LOCK {
                continue;
            }
            let id = name.to_str().ok_or("invalid import directory entry")?;
            count += 1;
            if count > MAX_STAGED_BATCHES {
                return Err("import batch count limit exceeded".into());
            }
            bytes = bytes.saturating_add(size(&self.batch_dir(id)?)?);
        }
        Ok((count, bytes))
    }

    fn available(&self, dir: &Dir) -> Result<u64, String> {
        Ok(MAX_STAGED_BATCH_BYTES
            .saturating_sub(size(dir)?)
            .min(MAX_STAGED_TOTAL_BYTES.saturating_sub(self.usage()?.1)))
    }
}

impl Batch {
    pub(crate) fn is_table(&self) -> bool {
        self.format == "Delimited"
    }

    pub(crate) fn table_settings(&self) -> Result<TableSettings, String> {
        self.settings_for(self.table_separator)
    }

    pub(crate) fn settings_for(
        &self,
        delimiter: Option<Delimiter>,
    ) -> Result<TableSettings, String> {
        if let Some(settings) = &self.recipe.table {
            return Ok(settings.clone());
        }
        let delimiter = delimiter.ok_or("choose a spreadsheet separator in the recipe")?;
        Ok(TableSettings {
            delimiter,
            encoding: Encoding::Utf8,
            profile: None,
            group_by: None,
        })
    }

    pub(crate) fn summary(&self) -> BatchSummary {
        BatchSummary {
            id: self.id.clone(),
            revision: self.revision,
            state: self.state,
            format: self.format.clone(),
            selected: self.selected,
        }
    }

    pub(crate) fn check_review(&self) -> Result<(), String> {
        if matches!(self.state, State::Review | State::Ready) {
            return Ok(());
        }
        Err(format!(
            "batch is {:?} and cannot be edited or previewed",
            self.state
        ))
    }
}

pub(crate) fn is_delimited(detected: Detection, selected: Option<ImportFormat>) -> bool {
    selected.is_none() && detected.format == ImportFormat::Delimited
        || selected == Some(ImportFormat::Delimited)
            && matches!(
                detected.evidence,
                Evidence::DelimitedRows
                    | Evidence::AmbiguousDelimiter
                    | Evidence::Inconclusive
                    | Evidence::EmptyInput
            )
}

pub(crate) fn read_recipe(path: &Path) -> Result<Recipe, String> {
    let bytes = read_manifest_bytes(open_path(path)?)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "import recipe must be UTF-8")?;
    Recipe::parse(text).map_err(|error| error.to_string())
}

fn read_manifest_bytes(file: std::fs::File) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    file.take(MAX_BATCH_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_BATCH_MANIFEST_BYTES {
        return Err("batch JSON or recipe exceeds its byte limit".into());
    }
    Ok(bytes)
}

fn load_json<T: serde::de::DeserializeOwned>(file: std::fs::File) -> Result<T, String> {
    let bytes = read_manifest_bytes(file)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "invalid batch JSON at line {}, column {}",
            error.line(),
            error.column()
        )
    })
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn io_error(error: impl std::fmt::Display) -> String {
    format!("cannot access private import files: {error}")
}

fn read_archive(input: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    input
        .take(MAX_IMPORT_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_IMPORT_FILE_BYTES {
        return Err("archive exceeds the import file limit".into());
    }
    Ok(bytes)
}

fn size(dir: &Dir) -> Result<u64, String> {
    // Reserve both manifest slots even when they are empty so cancellation can still write.
    let mut length = MANIFEST_RESERVATION;
    for entry in dir.entries().map_err(io_error)? {
        let name = entry.map_err(io_error)?.file_name();
        if name != SOURCE && name != MANIFEST && name != NEXT {
            return Err("unrecognized file in an import batch".into());
        }
        let metadata = dir.symlink_metadata(&name).map_err(io_error)?;
        if !metadata.is_file() {
            return Err("import batch entries must be regular files".into());
        }
        if name == SOURCE {
            length = length.saturating_add(metadata.len());
        } else if metadata.len() > MAX_BATCH_MANIFEST_BYTES {
            return Err("batch manifest exceeds its byte limit".into());
        }
    }
    Ok(length)
}

fn create_dir(parent: &Dir, name: &str) -> io::Result<()> {
    #[cfg(unix)]
    let mut options = DirBuilder::new();
    #[cfg(not(unix))]
    let options = DirBuilder::new();
    #[cfg(unix)]
    {
        use cap_std::fs::DirBuilderExt as _;
        options.mode(0o700);
    }
    parent.create_dir_with(name, &options)
}

fn open_private_dir(parent: &Dir, name: &str) -> Result<Dir, String> {
    let dir = parent.open_dir_nofollow(name).map_err(io_error)?;
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt as _;
        if dir.dir_metadata().map_err(io_error)?.permissions().mode() & 0o077 != 0 {
            return Err("batch directory must be private".into());
        }
    }
    Ok(dir)
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
}

fn create_file(dir: &Dir, name: &str) -> Result<std::fs::File, String> {
    let mut options = private_options();
    options.write(true).create_new(true);
    dir.open_with(name, &options)
        .map(cap_std::fs::File::into_std)
        .map_err(io_error)
}

fn read_file(dir: &Dir, name: impl AsRef<Path>) -> Result<std::fs::File, String> {
    if !dir.symlink_metadata(&name).map_err(io_error)?.is_file() {
        return Err("import input must be a regular file".into());
    }
    let mut options = private_options();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt as _;
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed());
    }
    let file = dir.open_with(name, &options).map_err(io_error)?.into_std();
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err("import input must be a regular file".into());
    }
    Ok(file)
}

pub(crate) fn open_path(path: &Path) -> Result<std::fs::File, String> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path.file_name().ok_or("input needs a filename")?;
    let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority()).map_err(io_error)?;
    read_file(&parent, name)
}

fn remove_if_present(dir: &Dir, name: &str) -> Result<(), String> {
    match dir.symlink_metadata(name) {
        Ok(meta) if meta.is_file() => dir.remove_file(name).map_err(io_error),
        Ok(_) => Err("batch cleanup requires regular files".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

#[cfg(unix)]
fn sync_dir(dir: &Dir) -> Result<(), String> {
    dir.open(".")
        .and_then(|file| file.sync_all())
        .map_err(io_error)
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Dir) -> Result<(), String> {
    Ok(())
}

struct HashInput<R> {
    input: R,
    hash: Sha256,
}

impl<R> HashInput<R> {
    fn new(input: R) -> Self {
        Self {
            input,
            hash: Sha256::new(),
        }
    }
    fn digest(self) -> String {
        format!("{:x}", self.hash.finalize())
    }
}

impl<R: Read> Read for HashInput<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(output)?;
        self.hash.update(
            output
                .get(..count)
                .ok_or_else(|| io::Error::other("invalid read count"))?,
        );
        Ok(count)
    }
}

impl<W: Write> Write for HashInput<W> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let count = self.input.write(input)?;
        self.hash.update(
            input
                .get(..count)
                .ok_or_else(|| io::Error::other("invalid write count"))?,
        );
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.input.flush()
    }
}
