/// This module represents a WorkspacePath, that is a path in the  project, possibly relative to the project root.
/// It is implemented as a separate type, with explicit conversions to PathBuf, so that type safety
/// prevents us from confusing it with either Strings, or PathBuf's
use anyhow::{anyhow, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialOrd, Ord, PartialEq, Eq, Clone)]
pub enum WorkspacePath {
    Workspace(PathBuf),
    NonWorkspace(PathBuf),
}

fn normalize_file(file: &Path, workspace_root: &Option<String>) -> PathBuf {
    if let Some(root) = workspace_root {
        match file.strip_prefix(root) {
            Ok(path) => PathBuf::from(format!("//{}", path.display())),
            Err(_) => file.to_owned(),
        }
    } else {
        file.to_owned()
    }
}

impl WorkspacePath {
    pub fn new(path: PathBuf) -> Self {
        if let Some(stripped) = path.to_str().unwrap_or_default().strip_prefix("//") {
            Self::Workspace(PathBuf::from(stripped))
        } else {
            Self::NonWorkspace(path)
        }
    }

    pub fn from_full_path(path: &Path, root: &Option<String>) -> Self {
        Self::new(normalize_file(path, root))
    }

    pub fn to_path(&self, root: &Option<String>) -> Result<PathBuf> {
        match self {
            Self::NonWorkspace(path) => Ok(path.clone()),
            Self::Workspace(path) => {
                let root = root
                    .as_ref()
                    .ok_or(anyhow!("Workspace relative paths used and no workspace_root specified"))?;
                Ok(PathBuf::from(root).join(path))
            }
        }
    }
}

impl<'a> From<&'a Path> for WorkspacePath {
    fn from(path: &'a Path) -> Self {
        Self::new(path.to_owned())
    }
}

impl From<PathBuf> for WorkspacePath {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl<'a> From<&'a str> for WorkspacePath {
    fn from(s: &'a str) -> Self {
        Self::new(PathBuf::from(s))
    }
}

impl From<String> for WorkspacePath {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl fmt::Display for WorkspacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonWorkspace(path) => {
                write!(f, "{}", path.display())
            }
            Self::Workspace(path) => {
                write!(f, "//{}", path.display())
            }
        }
    }
}

impl<'de> Deserialize<'de> for WorkspacePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string_result = String::deserialize(deserializer);
        string_result.map(Into::into)
    }
}

impl Serialize for WorkspacePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
