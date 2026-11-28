// SPDX-License-Identifier: GPL-3.0-or-later
//! Release the institution mutation lock while scanning private, immutable ranges.
use super::{
    Batch, Batches, HashInput, MANIFEST, RELATION_LOCK, SOURCE, State, io_error, load_json,
    lock_file, read_file, source_set,
};
use cap_std::fs::Dir;
use elzevir_app::import::batches::RelationPreview;
use elzevir_app::import::relations::{self, Selection};
use sha2::Digest;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Take};

struct ActiveSource<'a> {
    directory: &'a Dir,
    file: File,
    until_check: usize,
}

impl<'a> ActiveSource<'a> {
    fn open(directory: &'a Dir, offset: u64) -> Result<Self, String> {
        let mut file = read_file(directory, SOURCE)?;
        file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        Ok(Self {
            directory,
            file,
            until_check: 0,
        })
    }
}

impl Read for ActiveSource<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.until_check == 0 {
            if !self.directory.try_exists(SOURCE)? {
                return Err(io::Error::other("source set cancelled"));
            }
            self.until_check = 1024 * 1024;
        }
        let n = self.file.read(out)?;
        self.until_check = self.until_check.saturating_sub(n);
        Ok(n)
    }
}

struct MemberBytes<'a> {
    source: HashInput<Take<ActiveSource<'a>>>,
    expected: &'a str,
}

impl Read for MemberBytes<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let count = self.source.read(out)?;
        if count == 0
            && !out.is_empty()
            && (self.source.input.limit() != 0
                || format!("{:x}", self.source.hash.clone().finalize()) != self.expected)
        {
            return Err(io::Error::other("source member changed during analysis"));
        }
        Ok(count)
    }
}

impl Batches {
    pub(crate) fn relations(
        self,
        mut batch: Batch,
        selection: Selection,
        from: u64,
    ) -> Result<Box<RelationPreview>, String> {
        if batch.state != State::Review {
            return Err("reviewable source set required".into());
        }
        let sources = batch.sources.take().ok_or("expected a source set")?;
        if source_set::stamp(&sources)? != batch.sha256 {
            return Err("source set manifest changed".into());
        }
        let directory = self.batch_dir(&batch.id)?;
        let _analysis_lock = lock_file(&self.data, RELATION_LOCK)?;
        drop(self); // An operator can cancel or manage other batches during a large scan.
        source_set::verify(&sources, ActiveSource::open(&directory, 0)?)?;
        let open = |position: usize| {
            let member = position
                .checked_sub(1)
                .and_then(|i| sources.members.get(i))
                .filter(|m| m.position == position)
                .ok_or("unknown source member")?;
            let reader = ActiveSource::open(&directory, member.offset)?;
            Ok(BufReader::new(MemberBytes {
                source: HashInput::new(reader.take(member.bytes)),
                expected: &member.sha256,
            }))
        };
        let digests: Vec<_> = sources
            .members
            .iter()
            .map(|member| member.sha256.as_str())
            .collect();
        let report = relations::analyze(&digests, selection, from, open)?;
        let current: Batch = load_json(read_file(&directory, MANIFEST)?)?;
        if current.id != batch.id
            || current.operator != batch.operator
            || current.institution != batch.institution
            || current.revision != batch.revision
            || current.state != State::Review
            || current.sha256 != batch.sha256
            || current
                .sources
                .as_ref()
                .map(source_set::stamp)
                .transpose()?
                .as_ref()
                != Some(&batch.sha256)
        {
            return Err("source set changed during relation review".into());
        }
        Ok(Box::new(RelationPreview {
            batch: batch.id,
            revision: batch.revision,
            manifest_sha256: batch.sha256,
            sources,
            report,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use elzevir_app::import::batches::{BatchCommand, SourceInput};
    #[test]
    fn cancelled_set_interrupts_an_open_reader_while_analysis_lock_is_held() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(io_error)?;
        let batches = Batches::open(dir.path(), 1, true)?;
        let input = SourceInput {
            name: "rows".into(),
            input: Box::new(io::repeat(1).take(2 * 1024 * 1024)),
        };
        let batch = batches.stage_set("test".into(), Box::new(std::iter::once(Ok(input))))?;
        let directory = batches.batch_dir(&batch.id)?;
        let _analysis_lock = lock_file(&batches.data, RELATION_LOCK)?;
        assert!(lock_file(&batches.data, RELATION_LOCK).is_err());
        drop(batches);
        let mut reader = ActiveSource::open(&directory, 0)?;
        io::copy(&mut (&mut reader).take(1024 * 1024), &mut io::sink()).map_err(io_error)?;
        crate::import_service::execute(
            dir.path(),
            1,
            None,
            true,
            BatchCommand::Cancel(batch.id, Some(1), false),
        )
        .map_err(|e| e.to_string())?;
        assert!(reader.read(&mut [0]).is_err());
        Ok(())
    }
}
