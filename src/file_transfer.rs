//! Capability-scoped, bounded file transfer helpers.
use crate::protocol::FileEntry;
use ambient_authority::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024;
pub const MAX_CHUNK_SIZE: u32 = 1024 * 1024;
pub const MAX_PATH_BYTES: usize = 4096;

#[derive(Debug, Error)]
pub enum FileError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("path not allowed: {0}")]
    PathNotAllowed(String),
    #[error("path contains a symbolic link: {0}")]
    SymlinkNotAllowed(String),
    #[error("file or requested range exceeds configured limit")]
    TooLarge,
    #[error("requested chunk exceeds {MAX_CHUNK_SIZE} bytes")]
    ChunkTooLarge,
    #[error("not a directory")]
    NotADirectory,
}

#[derive(Clone)]
pub struct FileSession {
    root: std::sync::Arc<Dir>,
}

impl FileSession {
    pub async fn new(root: impl Into<PathBuf>) -> Result<Self, FileError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let root = std::fs::canonicalize(root)?;
        Ok(Self {
            root: std::sync::Arc::new(Dir::open_ambient_dir(root, ambient_authority())?),
        })
    }

    fn relative(path: &str) -> Result<PathBuf, FileError> {
        if path.len() > MAX_PATH_BYTES {
            return Err(FileError::PathNotAllowed(path.into()));
        }
        let path = Path::new(path);
        if path.is_absolute() {
            return Err(FileError::PathNotAllowed(path.display().to_string()));
        }
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Normal(component) => out.push(component),
                Component::CurDir => {}
                _ => return Err(FileError::PathNotAllowed(path.display().to_string())),
            }
        }
        Ok(out)
    }

    /// `Dir` anchors resolution at an opened root capability. Rejecting every
    /// symlink component additionally prevents symlink-based access.
    fn reject_symlinks(&self, relative: &Path) -> Result<(), FileError> {
        let mut current = PathBuf::new();
        for component in relative.components() {
            current.push(component);
            match self.root.symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(FileError::SymlinkNotAllowed(current.display().to_string()))
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn check_range(offset: u64, length: u32) -> Result<(), FileError> {
        if length > MAX_CHUNK_SIZE {
            return Err(FileError::ChunkTooLarge);
        }
        if offset
            .checked_add(length as u64)
            .ok_or(FileError::TooLarge)?
            > MAX_FILE_SIZE
        {
            return Err(FileError::TooLarge);
        }
        Ok(())
    }

    pub async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, FileError> {
        let relative = Self::relative(path)?;
        let root = self.clone();
        tokio::task::spawn_blocking(move || {
            root.reject_symlinks(&relative)?;
            let directory = root.root.open_dir(&relative)?;
            let mut entries = Vec::new();
            for entry in directory.read_dir(".")? {
                let entry = entry?;
                let metadata = entry.metadata()?;
                if metadata.file_type().is_symlink() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                entries.push(FileEntry {
                    name: name.clone(),
                    path: relative.join(&name).to_string_lossy().to_string(),
                    is_dir: metadata.is_dir(),
                    size: metadata.len().min(MAX_FILE_SIZE),
                    modified: metadata
                        .modified()
                        .ok()
                        .and_then(|value| {
                            value.into_std().duration_since(std::time::UNIX_EPOCH).ok()
                        })
                        .map_or(0, |value| value.as_secs()),
                });
            }
            entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
            Ok(entries)
        })
        .await
        .map_err(|error| FileError::Io(std::io::Error::other(error)))?
    }

    pub async fn read_chunk(
        &self,
        path: &str,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, FileError> {
        Self::check_range(offset, length)?;
        let relative = Self::relative(path)?;
        let root = self.clone();
        tokio::task::spawn_blocking(move || {
            root.reject_symlinks(&relative)?;
            let metadata = root.root.metadata(&relative)?;
            if !metadata.is_file() || metadata.len() > MAX_FILE_SIZE {
                return Err(FileError::TooLarge);
            }
            let mut file = root.root.open(&relative)?;
            file.seek(SeekFrom::Start(offset))?;
            let mut out = vec![0; length as usize];
            let read = file.read(&mut out)?;
            out.truncate(read);
            Ok(out)
        })
        .await
        .map_err(|error| FileError::Io(std::io::Error::other(error)))?
    }

    pub async fn write_chunk(
        &self,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, FileError> {
        let length = u32::try_from(data.len()).map_err(|_| FileError::ChunkTooLarge)?;
        Self::check_range(offset, length)?;
        let relative = Self::relative(path)?;
        let data = data.to_vec();
        let root = self.clone();
        tokio::task::spawn_blocking(move || {
            root.reject_symlinks(&relative)?;
            let mut options = OpenOptions::new();
            options.create(true).write(true).read(true);
            let mut file = root.root.open_with(&relative, &options)?;
            if file
                .metadata()?
                .len()
                .max(offset)
                .saturating_add(data.len() as u64)
                > MAX_FILE_SIZE
            {
                return Err(FileError::TooLarge);
            }
            file.seek(SeekFrom::Start(offset))?;
            file.write_all(&data)?;
            Ok(length)
        })
        .await
        .map_err(|error| FileError::Io(std::io::Error::other(error)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

    fn temporary_root() -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "easycontrol-file-test-{}-{nonce}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[tokio::test]
    async fn rejects_traversal_symlink_and_oversized_chunks() {
        let root = temporary_root();
        let files = FileSession::new(&root).await.unwrap();
        assert!(matches!(
            files.write_chunk("../escape", 0, b"x").await,
            Err(FileError::PathNotAllowed(_))
        ));
        assert!(matches!(
            files
                .write_chunk("large", 0, &vec![0; MAX_CHUNK_SIZE as usize + 1])
                .await,
            Err(FileError::ChunkTooLarge)
        ));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", root.join("escape-link")).unwrap();
            assert!(matches!(
                files.read_chunk("escape-link/file", 0, 1).await,
                Err(FileError::SymlinkNotAllowed(_))
            ));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn permits_bounded_rooted_read_write() {
        let root = temporary_root();
        let files = FileSession::new(&root).await.unwrap();
        assert_eq!(files.write_chunk("note.txt", 0, b"safe").await.unwrap(), 4);
        assert_eq!(files.read_chunk("note.txt", 0, 8).await.unwrap(), b"safe");
        let _ = std::fs::remove_dir_all(root);
    }
}
