use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::workspace::*;

#[derive(Debug)]
pub struct WorkspaceDependencyTree {
    workspaces: HashMap<PathBuf, WorkspaceContent>,
    root_workspace_path: PathBuf,
    source_file_to_workspace_map: HashMap<FileName, Vec<PathBuf>>,
}

#[derive(Debug)]
pub enum WorkspaceDependency<'a> {
    Dependency(&'a Workspace),
    Ambiguous(Vec<&'a Workspace>),
}

#[derive(Debug)]
struct WorkspaceContent {
    workspace: Workspace,
    source_files: Vec<SourceFile>,
}

impl WorkspaceDependencyTree {
    pub fn new(root_workspace: Workspace) -> Self {
        let root_workspace_path = root_workspace.sws_path.clone();
        let libraries = root_workspace.all_defined_dependency_workspaces();
        let workspaces: HashMap<PathBuf, WorkspaceContent> = std::iter::once(root_workspace)
            .chain(libraries)
            .map(|ws| (ws.sws_path.clone(), WorkspaceContent::new(ws)))
            .collect();
        let source_file_to_workspace_map = workspaces.iter().fold(
            HashMap::<FileName, Vec<PathBuf>>::new(),
            |mut map, (workspace_path, workspace)| {
                for source_file in &workspace.source_files {
                    if let Some(file_name) = source_file.path.file_name() {
                        map.entry(file_name.into())
                            .or_default()
                            .push(workspace_path.clone());
                    }
                }
                map
            },
        );
        Self {
            workspaces,
            root_workspace_path,
            source_file_to_workspace_map,
        }
    }

    pub fn root_workspace(&self) -> &Workspace {
        self.workspaces
            .get(&self.root_workspace_path)
            .map(|w| &w.workspace)
            .expect("Internal error: Must have a root workspace")
    }

    pub fn defined_transitive_workspace_dependencies(
        &self,
        workspace: &Workspace,
    ) -> HashSet<PathBuf> {
        let mut all_dependencies = HashSet::<PathBuf>::new();
        let mut workspaces: Vec<&Workspace> = vec![workspace];
        while let Some(workspace) = workspaces.pop() {
            for dependency in workspace
                .dependencies
                .iter()
                .filter(|&p| all_dependencies.insert(p.clone()))
                .filter_map(|p| self.workspaces.get(p))
                .map(|wc| &wc.workspace)
            {
                workspaces.push(dependency);
            }
        }
        all_dependencies
    }

    pub fn analyze_source_dependency(
        &self,
        workspace: &Workspace,
        dependency: &WorkspaceDependency,
    ) -> Option<Vec<SourceFile>> {
        let content = self.workspaces.get(&workspace.sws_path)?;
        let dependencies = dependency.all();
        let files: Vec<SourceFile> = content
            .source_files
            .iter()
            .filter_map(|source_file| {
                let deps: Vec<FileName> = source_file
                    .dependencies
                    .iter()
                    .filter(|dep| {
                        self.source_file_to_workspace_map
                            .get(dep)
                            .is_some_and(|workspaces| {
                                dependencies
                                    .iter()
                                    .all(|dependency| workspaces.contains(&dependency.sws_path))
                            })
                    })
                    .cloned()
                    .collect();
                if !deps.is_empty() {
                    Some(SourceFile {
                        path: source_file.path.clone(),
                        dependencies: deps,
                    })
                } else {
                    None
                }
            })
            .collect();

        if !files.is_empty() { Some(files) } else { None }
    }

    pub fn calculated_workspace_dependencies(
        &self,
        workspace: &Workspace,
    ) -> Vec<WorkspaceDependency<'_>> {
        let Some(workspace_content) = self.workspaces.get(&workspace.sws_path) else {
            return Vec::new();
        };

        let mut seen_workspaces = HashSet::new();
        seen_workspaces
            .insert(WorkspaceDependency::Dependency(&workspace_content.workspace).to_string());

        workspace_content
            .source_files
            .iter()
            .flat_map(|source_file| source_file.dependencies.iter())
            .filter_map(|file_dep| {
                match self.source_file_to_workspace_map.get(file_dep) {
                    Some(workspaces) => {
                        if workspaces.len() > 1 {
                            // Ambiguous workspace dependency, same file name is in multiple workspaces.
                            // Before DataFlex 26, the makepath order was depth-first,
                            // and after DataFlex 26, the makepath order is breadth-first.
                            // For now, flag all as ambiguous to establish a baseline.
                            // FIXME: This should check both before and after 26 behavior, compare the difference in results.
                            let workspace_dependencies = workspaces
                                .iter()
                                .filter_map(|p| self.workspaces.get(p))
                                .map(|wc| &wc.workspace)
                                .collect();
                            Some(WorkspaceDependency::Ambiguous(workspace_dependencies))
                            // // Ambiguous workspace dependency, same file name is in multiple workspaces.
                            // // Disambiguate with defined direct dependencies, which will match with makepath ordering.
                            // let matching_dependencies: Vec<_> = workspace_content
                            //     .workspace
                            //     .dependencies
                            //     .iter()
                            //     .filter(|dep| workspaces.contains(dep))
                            //     .collect();
                            // if matching_dependencies.len() == 1 {
                            //     matching_dependencies
                            //         .first()
                            //         .and_then(|&p| self.workspaces.get(p))
                            //         .map(|wc| WorkspaceDependency::Dependency(&wc.workspace))
                            // } else if matching_dependencies.len() > 1 {
                            //     let workspace_dependencies = matching_dependencies
                            //         .iter()
                            //         .filter_map(|&p| self.workspaces.get(p))
                            //         .map(|wc| &wc.workspace)
                            //         .collect();
                            //     Some(WorkspaceDependency::Ambiguous(workspace_dependencies))
                            // } else {
                            //     let workspace_dependencies = workspaces
                            //         .iter()
                            //         .filter_map(|p| self.workspaces.get(p))
                            //         .map(|wc| &wc.workspace)
                            //         .collect();
                            //     Some(WorkspaceDependency::Ambiguous(workspace_dependencies))
                            // }
                        } else {
                            workspaces
                                .first()
                                .and_then(|p| self.workspaces.get(p))
                                .map(|wc| WorkspaceDependency::Dependency(&wc.workspace))
                        }
                    }
                    None => {
                        //FIXME: Unresolved file dependency.
                        None
                    }
                }
            })
            .fold(Vec::new(), |mut result, workspace_dependency| {
                if seen_workspaces.insert(workspace_dependency.to_string()) {
                    result.push(workspace_dependency);
                }
                result
            })
    }
}

impl<'a> WorkspaceDependency<'a> {
    pub fn only_one(&self) -> Option<&Workspace> {
        match self {
            Self::Dependency(dependency) => Some(dependency),
            Self::Ambiguous(_) => None,
        }
    }

    pub fn all(&self) -> Vec<&'a Workspace> {
        match self {
            Self::Dependency(dependency) => vec![dependency],
            Self::Ambiguous(dependencies) => dependencies.clone(),
        }
    }
}

impl<'a> std::fmt::Display for WorkspaceDependency<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dependency(dependency) => write!(f, "{}", dependency.name()),
            Self::Ambiguous(dependencies) => write!(
                f,
                "{}",
                dependencies
                    .iter()
                    .map(|w| w.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl WorkspaceContent {
    fn new(workspace: Workspace) -> Self {
        let source_files = workspace.workspace_source_files();
        Self {
            workspace,
            source_files,
        }
    }
}
