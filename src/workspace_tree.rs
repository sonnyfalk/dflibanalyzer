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
struct WorkspaceContent {
    workspace: Workspace,
    source_files: Vec<SourceFile>,
}

impl WorkspaceDependencyTree {
    pub fn new(root_workspace: Workspace) -> Self {
        let root_workspace_path = root_workspace.sws_path.clone();
        let libraries = root_workspace.recursively_specified_dependencies();
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

    pub fn root_workspace(&self) -> Option<&Workspace> {
        self.workspaces
            .get(&self.root_workspace_path)
            .map(|w| &w.workspace)
    }

    pub fn workspace_dependencies(&self, workspace: &Workspace) -> Vec<&Workspace> {
        self.workspaces
            .get(&workspace.sws_path)
            .map(|w| self.calculated_workspace_dependencies(w))
            .unwrap_or_default()
    }

    fn calculated_workspace_dependencies(
        &self,
        workspace_content: &WorkspaceContent,
    ) -> Vec<&Workspace> {
        let mut seen_workspaces = HashSet::new();
        seen_workspaces.insert(&workspace_content.workspace.sws_path);

        workspace_content
            .source_files
            .iter()
            .flat_map(|source_file| source_file.dependencies.iter())
            .filter_map(|file_dep| {
                match self.source_file_to_workspace_map.get(file_dep) {
                    Some(workspaces) => {
                        if workspaces.len() > 1 {
                            // FIXME: Ambigous workspace dependency, same file name is in multiple workspaces.
                        }
                        workspaces
                            .first()
                            .and_then(|p| self.workspaces.get(p))
                            .map(|wc| &wc.workspace)
                    }
                    None => {
                        //FIXME: Unresolved file dependency.
                        None
                    }
                }
            })
            .fold(Vec::new(), |mut result, workspace| {
                if seen_workspaces.insert(&workspace.sws_path) {
                    result.push(workspace);
                }
                result
            })
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
