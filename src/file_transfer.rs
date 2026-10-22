use crate::protocol::FileEntry;
use std::path::PathBuf;
use thiserror::Error;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[derive(Debug, Error)]
pub enum FileError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("path not allowed: {0}")]
    PathNotAllowed(String),
    #[error("not a directory")]
    NotADirectory,
}

pub struct FileSession {
    root: PathBuf,
}

impl FileSession {
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self, FileError> {
        let root = root.into();
        if !root.is_dir() {
            fs::create_dir_all(&root).await?;
        }
        let root = fs::canonicalize(&root).await.unwrap_or(root);
        Ok(Self { root })
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, FileError> {
        let requested = self.root.join(path.strip_prefix('/').unwrap_or(path));
        let resolved = std::fs::canonicalize(&requested).unwrap_or_else(|_| requested.clone());
        if !resolved.starts_with(&self.root) {
            return Err(FileError::PathNotAllowed(path.into()));
        }
        Ok(resolved)
    }

    pub async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, FileError> {
        let resolved = self.resolve(path)?;
        let mut entries = Vec::new();
        let mut read_dir = fs::read_dir(&resolved).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let metadata = entry.metadata().await?;
            let name = entry.file_name().to_string_lossy().to_string();
            let full_path = entry.path();
            let rel = full_path
                .strip_prefix(&self.root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| name.clone());
            entries.push(FileEntry {
                name,
                path: rel,
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                modified: metadata
                    .modified()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    })
                    .unwrap_or(0),
            });
        }
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        Ok(entries)
    }

    pub async fn read_chunk(
        &self,
        path: &str,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, FileError> {
        let resolved = self.resolve(path)?;
        let mut file = fs::File::open(&resolved).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut buf = vec![0u8; length as usize];
        let n = file.read(&mut buf).await?;
        buf.truncate(n);
        Ok(buf)
    }

    pub async fn write_chunk(
        &self,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, FileError> {
        let resolved = self.resolve(path)?;
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&resolved)
            .await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        file.write_all(data).await?;
        Ok(data.len() as u32)
    }
}
