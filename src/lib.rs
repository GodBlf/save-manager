mod disk;
pub mod platform;
mod restore;
mod snapshot;

use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub use restore::Recovery;
pub use snapshot::Snapshot;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    Message(String),
}

impl Error {
    pub fn message_at(path: &Path, message: impl std::fmt::Display) -> Self {
        Self::Message(format!("{}: {message}", path.display()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInput {
    pub name: String,
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    pub id: Uuid,
    pub input: GameInput,
    next: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
}

pub struct Manager {
    state: PathBuf,
    games: Vec<Game>,
    notices: Vec<String>,
    _lock: File,
}

impl Manager {
    pub fn open(state: &Path) -> Result<Self> {
        disk::at(state, fs::create_dir_all(state))?;
        let state = disk::absolute(state)?;
        let lock_path = state.join("manager.lock");
        let lock = disk::at(
            &lock_path,
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&lock_path),
        )?;
        disk::at(&lock_path, fs2::FileExt::try_lock_exclusive(&lock))?;
        let config = state.join("games.json");
        let games = match disk::metadata(&config)? {
            Some(_) => disk::read(&config)?,
            None => Vec::new(),
        };
        let mut manager = Self {
            state,
            games,
            notices: Vec::new(),
            _lock: lock,
        };
        for game in manager.games.clone() {
            if let Err(error) = manager.finish_creation(game.id) {
                manager.notices.push(error.to_string());
            }
        }
        Ok(manager)
    }

    pub fn games(&self) -> &[Game] {
        &self.games
    }

    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    pub fn appearance(&self) -> Result<Option<Appearance>> {
        let path = self.state.join("appearance.json");
        disk::metadata(&path)?
            .map(|_| disk::read(&path))
            .transpose()
    }

    pub fn save_appearance(&self, appearance: Appearance) -> Result<()> {
        disk::write(&self.state.join("appearance.json"), &appearance)
    }

    pub fn game(&self, id: Uuid) -> Result<&Game> {
        self.games
            .iter()
            .find(|game| game.id == id)
            .ok_or_else(|| Error::Message("找不到该游戏配置".into()))
    }

    fn write_games(&mut self, games: Vec<Game>) -> Result<()> {
        disk::write(&self.state.join("games.json"), &games)?;
        self.games = games;
        Ok(())
    }

    pub fn save_game(&mut self, id: Option<Uuid>, mut input: GameInput) -> Result<Uuid> {
        input.name = input.name.trim().to_owned();
        if input.name.is_empty() {
            return Err(Error::Message("请输入游戏名称".into()));
        }
        input.root = disk::absolute(&input.root)?;
        if !disk::required(&input.root)?.is_dir() {
            return Err(Error::message_at(&input.root, "请选择目录"));
        }
        if disk::overlaps(&input.root, &self.state) {
            return Err(Error::message_at(
                &input.root,
                "快照根目录与应用数据目录重叠",
            ));
        }
        for path in &mut input.paths {
            *path = disk::absolute(path)?;
        }
        snapshot::validate_paths(&input.root, &self.state, &input.paths)?;
        if self
            .games
            .iter()
            .any(|game| Some(game.id) != id && disk::overlaps(&game.input.root, &input.root))
        {
            return Err(Error::message_at(
                &input.root,
                "快照根目录与其他游戏配置冲突",
            ));
        }
        let mut games = self.games.clone();
        let id = if let Some(id) = id {
            self.ensure_ready(id)?;
            let game = games
                .iter_mut()
                .find(|game| game.id == id)
                .expect("existing game");
            game.input = input;
            id
        } else {
            let id = Uuid::new_v4();
            let next = snapshot::occupied(&input.root)?
                .into_iter()
                .max()
                .map(snapshot::increment)
                .transpose()?
                .unwrap_or(0);
            games.push(Game { id, input, next });
            id
        };
        self.write_games(games)?;
        Ok(id)
    }

    pub fn remove_game(&mut self, id: Uuid) -> Result<()> {
        self.ensure_ready(id)?;
        self.write_games(
            self.games
                .iter()
                .filter(|game| game.id != id)
                .cloned()
                .collect(),
        )
    }

    fn ensure_ready(&self, id: Uuid) -> Result<()> {
        self.game(id)?;
        if self.pending_recovery(id)?.is_some() {
            return Err(Error::Message("请先处理未完成的恢复".into()));
        }
        if disk::metadata(&self.creation_path(id))?.is_some() {
            return Err(Error::Message("请先重启应用以处理未完成的创建".into()));
        }
        Ok(())
    }
}
