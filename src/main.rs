use std::collections::VecDeque;
use std::fs::*;
use std::path::PathBuf;

use clap::Parser;
use ini::*;

#[derive(Debug, Parser)]
struct Options {
    sws_file: PathBuf,
}

#[derive(Debug)]
struct Workspace {
    sws_path: PathBuf,
    df_version: Option<String>,
    appsrc_path: Vec<PathBuf>,
    ddsrc_path: Vec<PathBuf>,
    dependencies: Vec<PathBuf>,
}

impl Workspace {
    fn new(sws_file: PathBuf) -> Result<Workspace, String> {
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
                    if p.is_relative() && p.starts_with("..") {
                        std::path::absolute(root_folder.join(&p)).ok()
                    } else if p.is_absolute() {
                        Some(p)
                    } else {
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
                    sws_path: sws_file,
                    df_version: df_version.map(String::from),
                    appsrc_path: appsrc_path,
                    ddsrc_path: ddsrc_path,
                    dependencies: libraries,
                })
            } else {
                Ok(Workspace {
                    sws_path: sws_file,
                    df_version: df_version.map(String::from),
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

    fn all_dependencies(&self) -> Vec<Workspace> {
        let mut workspaces = Vec::new();
        let mut dependencies = VecDeque::from_iter(self.dependencies.iter().cloned());
        let mut visited = std::collections::HashSet::new();

        while let Some(library_sws) = dependencies.pop_front() {
            if visited.insert(library_sws.clone()) {
                match Workspace::new(library_sws) {
                    Ok(workspace) => {
                        dependencies.extend(workspace.dependencies.clone());
                        workspaces.push(workspace);
                    }
                    Err(e) => {
                        eprintln!("{e}");
                    }
                }
            }
        }
        workspaces
    }
}

fn main() -> Result<(), String> {
    let options = Options::parse();
    let root_workspace = Workspace::new(options.sws_file)?;
    println!("Root workspace:\n{:#?}", root_workspace);

    let libraries = root_workspace.all_dependencies();
    println!("Libraries:\n{:#?}", libraries);

    Ok(())
}
