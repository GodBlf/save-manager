use crate::{Error, Manager, Result, disk};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const MANIFEST: &str = ".save-manager.json";

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: Uuid,
    pub number: u64,
    pub name: Option<String>,
    pub created: i64,
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Manifest {
    version: u32,
    id: Uuid,
    number: u64,
    created: i64,
    pub(crate) paths: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Creation {
    root: PathBuf,
    id: Uuid,
    number: u64,
    directory: String,
}

impl Creation {
    fn staging(&self) -> PathBuf {
        self.root.join(format!(".save-manager-create-{}", self.id))
    }

    fn target(&self) -> PathBuf {
        self.root.join(&self.directory)
    }
}

fn name(value: &str) -> Result<Option<String>> {
    let value = value.trim();
    if value.ends_with('.')
        || value
            .chars()
            .any(|c| c <= '\u{1f}' || "<>:\"/\\|?*".contains(c))
    {
        return Err(Error::Message("名称包含 Windows 不允许的字符或结尾".into()));
    }
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

fn directory(number: u64, value: &str) -> Result<String> {
    let name = name(value)?;
    let directory = match name {
        Some(name) => format!("{number}-{name}"),
        None => number.to_string(),
    };
    if directory.encode_utf16().count() > 255 {
        return Err(Error::Message("快照目录名太长".into()));
    }
    Ok(directory)
}

fn parse_directory(value: &OsStr) -> Option<(u64, Option<String>)> {
    let value = value.to_str()?;
    let (number, suffix) = match value.split_once('-') {
        Some((n, suffix)) => (n, Some(suffix)),
        None => (value, None),
    };
    if !number.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((number.parse().ok()?, suffix.map(str::to_owned)))
}

pub(crate) fn occupied(root: &Path) -> Result<Vec<u64>> {
    disk::no_links(root)?;
    let mut numbers = Vec::new();
    for entry in disk::at(root, fs::read_dir(root))? {
        let entry = disk::at(root, entry)?;
        if let Some((number, _)) = parse_directory(&entry.file_name()) {
            numbers.push(number);
        }
    }
    Ok(numbers)
}

pub(crate) fn increment(number: u64) -> Result<u64> {
    number
        .checked_add(1)
        .ok_or_else(|| Error::Message("快照编号已用尽".into()))
}

fn leaf(path: &Path) -> Result<&OsStr> {
    path.file_name()
        .ok_or_else(|| Error::message_at(path, "请选择文件或子目录"))
}

pub(crate) fn validate_paths(root: &Path, state: &Path, paths: &[PathBuf]) -> Result<()> {
    for (index, path) in paths.iter().enumerate() {
        let filename = leaf(path)?;
        if disk::eq_name(filename, OsStr::new(MANIFEST)) {
            return Err(Error::message_at(path, "名称与快照标识文件冲突"));
        }
        if disk::overlaps(root, path) || disk::overlaps(state, path) {
            return Err(Error::message_at(
                path,
                "路径与快照根目录或应用数据目录重叠",
            ));
        }
        for other in &paths[..index] {
            if disk::overlaps(other, path) || disk::eq_name(leaf(other)?, filename) {
                return Err(Error::message_at(
                    path,
                    format!("与追踪路径 {} 重叠或同名", other.display()),
                ));
            }
        }
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    let file = path.join(MANIFEST);
    if !disk::required(&file)?.is_file() {
        return Err(Error::message_at(&file, "无效的快照标识"));
    }
    let manifest: Manifest = disk::read(&file)?;
    if manifest.version != 1
        || manifest.id.is_nil()
        || manifest.paths.is_empty()
        || chrono::DateTime::from_timestamp(manifest.created, 0).is_none()
        || manifest
            .paths
            .iter()
            .any(|p| !p.is_absolute() || p.file_name().is_none())
    {
        return Err(Error::message_at(&file, "无效的快照标识"));
    }
    Ok(manifest)
}

fn has_payload(path: &Path, manifest: &Manifest) -> bool {
    manifest.paths.iter().all(|original| {
        leaf(original).is_ok_and(|name| {
            !disk::eq_name(name, OsStr::new(MANIFEST))
                && disk::required(&path.join(name))
                    .is_ok_and(|meta| meta.is_file() || meta.is_dir())
        })
    })
}

impl Manager {
    pub fn snapshots(&self, game: Uuid) -> Result<Vec<Snapshot>> {
        let root = &self.game(game)?.input.root;
        disk::no_links(root)?;
        let mut snapshots = Vec::new();
        for entry in disk::at(root, fs::read_dir(root))? {
            let entry = disk::at(root, entry)?;
            if let Some((number, name)) = parse_directory(&entry.file_name())
                && disk::at(root, entry.file_type())?.is_dir()
                && disk::required(&entry.path()).is_ok_and(|m| m.is_dir())
                && let Ok(manifest) = read_manifest(&entry.path())
                && manifest.number == number
                && has_payload(&entry.path(), &manifest)
            {
                snapshots.push(Snapshot {
                    id: manifest.id,
                    number,
                    name,
                    created: manifest.created,
                    path: entry.path(),
                });
            }
        }
        snapshots.sort_by(|a, b| {
            b.created
                .cmp(&a.created)
                .then(b.number.cmp(&a.number))
                .then(a.path.cmp(&b.path))
        });
        Ok(snapshots)
    }

    pub(crate) fn creation_path(&self, game: Uuid) -> PathBuf {
        self.state.join(format!("create-{game}.json"))
    }

    pub(crate) fn finish_creation(&mut self, game: Uuid) -> Result<()> {
        let journal = self.creation_path(game);
        if disk::metadata(&journal)?.is_none() {
            return Ok(());
        }
        let operation: Creation = disk::read(&journal)?;
        disk::no_links(&operation.root)?;
        if !disk::required(&operation.root)?.is_dir() {
            return Err(Error::message_at(&operation.root, "快照根目录不可用"));
        }
        let target = operation.target();
        let staging = operation.staging();
        let published = if disk::metadata(&staging)?.is_some() {
            disk::remove(&staging)?;
            false
        } else if disk::metadata(&target)?.is_some() {
            disk::required(&target)?;
            disk::metadata(&target.join(MANIFEST))?.is_some()
                && read_manifest(&target)?.id == operation.id
        } else {
            false
        };
        if published && self.game(game)?.next <= operation.number {
            let mut games = self.games.clone();
            let entry = games
                .iter_mut()
                .find(|entry| entry.id == game)
                .expect("existing game");
            entry.next = increment(operation.number)?;
            self.write_games(games)?;
        }
        disk::remove(&journal)
    }

    pub fn create_snapshot(&mut self, game: Uuid, name: &str) -> Result<()> {
        self.ensure_ready(game)?;
        let entry = self.game(game)?;
        let root = entry.input.root.clone();
        let paths = entry
            .input
            .paths
            .iter()
            .map(|path| disk::absolute(path))
            .collect::<Result<Vec<_>>>()?;
        if paths.is_empty() {
            return Err(Error::Message("请先添加追踪路径".into()));
        }
        validate_paths(&root, &self.state, &paths)?;
        let used = occupied(&root)?;
        let mut number = entry.next;
        while used.contains(&number) {
            number = increment(number)?;
        }
        increment(number)?;
        let operation = Creation {
            directory: directory(number, name)?,
            root,
            id: Uuid::new_v4(),
            number,
        };
        let staging = operation.staging();
        let journal = self.creation_path(game);
        disk::write(&journal, &operation)?;
        let result = (|| {
            disk::at(&staging, fs::create_dir(&staging))?;
            for path in &paths {
                disk::copy(path, &staging.join(leaf(path)?))?;
            }
            let manifest = Manifest {
                version: 1,
                id: operation.id,
                number,
                created: chrono::Utc::now().timestamp(),
                paths,
            };
            // The whole directory is private until published. A separate temporary
            // manifest name could collide with a tracked file.
            let file = staging.join(MANIFEST);
            disk::write_new(&file, &manifest)?;
            if occupied(&operation.root)?.contains(&number) {
                return Err(Error::message_at(&operation.target(), "编号已被占用"));
            }
            disk::rename_new(&staging, &operation.target())
        })();
        let cleanup = self.finish_creation(game);
        match (result, cleanup) {
            (Err(error), Err(cleanup)) => {
                Err(Error::Message(format!("{error}\n清理失败：{cleanup}")))
            }
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    pub(crate) fn resolve(&self, game: Uuid, snapshot: &Snapshot) -> Result<(PathBuf, Manifest)> {
        let root = &self.game(game)?.input.root;
        disk::no_links(root)?;
        let path = &snapshot.path;
        if path.parent() != Some(root.as_path()) || !disk::required(path)?.is_dir() {
            return Err(Error::message_at(path, "快照不在当前根目录"));
        }
        let manifest = read_manifest(path)?;
        if manifest.id != snapshot.id || manifest.number != snapshot.number {
            return Err(Error::message_at(path, "快照已变化，请刷新后重试"));
        }
        Ok((path.clone(), manifest))
    }

    pub fn rename_snapshot(&self, game: Uuid, snapshot: &Snapshot, name: &str) -> Result<()> {
        self.ensure_ready(game)?;
        let (path, manifest) = self.resolve(game, snapshot)?;
        let target = path.with_file_name(directory(manifest.number, name)?);
        if path.as_os_str() == target.as_os_str() {
            return Ok(());
        }
        #[cfg(windows)]
        if disk::eq_name(leaf(&path)?, leaf(&target)?) {
            return disk::at(&target, fs::rename(path, &target));
        }
        disk::rename_new(&path, &target)
    }

    pub fn delete_snapshot(&self, game: Uuid, snapshot: &Snapshot) -> Result<()> {
        self.ensure_ready(game)?;
        let (path, _) = self.resolve(game, snapshot)?;
        crate::platform::recycle(&path)
    }
}
