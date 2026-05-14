//! Project database scaffolding for the vendored ty integration boundary.

use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
use ruff_db::files::{File, system_path_to_file};
use ruff_db::system::{OsSystem, SystemPathBuf};
use ruff_python_ast::name::Name;
use ty_project::{ProjectDatabase, ProjectMetadata};

use crate::facade::{FacadeError, FacadeResult};
use crate::targets::{FileId, FileInfo};

/// Strato-owned project handle around a vendored ty project database.
#[derive(Debug)]
pub struct StratoProject {
    root: PathBuf,
    db: ProjectDatabase,
    files: Vec<ProjectFile>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProjectFile {
    pub(crate) id: FileId,
    pub(crate) raw: File,
}

impl StratoProject {
    /// Creates a ty-backed Strato project for the Python files under `root`.
    pub fn from_root(root: impl AsRef<Path>) -> FacadeResult<Self> {
        let root = root.as_ref().to_path_buf();
        let paths = collect_python_files(&root)?;
        Self::from_paths(root, paths)
    }

    /// Creates a ty-backed Strato project from an explicit discovery manifest path list.
    pub fn from_paths(
        root: impl AsRef<Path>,
        paths: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> FacadeResult<Self> {
        let root = root.as_ref().to_path_buf();
        let system_root = system_path(&root)?;
        let metadata = ProjectMetadata::new(Name::new("strato"), system_root.clone());
        let system = OsSystem::new(&system_root);
        let mut db = ProjectDatabase::fallible(metadata, system)
            .map_err(|error| FacadeError::ProjectSetup(error.to_string()))?;

        let mut paths = paths
            .into_iter()
            .map(|path| path.as_ref().to_path_buf())
            .collect::<Vec<_>>();
        paths.sort();

        let mut files = Vec::with_capacity(paths.len());
        for (index, path) in paths.into_iter().enumerate() {
            let system_path = system_path(&path)?;
            File::sync_path(&mut db, &system_path);
            let raw = system_path_to_file(&db, &system_path)
                .map_err(|error| FacadeError::FileLoad(format!("{}: {error}", path.display())))?;
            files.push(ProjectFile {
                id: FileId::new(index),
                raw,
            });
        }

        Ok(Self { root, db, files })
    }

    /// Returns the project root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) const fn db(&self) -> &ProjectDatabase {
        &self.db
    }

    pub(crate) fn project_file(&self, id: FileId) -> Option<ProjectFile> {
        self.files
            .get(id.index())
            .copied()
            .filter(|file| file.id == id)
    }

    pub(crate) fn contains_raw_file(&self, file: File) -> bool {
        self.files.iter().any(|candidate| candidate.raw == file)
    }

    /// Returns deterministic file metadata for all project Python files.
    #[must_use]
    pub fn files(&self) -> Vec<FileInfo> {
        self.files
            .iter()
            .map(|file| {
                let path = file.raw.path(&self.db).to_string();
                FileInfo::new(file.id, PathBuf::from(path), file.raw.is_stub(&self.db))
            })
            .collect()
    }
}

fn system_path(path: &Path) -> FacadeResult<SystemPathBuf> {
    let absolute = path
        .canonicalize()
        .map_err(|error| FacadeError::FileLoad(format!("{}: {error}", path.display())))?;
    let utf8 = Utf8PathBuf::from_path_buf(absolute).map_err(|path| {
        FacadeError::FileLoad(format!("path is not valid UTF-8: {}", path.display()))
    })?;
    Ok(SystemPathBuf::from_utf8_path_buf(utf8))
}

fn collect_python_files(root: &Path) -> FacadeResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_python_files_impl(root, &mut files)?;
    Ok(files)
}

fn collect_python_files_impl(path: &Path, files: &mut Vec<PathBuf>) -> FacadeResult<()> {
    let entries = std::fs::read_dir(path)
        .map_err(|error| FacadeError::FileLoad(format!("{}: {error}", path.display())))?;
    for entry in entries {
        let entry = entry.map_err(|error| FacadeError::FileLoad(error.to_string()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| FacadeError::FileLoad(format!("{}: {error}", path.display())))?;
        if file_type.is_dir() {
            collect_python_files_impl(&path, files)?;
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("py" | "pyi")
        ) {
            files.push(path);
        }
    }
    Ok(())
}
