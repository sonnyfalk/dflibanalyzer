use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::workspace::*;

#[derive(Debug)]
pub struct WorkspaceDependencyTree {
    workspaces: HashMap<PathBuf, WorkspaceContent>,
    root_workspace_path: PathBuf,
    source_file_to_workspace_map: HashMap<FileName, Vec<PathBuf>>,
}

#[derive(Debug, Clone)]
pub enum WorkspaceDependency<'a> {
    Dependency(&'a Workspace),
    Missing(&'a Workspace),
    Ambiguous(Vec<&'a Workspace>),
}

#[derive(Debug)]
struct WorkspaceContent {
    workspace: Workspace,
    source_files: Vec<SourceFile>,
}

struct WorkspaceTreeIterator<'a> {
    tree: &'a WorkspaceDependencyTree,
    strategy: IteratorStrategy,
    workspaces: VecDeque<&'a Workspace>,
    visited: HashSet<&'a Path>,
}

enum IteratorStrategy {
    BreadthFirst,
    DepthFirst,
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
        self.workspace(&self.root_workspace_path)
            .expect("Internal error: Must have a root workspace")
    }

    pub fn workspace(&self, workspace_path: &Path) -> Option<&Workspace> {
        self.workspaces.get(workspace_path).map(|w| &w.workspace)
    }

    pub fn defined_transitive_workspace_dependencies(
        &self,
        workspace: &Workspace,
    ) -> HashSet<PathBuf> {
        self.iter(workspace, IteratorStrategy::BreadthFirst)
            .map(|w| w.sws_path.clone())
            .collect()
    }

    pub fn defined_indirect_workspace_dependencies(
        &self,
        workspace: &Workspace,
    ) -> HashSet<PathBuf> {
        self.iter(workspace, IteratorStrategy::BreadthFirst)
            .skip(1)
            .flat_map(|w| w.dependencies.iter())
            .cloned()
            .collect()
    }

    pub fn resolve_workspace_dependency_df26<'a>(
        &self,
        dependency: WorkspaceDependency<'a>,
    ) -> WorkspaceDependency<'a> {
        self.resolve_workspace_dependency(dependency, IteratorStrategy::BreadthFirst, false)
    }

    pub fn resolve_workspace_dependency_df25<'a>(
        &self,
        dependency: WorkspaceDependency<'a>,
    ) -> WorkspaceDependency<'a> {
        self.resolve_workspace_dependency(dependency, IteratorStrategy::DepthFirst, true)
    }

    fn iter<'a>(
        &'a self,
        workspace: &'a Workspace,
        strategy: IteratorStrategy,
    ) -> impl Iterator<Item = &'a Workspace> {
        WorkspaceTreeIterator::new(self, workspace, strategy)
    }

    fn resolve_workspace_dependency<'a>(
        &self,
        dependency: WorkspaceDependency<'a>,
        strategy: IteratorStrategy,
        take_first: bool,
    ) -> WorkspaceDependency<'a> {
        let WorkspaceDependency::Ambiguous(dependencies) = dependency else {
            return dependency;
        };
        let sws_dependencies: HashSet<_> = dependencies.iter().map(|w| &w.sws_path).collect();
        if let Some(sibling_dependencies) = self
            .iter(self.root_workspace(), strategy)
            .map(|w| &w.dependencies)
            .find(|dependencies| dependencies.iter().any(|p| sws_dependencies.contains(p)))
        {
            let ordered_dependencies: Vec<_> = sibling_dependencies
                .iter()
                .filter_map(|p| dependencies.iter().find(|w| &w.sws_path == p))
                .collect();
            if ordered_dependencies.len() == 1 || take_first {
                WorkspaceDependency::Dependency(ordered_dependencies.first().unwrap())
            } else {
                WorkspaceDependency::Ambiguous(dependencies)
            }
        } else {
            WorkspaceDependency::Ambiguous(dependencies)
        }
    }

    pub fn all_duplicate_filenames(&self) -> HashMap<FileName, WorkspaceDependency<'_>> {
        self.source_file_to_workspace_map
            .iter()
            .filter(|(_, workspace_paths)| workspace_paths.len() > 1)
            .map(|(source_file, workspace_paths)| {
                (
                    source_file.clone(),
                    WorkspaceDependency::Ambiguous(
                        workspace_paths
                            .iter()
                            .filter_map(|p| self.workspace(p))
                            .collect(),
                    ),
                )
            })
            .collect()
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
                            // For now, try to resolve the ambiguity with BFS search.
                            // FIXME: This should check both before and after 26 behavior, compare the difference in results.
                            let workspace_dependencies = workspaces
                                .iter()
                                .filter_map(|p| self.workspaces.get(p))
                                .map(|wc| &wc.workspace)
                                .collect();
                            match self.resolve_workspace_dependency_df26(
                                WorkspaceDependency::Ambiguous(workspace_dependencies),
                            ) {
                                // FIXME: Consider indicating reverse dependencies, and/or shadowed references
                                WorkspaceDependency::Dependency(_) => None,
                                dep => Some(dep),
                            }
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
            Self::Missing(dependency) => Some(dependency),
            Self::Ambiguous(_) => None,
        }
    }

    pub fn all(&self) -> Vec<&'a Workspace> {
        match self {
            Self::Dependency(dependency) => vec![dependency],
            Self::Missing(dependency) => vec![dependency],
            Self::Ambiguous(dependencies) => dependencies.clone(),
        }
    }
}

impl<'a> std::fmt::Display for WorkspaceDependency<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dependency(dependency) => write!(f, "{}", dependency.name()),
            Self::Missing(dependency) => write!(f, "{}", dependency.name()),
            Self::Ambiguous(dependencies) => write!(
                f,
                "{}",
                dependencies
                    .iter()
                    .map(|w| w.name())
                    .collect::<Vec<_>>()
                    .join(if f.alternate() { "\n" } else { ", " })
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

impl<'a> WorkspaceTreeIterator<'a> {
    fn new(
        tree: &'a WorkspaceDependencyTree,
        workspace: &'a Workspace,
        strategy: IteratorStrategy,
    ) -> Self {
        Self {
            tree,
            strategy,
            workspaces: VecDeque::from_iter(std::iter::once(workspace)),
            visited: HashSet::from_iter(std::iter::once(workspace.sws_path.as_path())),
        }
    }
}

impl<'a> Iterator for WorkspaceTreeIterator<'a> {
    type Item = &'a Workspace;

    fn next(&mut self) -> Option<Self::Item> {
        let workspace = self.workspaces.pop_front()?;
        for dependency in workspace
            .dependencies
            .iter()
            .filter(|p| self.visited.insert(p))
            .filter_map(|p| self.tree.workspace(p))
        {
            match self.strategy {
                IteratorStrategy::BreadthFirst => self.workspaces.push_back(dependency),
                IteratorStrategy::DepthFirst => self.workspaces.push_front(dependency),
            }
        }

        Some(workspace)
    }
}

impl<'a> WorkspaceDependency<'a> {
    pub fn sort_order(&self) -> usize {
        match self {
            WorkspaceDependency::Dependency(_) => 3,
            WorkspaceDependency::Missing(_) => 2,
            WorkspaceDependency::Ambiguous(_) => 1,
        }
    }

    pub fn color(&self) -> colored::Color {
        match self {
            WorkspaceDependency::Dependency(_) => colored::Color::Green,
            WorkspaceDependency::Missing(_) => colored::Color::Yellow,
            WorkspaceDependency::Ambiguous(_) => colored::Color::Red,
        }
    }
}
