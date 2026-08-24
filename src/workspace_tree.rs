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
    Reverse(&'a Workspace),
    Ambiguous(Vec<&'a Workspace>),
    Conflicting(&'a Workspace, &'a Workspace),
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

    pub fn all_workspaces(&self) -> impl Iterator<Item = &Workspace> {
        self.iter(self.root_workspace(), IteratorStrategy::BreadthFirst)
    }

    pub fn defined_transitive_workspace_dependencies(
        &self,
        workspace: &Workspace,
    ) -> HashSet<PathBuf> {
        self.iter(workspace, IteratorStrategy::BreadthFirst)
            .flat_map(|w| w.dependencies.iter())
            .cloned()
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
        let WorkspaceDependency::Ambiguous(candidates) = dependency else {
            return dependency;
        };
        let sws_candidates: HashSet<_> = candidates.iter().map(|w| &w.sws_path).collect();
        if let Some(sibling_dependencies) = self
            .iter(self.root_workspace(), strategy)
            .map(|w| &w.dependencies)
            .find(|dependencies| dependencies.iter().any(|p| sws_candidates.contains(p)))
        {
            let mut ordered_candidates: Vec<_> = sibling_dependencies
                .iter()
                .filter_map(|p| candidates.iter().find(|w| &w.sws_path == p).map(|w| *w))
                .collect();
            if ordered_candidates.len() == 1 || take_first {
                WorkspaceDependency::Dependency(ordered_candidates.first().unwrap())
            } else {
                let all_transitive_dependencies: HashSet<_> = ordered_candidates
                    .iter()
                    .flat_map(|w| {
                        self.defined_transitive_workspace_dependencies(w)
                            .into_iter()
                    })
                    .collect();
                let transitive_candidates: Vec<_> = ordered_candidates
                    .extract_if(.., |w| all_transitive_dependencies.contains(&w.sws_path))
                    .collect();
                if ordered_candidates.len() == 1 {
                    WorkspaceDependency::Dependency(ordered_candidates.first().unwrap())
                } else if !ordered_candidates.is_empty() {
                    WorkspaceDependency::Ambiguous(ordered_candidates)
                } else {
                    WorkspaceDependency::Ambiguous(transitive_candidates)
                }
            }
        } else {
            WorkspaceDependency::Ambiguous(candidates)
        }
    }

    pub fn all_duplicate_filenames(&self) -> Vec<(FileName, WorkspaceDependency<'_>)> {
        let mut files: Vec<_> = self
            .source_file_to_workspace_map
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
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    pub fn source_file_in_workspace(
        &self,
        file: &FileName,
        workspace: &Workspace,
    ) -> Option<&PathBuf> {
        self.workspaces
            .get(&workspace.sws_path)
            .and_then(|wc| {
                wc.source_files.iter().find(|f| {
                    f.path
                        .file_name()
                        .map(FileName::from)
                        .is_some_and(|f| f == *file)
                })
            })
            .map(|f| &f.path)
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
                            let workspace_dependencies: Vec<_> = workspaces
                                .iter()
                                .filter_map(|p| self.workspaces.get(p))
                                .map(|wc| &wc.workspace)
                                .collect();
                            match self.resolve_workspace_dependency_df26(
                                WorkspaceDependency::Ambiguous(workspace_dependencies.clone()),
                            ) {
                                WorkspaceDependency::Dependency(df26_resolution) => {
                                    if let WorkspaceDependency::Dependency(df25_resolution) = self
                                        .resolve_workspace_dependency_df25(
                                            WorkspaceDependency::Ambiguous(
                                                workspace_dependencies.clone(),
                                            ),
                                        )
                                    {
                                        if df25_resolution.sws_path == df26_resolution.sws_path {
                                            // They both resolve to the same, no conflict.
                                            None
                                        } else {
                                            // They resolve to different libraries, flag a conflict.
                                            Some(WorkspaceDependency::Conflicting(
                                                df26_resolution,
                                                df25_resolution,
                                            ))
                                        }
                                    } else {
                                        // Ambiguous by default.
                                        Some(WorkspaceDependency::Ambiguous(workspace_dependencies))
                                    }
                                }
                                dep => Some(dep),
                            }
                        } else {
                            workspaces
                                .first()
                                .and_then(|p| self.workspaces.get(p))
                                .map(|wc| {
                                    if self
                                        .defined_transitive_workspace_dependencies(&wc.workspace)
                                        .contains(&workspace.sws_path)
                                    {
                                        WorkspaceDependency::Reverse(&wc.workspace)
                                    } else {
                                        WorkspaceDependency::Dependency(&wc.workspace)
                                    }
                                })
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

    pub fn to_root_workspace_and_dependencies(mut self) -> (Workspace, Vec<Workspace>) {
        let root_path = self.root_workspace_path;
        let root_workspace = self
            .workspaces
            .extract_if(|p, _| *p == root_path)
            .map(|(_, wc)| wc.workspace)
            .next()
            .unwrap();
        let dependencies = self
            .workspaces
            .into_iter()
            .map(|(_, wc)| wc.workspace)
            .collect();
        (root_workspace, dependencies)
    }
}

impl<'a> WorkspaceDependency<'a> {
    pub fn all(&self) -> Vec<&'a Workspace> {
        match self {
            Self::Dependency(dependency) => vec![dependency],
            Self::Missing(dependency) => vec![dependency],
            Self::Reverse(dependency) => vec![dependency],
            Self::Ambiguous(dependencies) => dependencies.clone(),
            Self::Conflicting(df26_resolution, df25_resolition) => {
                vec![df26_resolution, df25_resolition]
            }
        }
    }
}

impl<'a> std::fmt::Display for WorkspaceDependency<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dependency(dependency) => write!(f, "{}", dependency.name()),
            Self::Missing(dependency) => write!(f, "{}", dependency.name()),
            Self::Reverse(dependency) => write!(f, "{}", dependency.name()),
            Self::Ambiguous(dependencies) => write!(
                f,
                "{}",
                dependencies
                    .iter()
                    .map(|w| w.name())
                    .collect::<Vec<_>>()
                    .join(if f.alternate() { "\n" } else { ", " })
            ),
            Self::Conflicting(df26_resolution, df25_resolution) => {
                write!(f, "{} / {}", df26_resolution.name(), df25_resolution.name())
            }
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
            WorkspaceDependency::Dependency(_) => 5,
            WorkspaceDependency::Missing(_) => 4,
            WorkspaceDependency::Reverse(_) => 3,
            WorkspaceDependency::Conflicting(_, _) => 2,
            WorkspaceDependency::Ambiguous(_) => 1,
        }
    }

    pub fn color(&self) -> colored::Color {
        match self {
            WorkspaceDependency::Dependency(_) => colored::Color::Green,
            WorkspaceDependency::Missing(_) => colored::Color::Yellow,
            WorkspaceDependency::Reverse(_) => colored::Color::Yellow,
            WorkspaceDependency::Ambiguous(_) => colored::Color::Red,
            WorkspaceDependency::Conflicting(_, _) => colored::Color::Red,
        }
    }
}
