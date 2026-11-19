// SPDX-License-Identifier: GPL-3.0-or-later
//! Preflight the central directory before opening any ZIP members.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use ::zip::{CompressionMethod, ZipArchive};

use super::{
    Budget, Entry, Error, Kind, MAX_ARCHIVE_MEMBERS, Selected, Walker, book, checked_name, ratio,
};

type Archive<'a> = ZipArchive<Cursor<&'a [u8]>>;

struct Member {
    name: String,
    expanded: u64,
    directory: bool,
}

impl Walker<'_> {
    pub(super) fn zip(
        &mut self,
        bytes: &[u8],
        path: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), Error> {
        let (count, start) = directory(bytes)?;
        self.budget.members(count)?;
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Archive)?;
        if archive.len() != count || archive.central_directory_start() != start as u64 {
            return Err(Error::Name);
        }
        let members = members(&mut archive, &mut self.budget)?;
        if archive
            .has_overlapping_files()
            .map_err(|_| Error::Archive)?
        {
            return Err(Error::Archive);
        }
        if let Some(kind) = workbook_kind(&members)? {
            let mut bounds = book::Bounds::default();
            for (index, member) in members
                .iter()
                .enumerate()
                .filter(|(_, member)| !member.directory)
            {
                let data = member_bytes(&mut archive, index, member.expanded)?;
                bounds.inspect(kind, &member.name, &data)?;
            }
            let choice = self.choice.filter(|choice| choice.member == *path);
            let (sheets, rows) = book::read(kind, bytes, choice)?;
            self.entries.push(Entry {
                member: path.clone(),
                kind,
                sheets,
            });
            if let Some(rows) = rows {
                self.selected = Some(Selected::Workbook(rows));
            }
            return Ok(());
        }
        for (index, member) in members.into_iter().enumerate() {
            if !member.directory {
                let data = member_bytes(&mut archive, index, member.expanded)?;
                path.push(member.name);
                self.visit(&data, path, depth + 1)?;
                path.pop();
            }
        }
        Ok(())
    }
}

fn workbook_kind(members: &[Member]) -> Result<Option<Kind>, Error> {
    let has = |name| {
        members
            .iter()
            .any(|member| member.name == name && !member.directory)
    };
    match (
        has("xl/workbook.xml"),
        has("mimetype") && has("content.xml"),
    ) {
        (true, false) if has("[Content_Types].xml") => Ok(Some(Kind::Xlsx)),
        (false, true) if has("META-INF/manifest.xml") => Ok(Some(Kind::Ods)),
        (false, false) => Ok(None),
        _ => Err(Error::Workbook),
    }
}

fn members(archive: &mut Archive<'_>, budget: &mut Budget) -> Result<Vec<Member>, Error> {
    let mut names = BTreeMap::new();
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive.by_index_raw(index).map_err(|_| Error::Archive)?;
        let dir = file.is_dir();
        let mode = file.unix_mode().unwrap_or_default() & 0o170_000;
        if file.is_symlink()
            || file.encrypted()
            || !matches!(
                file.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            || !(mode == 0 || dir && mode == 0o040_000 || !dir && mode == 0o100_000)
            || dir && file.size() != 0
        {
            return Err(Error::Member);
        }
        let name = checked_name(file.name_raw(), dir)?;
        if names.insert(name.to_ascii_lowercase(), dir).is_some() {
            return Err(Error::Name);
        }
        ratio(file.size(), file.compressed_size())?;
        budget.expand(file.size())?;
        entries.push(Member {
            name,
            expanded: file.size(),
            directory: dir,
        });
    }
    for name in names.keys() {
        for (position, _) in name.match_indices('/') {
            let parent = name.get(..position).ok_or(Error::Name)?;
            if names.get(parent) == Some(&false) {
                return Err(Error::Name);
            }
        }
    }
    Ok(entries)
}

fn member_bytes(archive: &mut Archive<'_>, index: usize, expected: u64) -> Result<Vec<u8>, Error> {
    let mut file = archive.by_index(index).map_err(|_| Error::Archive)?;
    let mut data = Vec::new();
    let size = usize::try_from(expected).map_err(|_| Error::Limit("expanded bytes"))?;
    data.try_reserve_exact(size)
        .map_err(|_| Error::Limit("expanded bytes"))?;
    data.resize(size, 0);
    file.read_exact(&mut data).map_err(|_| Error::Archive)?;
    if file.read(&mut [0]).map_err(|_| Error::Archive)? != 0 {
        return Err(Error::Archive);
    }
    Ok(data)
}

fn directory(bytes: &[u8]) -> Result<(usize, usize), Error> {
    const END: usize = 22;
    const HEADER: usize = 46;
    let tail_start = bytes.len().saturating_sub(END + usize::from(u16::MAX));
    let tail = bytes.get(tail_start..).ok_or(Error::Archive)?;
    let end = tail
        .windows(END)
        .enumerate()
        .rev()
        .find_map(|(position, end)| {
            (end.starts_with(b"PK\x05\x06")
                && u16_at(end, 20)
                    .is_ok_and(|length| usize::from(length) + position + END == tail.len()))
            .then_some((tail_start + position, end))
        })
        .ok_or(Error::Archive)?;
    let (position, end) = end;
    let comment = bytes.get(position + END..).ok_or(Error::Archive)?;
    if end.get(4..8) != Some(&[0; 4])
        || u16_at(end, 8)? != u16_at(end, 10)?
        || comment
            .windows(4)
            .any(|signature| signature == b"PK\x05\x06")
        || position
            .checked_sub(20)
            .is_some_and(|i| bytes.get(i..i + 4) == Some(b"PK\x06\x07"))
    {
        return Err(Error::Archive);
    }
    let count = usize::from(u16_at(end, 10)?);
    if count > MAX_ARCHIVE_MEMBERS {
        return Err(Error::Limit("member count"));
    }
    let start = u32_at(end, 16)? as usize;
    if start.checked_add(u32_at(end, 12)? as usize) != Some(position) {
        return Err(Error::Archive);
    }
    let mut directory = bytes.get(start..position).ok_or(Error::Archive)?;
    for _ in 0..count {
        if !directory.starts_with(b"PK\x01\x02") {
            return Err(Error::Archive);
        }
        let filename = usize::from(u16_at(directory, 28)?);
        let extra = usize::from(u16_at(directory, 30)?);
        let comment = usize::from(u16_at(directory, 32)?);
        directory = directory
            .get(HEADER + filename + extra + comment..)
            .ok_or(Error::Archive)?;
    }
    if !directory.is_empty() {
        return Err(Error::Archive);
    }
    Ok((count, start))
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, Error> {
    bytes
        .get(offset..offset + 2)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(Error::Archive)
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    bytes
        .get(offset..offset + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(Error::Archive)
}
