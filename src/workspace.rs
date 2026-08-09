use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::*;
use std::path::{Path, PathBuf};

use ini::*;
use serde::Deserialize;

#[derive(Debug)]
pub struct Workspace {
    pub sws_path: PathBuf,
    pub _df_version: Option<String>,
    pub appsrc_path: Vec<PathBuf>,
    pub ddsrc_path: Vec<PathBuf>,
    pub dependencies: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct JsonWorkspaceFile {
    dependencies: Option<Vec<serde_json::Value>>,
    paths: Option<JsonWorkspacePaths>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonWorkspacePaths {
    app_src: Option<serde_json::Value>,
    dd_src: Option<serde_json::Value>,
}

#[derive(Debug)]
pub struct SourceFile {
    pub path: PathBuf,
    pub dependencies: Vec<FileName>,
}

#[derive(Debug, Clone)]
pub struct FileName(String);

impl Workspace {
    pub fn new(sws_file: &PathBuf) -> Result<Workspace, String> {
        Self::try_new_from_json_file(sws_file).or_else(|_| Self::try_new_from_ini_file(sws_file))
    }

    fn try_new_from_json_file(sws_file: &PathBuf) -> Result<Workspace, String> {
        let sws_content = read_to_string(&sws_file).map_err(|e| {
            format!(
                "Couldn't open workspace file '{}': {}",
                sws_file.to_string_lossy(),
                e
            )
        })?;
        if let Ok(workspace_file) = serde_json::from_str::<JsonWorkspaceFile>(&sws_content) {
            let root_folder = sws_file
                .parent()
                .map(|p| p.to_path_buf())
                .expect("Internal error: must have a sws root folder");
            let appsrc_paths: Vec<_> = workspace_file
                .paths
                .as_ref()
                .and_then(|paths| paths.app_src.as_ref())
                .map(|value| match value {
                    serde_json::Value::String(s) => vec![PathBuf::from(s)],
                    serde_json::Value::Array(array) => array
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(PathBuf::from)
                        .collect(),
                    _ => Vec::new(),
                })
                .into_iter()
                .flat_map(|v| v.into_iter())
                .filter_map(|p| {
                    if p.is_relative() {
                        std::path::absolute(root_folder.join(p)).ok()
                    } else {
                        Some(p)
                    }
                })
                .collect();
            let ddsrc_paths: Vec<_> = workspace_file
                .paths
                .as_ref()
                .and_then(|paths| paths.dd_src.as_ref())
                .map(|value| match value {
                    serde_json::Value::String(s) => vec![PathBuf::from(s)],
                    serde_json::Value::Array(array) => array
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(PathBuf::from)
                        .collect(),
                    _ => Vec::new(),
                })
                .into_iter()
                .flat_map(|v| v.into_iter())
                .filter_map(|p| {
                    if p.is_relative() {
                        std::path::absolute(root_folder.join(p)).ok()
                    } else {
                        Some(p)
                    }
                })
                .collect();
            let dependencies: Vec<_> = workspace_file
                .dependencies
                .iter()
                .flat_map(|deps| deps.iter())
                .filter_map(|value| value.as_str())
                .filter(|s| s.starts_with("..") || s.starts_with("/"))
                .map(PathBuf::from)
                .filter_map(|p| {
                    if p.is_relative() {
                        std::path::absolute(root_folder.join(p)).ok()
                    } else {
                        Some(p)
                    }
                })
                .collect();

            Ok(Workspace {
                sws_path: sws_file.clone(),
                _df_version: None,
                appsrc_path: if !appsrc_paths.is_empty() {
                    appsrc_paths
                } else {
                    vec![root_folder.join("AppSrc")]
                },
                ddsrc_path: if !ddsrc_paths.is_empty() {
                    ddsrc_paths
                } else {
                    vec![root_folder.join("DdSrc")]
                },
                dependencies,
            })
        } else {
            Err(format!(
                "Couldn't read workspace file '{}': Unrecognized format",
                sws_file.to_string_lossy()
            ))
        }
    }

    fn try_new_from_ini_file(sws_file: &PathBuf) -> Result<Workspace, String> {
        let sws_content = read_to_string(&sws_file).map_err(|e| {
            format!(
                "Couldn't open workspace file '{}': {}",
                sws_file.to_string_lossy(),
                e
            )
        })?;
        if let Ok(ini) = Ini::load_from_str_noescape(&sws_content) {
            let root_folder = sws_file
                .parent()
                .map(|p| p.to_path_buf())
                .expect("Internal error: must have a sws root folder");
            let df_version = ini
                .section(Some("Properties"))
                .and_then(|properties| properties.get("Version"));
            let libraries = ini
                .section(Some("Libraries"))
                .iter()
                .flat_map(|libraries| libraries.iter())
                .map(|(_, l)| PathBuf::from(l))
                .filter_map(|p| {
                    if p.is_relative() {
                        std::path::absolute(root_folder.join(&p)).ok()
                    } else if p.is_absolute() {
                        Some(p)
                    } else {
                        eprintln!(
                            "Unrecognized library path: {}, referenced from {}",
                            p.to_string_lossy(),
                            sws_file.to_string_lossy()
                        );
                        None
                    }
                })
                .collect();
            let config_path = ini
                .section(Some("WorkspacePaths"))
                .and_then(|properties| properties.get("ConfigFile"))
                .map(PathBuf::from)
                .and_then(|p| {
                    if p.is_relative() {
                        std::path::absolute(root_folder.join(&p)).ok()
                    } else if p.is_absolute() {
                        Some(p)
                    } else {
                        None
                    }
                });
            if let Some(config_path) = config_path
                && let Some(config_ini) = Ini::load_from_file_noescape(&config_path).ok()
                && let Some(section) = config_ini.section(Some("Workspace"))
            {
                let home = section
                    .get("Home")
                    .map(PathBuf::from)
                    .and_then(|p| {
                        if p.is_relative() {
                            std::path::absolute(config_path.parent().unwrap().join(&p)).ok()
                        } else if p.is_absolute() {
                            Some(p)
                        } else {
                            eprintln!(
                                "Warning: Unrecognized path '{}' while processing '{}'",
                                p.to_string_lossy(),
                                config_path.to_string_lossy()
                            );
                            None
                        }
                    })
                    .unwrap_or_else(|| root_folder.clone());
                let appsrc_path = section
                    .get("AppSrcPath")
                    .iter()
                    .flat_map(|p| p.split(';'))
                    .map(PathBuf::from)
                    .filter_map(|p| {
                        if p.is_relative() {
                            std::path::absolute(home.join(&p)).ok()
                        } else if p.is_absolute() {
                            Some(p)
                        } else {
                            eprintln!(
                                "Warning: Unrecognized path '{}' while processing '{}'",
                                p.to_string_lossy(),
                                config_path.to_string_lossy()
                            );
                            None
                        }
                    })
                    .collect();
                let ddsrc_path = section
                    .get("DDSrcPath")
                    .iter()
                    .flat_map(|p| p.split(';'))
                    .map(PathBuf::from)
                    .filter_map(|p| {
                        if p.is_relative() {
                            std::path::absolute(home.join(&p)).ok()
                        } else if p.is_absolute() {
                            Some(p)
                        } else {
                            eprintln!(
                                "Warning: Unrecognized path '{}' while processing '{}'",
                                p.to_string_lossy(),
                                config_path.to_string_lossy()
                            );
                            None
                        }
                    })
                    .collect();
                Ok(Workspace {
                    sws_path: sws_file.clone(),
                    _df_version: df_version.map(String::from),
                    appsrc_path: appsrc_path,
                    ddsrc_path: ddsrc_path,
                    dependencies: libraries,
                })
            } else {
                Ok(Workspace {
                    sws_path: sws_file.clone(),
                    _df_version: df_version.map(String::from),
                    appsrc_path: vec![root_folder.join("AppSrc")],
                    ddsrc_path: vec![root_folder.join("DdSrc")],
                    dependencies: libraries,
                })
            }
        } else {
            Err(format!(
                "Couldn't read workspace file '{}': Unrecognized format",
                sws_file.to_string_lossy()
            ))
        }
    }

    pub fn name(&self) -> std::borrow::Cow<'_, str> {
        self.sws_path.file_name().unwrap().to_string_lossy()
    }

    pub fn all_defined_dependency_workspaces(&self) -> Vec<Workspace> {
        let mut workspaces = Vec::new();
        let mut dependencies = VecDeque::from_iter(self.dependencies.iter().cloned());
        let mut visited = std::collections::HashSet::new();

        while let Some(library_sws) = dependencies.pop_front()
            && visited.insert(library_sws.clone())
        {
            match Workspace::new(&library_sws) {
                Ok(workspace) => {
                    dependencies.extend(workspace.dependencies.clone());
                    workspaces.push(workspace);
                }
                Err(e) => {
                    eprintln!("{e}");
                }
            }
        }
        workspaces
    }

    pub fn workspace_source_files(&self) -> Vec<SourceFile> {
        self.appsrc_path
            .iter()
            .chain(self.ddsrc_path.iter())
            .fold(Vec::new(), |mut result, dir| {
                collect_source_files(dir, &mut result);
                result
            })
            .into_iter()
            .filter_map(|path| match source_file_dependencies(&path) {
                Ok(dependencies) => Some(SourceFile { path, dependencies }),
                Err(e) => {
                    eprintln!("{e}");
                    None
                }
            })
            .collect()
    }
}

fn collect_source_files(dir: &Path, result: &mut Vec<PathBuf>) {
    let Ok(entries) = read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, result);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext,
                    "pkg" | "vw" | "wo" | "sl" | "dd" | "src" | "dg" | "bp" | "rv" | "fd" | "inc"
                )
            })
        {
            result.push(path);
        }
    }
}

fn source_file_dependencies(path: &Path) -> Result<Vec<FileName>, String> {
    let bytes = read(path).map_err(|e| {
        format!(
            "Couldn't read source file, skipping '{}': {e}",
            path.to_string_lossy()
        )
    })?;

    let os_content = unsafe { OsString::from_encoded_bytes_unchecked(bytes) };
    let content = os_content.to_string_lossy();

    Ok(content
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            if words
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case("Use"))
                && let Some(file_name) = words.next()
            {
                Some(file_name.into())
            } else {
                None
            }
        })
        .collect())
}

impl From<&str> for FileName {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<&OsStr> for FileName {
    fn from(value: &OsStr) -> Self {
        Self(value.to_string_lossy().to_string())
    }
}

impl PartialEq for FileName {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for FileName {}

impl std::hash::Hash for FileName {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_ascii_lowercase().hash(state);
    }
}

impl std::fmt::Display for FileName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
