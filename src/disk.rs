use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

pub fn at<T>(path: &Path, result: std::io::Result<T>) -> Result<T> {
    result.map_err(|source| Error::Io {
        path: path.into(),
        source,
    })
}

pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = at(path, fs::read(path))?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: path.into(),
        source,
    })
}

pub fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::Json {
        path: path.into(),
        source,
    })?;
    let temporary = path.with_extension("tmp");
    let mut file = at(&temporary, File::create(&temporary))?;
    at(&temporary, file.write_all(&bytes))?;
    at(&temporary, file.sync_all())?;
    drop(file);
    replace(&temporary, path)
}

pub fn write_new<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::Json {
        path: path.into(),
        source,
    })?;
    let mut file = at(path, File::create_new(path))?;
    at(path, file.write_all(&bytes))?;
    at(path, file.sync_all())
}

#[cfg(windows)]
fn replace(source: &Path, target: &Path) -> Result<()> {
    use windows::{Win32::Storage::FileSystem::*, core::PCWSTR};
    let from = crate::platform::wide(source);
    let to = crate::platform::wide(target);
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| Error::message_at(target, error))
}

#[cfg(not(windows))]
fn replace(source: &Path, target: &Path) -> Result<()> {
    at(target, fs::rename(source, target))
}

pub fn metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        result => at(path, result).map(Some),
    }
}

pub fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    meta.file_type().is_symlink()
}

pub fn no_links(path: &Path) -> Result<()> {
    for part in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if metadata(part)?.is_some_and(|m| is_link(&m)) {
            return Err(Error::message_at(part, "不支持符号链接或目录联接"));
        }
    }
    Ok(())
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(Error::message_at(path, "请选择绝对路径"));
    }
    let absolute = at(path, std::path::absolute(path))?;
    if absolute
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(Error::message_at(path, "路径不能包含 .."));
    }
    no_links(&absolute)?;
    if metadata(&absolute)?.is_some() {
        return at(&absolute, dunce::canonicalize(&absolute));
    }
    match (absolute.parent(), absolute.file_name()) {
        (Some(parent), Some(name)) => Ok(self::absolute(parent)?.join(name)),
        _ => Ok(absolute),
    }
}

pub fn eq_name(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Globalization::CompareStringOrdinal;
        let a: Vec<_> = a.encode_wide().collect();
        let b: Vec<_> = b.encode_wide().collect();
        unsafe { CompareStringOrdinal(&a, &b, true) }.0 == 2
    }
    #[cfg(not(windows))]
    {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
}

pub fn contains(parent: &Path, child: &Path) -> bool {
    let child_parts: Vec<_> = child.components().collect();
    let parent_parts: Vec<_> = parent.components().collect();
    parent_parts.len() <= child_parts.len()
        && parent_parts
            .iter()
            .zip(child_parts)
            .all(|(a, b)| eq_name(a.as_os_str(), b.as_os_str()))
}

pub fn overlaps(a: &Path, b: &Path) -> bool {
    contains(a, b) || contains(b, a)
}

pub fn copy(source: &Path, target: &Path) -> Result<()> {
    let meta = required(source)?;
    if meta.is_dir() {
        at(target, fs::create_dir(target))?;
        for entry in at(source, fs::read_dir(source))? {
            let entry = at(source, entry)?;
            copy(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if meta.is_file() {
        let mut input = at(source, File::open(source))?;
        let mut output = at(target, File::create_new(target))?;
        at(source, std::io::copy(&mut input, &mut output))?;
        at(target, output.sync_all())?;
    } else {
        return Err(Error::message_at(source, "仅支持普通文件和目录"));
    }
    Ok(())
}

pub fn required(path: &Path) -> Result<fs::Metadata> {
    match metadata(path)? {
        Some(meta) if !is_link(&meta) => Ok(meta),
        Some(_) => Err(Error::message_at(path, "不支持符号链接或目录联接")),
        None => Err(Error::message_at(path, "找不到路径")),
    }
}

pub fn validate_tree(path: &Path) -> Result<()> {
    let meta = required(path)?;
    if meta.is_dir() {
        for entry in at(path, fs::read_dir(path))? {
            validate_tree(&at(path, entry)?.path())?;
        }
    } else if !meta.is_file() {
        return Err(Error::message_at(path, "仅支持普通文件和目录"));
    }
    Ok(())
}

pub fn fingerprint(path: &Path) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    fingerprint_entry(path, &mut digest)?;
    Ok(digest.finalize().into())
}

fn fingerprint_entry(path: &Path, digest: &mut Sha256) -> Result<()> {
    let meta = required(path)?;
    if meta.is_dir() {
        let mut children = at(path, fs::read_dir(path))?
            .map(|entry| at(path, entry))
            .collect::<Result<Vec<_>>>()?;
        children.sort_by_key(|entry| entry.file_name());
        digest.update([0]);
        digest.update((children.len() as u64).to_le_bytes());
        for child in children {
            let name = child.file_name();
            let name = name.as_encoded_bytes();
            digest.update((name.len() as u64).to_le_bytes());
            digest.update(name);
            fingerprint_entry(&child.path(), digest)?;
        }
    } else if meta.is_file() {
        digest.update([1]);
        digest.update(meta.len().to_le_bytes());
        at(
            path,
            std::io::copy(&mut at(path, File::open(path))?, digest),
        )?;
    } else {
        return Err(Error::message_at(path, "仅支持普通文件和目录"));
    }
    Ok(())
}

pub fn remove(path: &Path) -> Result<()> {
    if let Some(meta) = metadata(path)? {
        if is_link(&meta) {
            return Err(Error::message_at(path, "临时路径被替换成链接"));
        }
        if meta.is_dir() {
            at(path, fs::remove_dir_all(path))?;
        } else {
            at(path, fs::remove_file(path))?;
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn rename_new(source: &Path, target: &Path) -> Result<()> {
    use windows::{Win32::Storage::FileSystem::*, core::PCWSTR};
    let from = crate::platform::wide(source);
    let to = crate::platform::wide(target);
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| Error::message_at(target, error))
}

#[cfg(not(windows))]
pub fn rename_new(source: &Path, target: &Path) -> Result<()> {
    if metadata(target)?.is_some() {
        return Err(Error::message_at(target, "目标已存在"));
    }
    at(target, fs::rename(source, target))
}
