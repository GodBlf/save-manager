use crate::{Error, Manager, Result, Snapshot, disk, snapshot};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Recovery {
    pub targets: Vec<PathBuf>,
    pub backups: Vec<PathBuf>,
    pub committed: bool,
}

#[derive(Serialize, Deserialize)]
struct Restore {
    phase: Phase,
    entries: Vec<Replacement>,
    missing_parents: Vec<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    Preparing,
    Applying,
    Committed,
}

#[derive(Serialize, Deserialize)]
struct Replacement {
    target: PathBuf,
    work: PathBuf,
    had_original: bool,
    incoming_fingerprint: Option<[u8; 32]>,
}

impl Replacement {
    fn incoming(&self) -> PathBuf {
        self.work.join("incoming")
    }

    fn backup(&self) -> PathBuf {
        self.work.join("original")
    }

    fn discarded(&self) -> PathBuf {
        self.work.join("discarded")
    }

    fn install(&self) -> Result<()> {
        if self.had_original {
            disk::rename_new(&self.target, &self.backup())?;
        }
        disk::rename_new(&self.incoming(), &self.target)
    }

    fn undo(&self) -> Result<()> {
        let backup = self.backup();
        let discarded = self.discarded();
        if disk::metadata(&discarded)?.is_some()
            && Some(disk::fingerprint(&discarded)?) != self.incoming_fingerprint
        {
            return Err(Error::message_at(
                &discarded,
                "此副本含有撤回期间发生的更改",
            ));
        }
        if disk::metadata(&backup)?.is_some() {
            self.set_aside_replacement()?;
            disk::rename_new(&backup, &self.target)?;
        } else if !self.had_original
            && disk::metadata(&self.incoming())?.is_none()
            && disk::metadata(&self.discarded())?.is_none()
            && disk::metadata(&self.work)?.is_some()
        {
            self.set_aside_replacement()?;
        }
        Ok(())
    }

    fn set_aside_replacement(&self) -> Result<()> {
        if disk::metadata(&self.target)?.is_some() {
            let expected = self.incoming_fingerprint;
            if disk::metadata(&self.incoming())?.is_some()
                || disk::metadata(&self.discarded())?.is_some()
                || Some(disk::fingerprint(&self.target)?) != expected
            {
                return Err(Error::message_at(
                    &self.target,
                    "存档已被其他操作修改，已保留当前内容",
                ));
            }
            disk::rename_new(&self.target, &self.discarded())?;
            if !disk::fingerprint(&self.discarded()).is_ok_and(|hash| Some(hash) == expected) {
                disk::rename_new(&self.discarded(), &self.target)?;
                return Err(Error::message_at(
                    &self.target,
                    "存档在撤回期间发生变化，已保留当前内容",
                ));
            }
        }
        Ok(())
    }
}

impl Restore {
    fn finish(&self) -> Result<()> {
        if self.phase == Phase::Applying {
            let mut failures = Vec::new();
            for entry in self.entries.iter().rev() {
                let volume = entry
                    .target
                    .ancestors()
                    .last()
                    .expect("absolute restore target");
                let result = disk::required(volume)
                    .and_then(|_| disk::no_links(&entry.work))
                    .and_then(|()| disk::no_links(&entry.target))
                    .and_then(|()| entry.undo());
                if let Err(error) = result {
                    failures.push(error.to_string());
                }
            }
            if !failures.is_empty() {
                return Err(Error::Message(failures.join("\n")));
            }
        }
        for entry in &self.entries {
            disk::required(
                entry
                    .target
                    .ancestors()
                    .last()
                    .expect("absolute restore target"),
            )?;
            disk::no_links(&entry.work)?;
            disk::remove(&entry.work)?;
        }
        if self.phase != Phase::Committed {
            for parent in self.missing_parents.iter().rev() {
                disk::no_links(parent)?;
                if let Err(error) = fs::remove_dir(parent)
                    && !matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                    )
                {
                    return disk::at(parent, Err(error));
                }
            }
        }
        Ok(())
    }

    fn recovery(&self) -> Recovery {
        Recovery {
            targets: self
                .entries
                .iter()
                .map(|entry| entry.target.clone())
                .collect(),
            backups: self
                .entries
                .iter()
                .flat_map(|entry| [entry.backup(), entry.discarded()])
                .filter(|path| disk::metadata(path).is_ok_and(|meta| meta.is_some()))
                .collect(),
            committed: self.phase == Phase::Committed,
        }
    }

    fn with_backup_paths(&self, error: Error) -> Error {
        let locations = self
            .recovery()
            .backups
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        if locations.is_empty() {
            return error;
        }
        Error::Message(format!("{error}\n原存档副本：\n{}", locations.join("\n")))
    }
}

impl Manager {
    fn recovery_path(&self, game: Uuid) -> PathBuf {
        self.state.join(format!("restore-{game}.json"))
    }

    pub fn has_pending_recovery(&self, game: Uuid) -> Result<bool> {
        self.game(game)?;
        Ok(disk::metadata(&self.recovery_path(game))?.is_some())
    }

    pub fn pending_recovery(&self, game: Uuid) -> Result<Option<Recovery>> {
        self.game(game)?;
        let journal = self.recovery_path(game);
        disk::metadata(&journal)?
            .map(|_| disk::read::<Restore>(&journal).map(|operation| operation.recovery()))
            .transpose()
    }

    pub fn recover_restore(&self, game: Uuid) -> Result<()> {
        self.game(game)?;
        let journal = self.recovery_path(game);
        let operation: Restore = disk::read(&journal)?;
        operation
            .finish()
            .map_err(|error| operation.with_backup_paths(error))?;
        disk::remove(&journal)
    }

    pub fn restore_targets(&self, game: Uuid, snapshot: &Snapshot) -> Result<Vec<PathBuf>> {
        self.ensure_ready(game)?;
        let (_, manifest) = self.resolve(game, snapshot)?;
        Ok(manifest.paths)
    }

    pub fn restore_snapshot(
        &self,
        game: Uuid,
        snapshot: &Snapshot,
        confirmed: &[PathBuf],
    ) -> Result<()> {
        self.ensure_ready(game)?;
        let (source, manifest) = self.resolve(game, snapshot)?;
        if manifest.paths != confirmed {
            return Err(Error::message_at(&source, "恢复目标已变化，请重新确认"));
        }
        let paths = manifest
            .paths
            .iter()
            .map(|path| disk::absolute(path))
            .collect::<Result<Vec<_>>>()?;
        snapshot::validate_paths(&self.game(game)?.input.root, &self.state, &paths)?;
        for other in &self.games {
            if let Some(path) = paths
                .iter()
                .find(|path| disk::overlaps(path, &other.input.root))
            {
                return Err(Error::message_at(path, "恢复目标与快照根目录重叠"));
            }
            if let Some(pending) = self.pending_recovery(other.id)?
                && let Some(path) = paths.iter().find(|path| {
                    pending
                        .targets
                        .iter()
                        .any(|target| disk::overlaps(path, target))
                })
            {
                return Err(Error::message_at(path, "与其他游戏未完成的恢复重叠"));
            }
        }
        let mut operation = Restore {
            phase: Phase::Preparing,
            entries: Vec::new(),
            missing_parents: Vec::new(),
        };
        for target in paths {
            let parent = target.parent().expect("validated target");
            for ancestor in parent.ancestors() {
                if disk::metadata(ancestor)?.is_none()
                    && !operation.missing_parents.iter().any(|p| {
                        disk::overlaps(p, ancestor)
                            && p.components().count() == ancestor.components().count()
                    })
                {
                    operation.missing_parents.push(ancestor.to_owned());
                }
            }
            let had_original = disk::metadata(&target)?.is_some();
            if had_original {
                disk::validate_tree(&target)?;
            }
            let work = parent.join(format!(".save-manager-restore-{}", Uuid::new_v4()));
            operation.entries.push(Replacement {
                target,
                work,
                had_original,
                incoming_fingerprint: None,
            });
        }
        operation
            .missing_parents
            .sort_by_key(|path| path.components().count());
        let journal = self.recovery_path(game);
        disk::write(&journal, &operation)?;
        let result: Result<()> = (|| {
            for parent in &operation.missing_parents {
                disk::at(parent, fs::create_dir(parent))?;
            }
            for (entry, original) in operation.entries.iter_mut().zip(manifest.paths) {
                disk::at(&entry.work, fs::create_dir(&entry.work))?;
                disk::copy(
                    &source.join(original.file_name().expect("validated path")),
                    &entry.incoming(),
                )?;
                entry.incoming_fingerprint = Some(disk::fingerprint(&entry.incoming())?);
            }
            operation.phase = Phase::Applying;
            disk::write(&journal, &operation)?;
            for entry in &operation.entries {
                disk::no_links(&entry.target)?;
                entry
                    .install()
                    .map_err(|error| Error::message_at(&entry.target, error))?;
            }
            operation.phase = Phase::Committed;
            if let Err(error) = disk::write(&journal, &operation) {
                operation.phase = Phase::Applying;
                return Err(error);
            }
            Ok(())
        })();
        let cleanup = operation.finish().and_then(|()| disk::remove(&journal));
        match (result, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(operation
                .with_backup_paths(Error::Message(format!("存档已恢复，清理失败：{error}")))),
            (Err(error), Err(cleanup)) => Err(operation.with_backup_paths(Error::Message(
                format!("{error}\n撤回或清理失败：{cleanup}"),
            ))),
        }
    }
}
