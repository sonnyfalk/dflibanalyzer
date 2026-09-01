use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::*;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    version: String,
    root_workspace: Workspace,
    dependencies: Vec<Workspace>,
    tree: DependencyNode,
    conflicting_files: Vec<ConflictingFile>,
    errors: Vec<DependencyIssue>,
    warnings: Vec<DependencyIssue>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DependencyNode {
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<DependencyNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ambiguous_use: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    conflicting_use: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    reverse_use: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    missing_dependencies: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConflictingFile {
    name: String,
    candidates: Vec<ConflictingFileCandidate>,
    df26: String,
    df25: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConflictingFileCandidate {
    workspace: String,
    path: PathBuf,
    group: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DependencyIssue {
    description: String,
    workspace: String,
    related_workspaces: Vec<String>,
    df26: Option<String>,
    df25: Option<String>,
    impacted_files: Vec<PathBuf>,
    referenced_files: Vec<PathBuf>,
    suggestions: Vec<String>,
}

pub fn analyze_and_output_json(root_workspace: Workspace) {
    let tree = WorkspaceDependencyTree::new(root_workspace);
    let json_tree = workspace_dependency_tree(&tree);
    let conflicting_files = conflicting_files(&tree);
    let (errors, warnings) = dependency_issues(&tree);
    let (root_workspace, dependencies) = tree.to_root_workspace_and_dependencies();
    let output = Output {
        version: clap::crate_version!().into(),
        root_workspace: root_workspace,
        dependencies: dependencies,
        tree: json_tree,
        conflicting_files: conflicting_files,
        errors: errors,
        warnings: warnings,
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn workspace_dependency_tree(tree: &WorkspaceDependencyTree) -> DependencyNode {
    fn node(
        workspace: &Workspace,
        tree: &WorkspaceDependencyTree,
        seen: &mut HashSet<PathBuf>,
    ) -> DependencyNode {
        if seen.insert(workspace.sws_path.clone()) {
            let dependencies = workspace
                .dependencies
                .iter()
                .filter_map(|p| tree.workspace(p))
                .map(|w| node(w, tree, seen))
                .collect();
            let defined_dependencies = tree.defined_transitive_workspace_dependencies(workspace);
            let calculated_dependencies = tree.calculated_workspace_dependencies(workspace);
            DependencyNode {
                name: workspace.name().into(),
                dependencies: dependencies,
                ambiguous_use: calculated_dependencies
                    .iter()
                    .filter_map(|dep| {
                        if matches!(dep, WorkspaceDependency::Ambiguous(_)) {
                            Some(dep.to_string())
                        } else {
                            None
                        }
                    })
                    .collect(),
                conflicting_use: calculated_dependencies
                    .iter()
                    .filter_map(|dep| {
                        if matches!(dep, WorkspaceDependency::Conflicting(_, _)) {
                            Some(dep.to_string())
                        } else {
                            None
                        }
                    })
                    .collect(),
                reverse_use: calculated_dependencies
                    .iter()
                    .filter_map(|dep| {
                        if matches!(dep, WorkspaceDependency::Reverse(_)) {
                            Some(dep.to_string())
                        } else {
                            None
                        }
                    })
                    .collect(),
                missing_dependencies: calculated_dependencies
                    .iter()
                    .filter_map(|dep| {
                        if let WorkspaceDependency::Dependency(dep) = dep
                            && !defined_dependencies.contains(&dep.sws_path)
                        {
                            Some(WorkspaceDependency::Missing(dep).to_string())
                        } else {
                            None
                        }
                    })
                    .collect(),
            }
        } else {
            DependencyNode {
                name: workspace.name().into(),
                dependencies: Vec::new(),
                ambiguous_use: Vec::new(),
                conflicting_use: Vec::new(),
                reverse_use: Vec::new(),
                missing_dependencies: Vec::new(),
            }
        }
    }

    node(tree.root_workspace(), tree, &mut HashSet::new())
}

fn conflicting_files(tree: &WorkspaceDependencyTree) -> Vec<ConflictingFile> {
    let all_duplicate_filenames = tree.all_duplicate_filenames();
    all_duplicate_filenames
        .into_iter()
        .map(|(file, dependency)| {
            let candidates: Vec<_> = dependency
                .all()
                .into_iter()
                .filter_map(|candidate| {
                    tree.source_file_in_workspace(&file, candidate)
                        .map(|p| (candidate, p))
                })
                .collect();
            let groups = group_identical_files(candidates.iter().map(|(_, p)| *p));
            let candidates: Vec<_> = candidates
                .into_iter()
                .map(|(workspace, path)| ConflictingFileCandidate {
                    workspace: workspace.name().into(),
                    path: path.clone(),
                    group: groups[path],
                })
                .collect();
            let resolved26 = match tree.resolve_workspace_dependency_df26(dependency.clone()) {
                WorkspaceDependency::Dependency(dep) => Some(dep.name().to_string()),
                _ => None,
            };
            let resolved25 = match tree.resolve_workspace_dependency_df25(dependency.clone()) {
                WorkspaceDependency::Dependency(dep) => Some(dep.name().to_string()),
                _ => None,
            };
            ConflictingFile {
                name: file.to_string(),
                candidates: candidates,
                df26: resolved26.unwrap_or("(Unresolved)".into()),
                df25: resolved25.unwrap_or("(Unresolved)".into()),
            }
        })
        .collect()
}

fn group_identical_files<'a>(
    files: impl Iterator<Item = &'a PathBuf>,
) -> HashMap<&'a PathBuf, usize> {
    let mut entries: Vec<_> = files
        .map(|path| {
            std::fs::read(path)
                .map_err(|_| path)
                .map(|bytes| (path, bytes))
        })
        .collect::<Vec<_>>();

    let error_entries: Vec<_> = entries
        .extract_if(.., |entry| entry.is_err())
        .map(|entry| entry.unwrap_err())
        .collect();

    let mut entries: Vec<_> = entries.into_iter().map(|entry| entry.unwrap()).collect();
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    entries
        .chunk_by(|a, b| a.1 == b.1)
        .map(|chunk| chunk.iter().map(|entry| entry.0).collect::<Vec<_>>())
        .chain(error_entries.into_iter().map(|entry| vec![entry]))
        .enumerate()
        .flat_map(|(index, group)| group.into_iter().map(move |p| (p, index)))
        .collect()
}

fn dependency_issues(
    tree: &WorkspaceDependencyTree,
) -> (Vec<DependencyIssue>, Vec<DependencyIssue>) {
    tree.all_workspaces().fold(
        (Vec::new(), Vec::new()),
        |(mut errors, mut warnings), workspace| {
            let defined_dependencies = tree.defined_transitive_workspace_dependencies(workspace);
            let calculated_dependencies = tree.calculated_workspace_dependencies(workspace);
            for dep in calculated_dependencies {
                match dep {
                    WorkspaceDependency::Dependency(workspace) => {
                        if !defined_dependencies.contains(&workspace.sws_path) {
                            if let Some(issue) = dependency_issue(
                                workspace,
                                WorkspaceDependency::Missing(workspace),
                                tree,
                            ) {
                                warnings.push(issue);
                            }
                        }
                    }
                    WorkspaceDependency::Ambiguous(_) => {
                        if let Some(issue) = dependency_issue(workspace, dep, tree) {
                            errors.push(issue);
                        }
                    }
                    WorkspaceDependency::Missing(_) => {
                        if let Some(issue) = dependency_issue(workspace, dep, tree) {
                            warnings.push(issue);
                        }
                    }
                    WorkspaceDependency::Reverse(_) => {
                        if let Some(issue) = dependency_issue(workspace, dep, tree) {
                            warnings.push(issue);
                        }
                    }
                    WorkspaceDependency::Conflicting(_, _) => {
                        if let Some(issue) = dependency_issue(workspace, dep, tree) {
                            errors.push(issue);
                        }
                    }
                }
            }

            (errors, warnings)
        },
    )
}

fn dependency_issue(
    workspace: &Workspace,
    dep: WorkspaceDependency,
    tree: &WorkspaceDependencyTree,
) -> Option<DependencyIssue> {
    let (impacted_files, referenced_files) = tree
        .analyze_source_dependency(workspace, &dep)
        .map(|source_dependencies| {
            (
                source_dependencies
                    .iter()
                    .map(|s| &s.path)
                    .cloned()
                    .collect::<Vec<_>>(),
                source_dependencies
                    .iter()
                    .flat_map(|s| s.dependencies.iter())
                    .cloned()
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();

    match dep {
        WorkspaceDependency::Dependency(_) => None,
        WorkspaceDependency::Missing(dep) => Some(DependencyIssue {
            description: "Missing library dependency.".into(),
            workspace: workspace.name().into(),
            df26: None,
            df25: None,
            related_workspaces: vec![dep.name().into()],
            impacted_files,
            referenced_files: referenced_files
                .into_iter()
                .filter_map(|f| tree.source_file_in_workspace(&f, dep))
                .cloned()
                .collect(),
            suggestions: vec![format!(
                "Consider adding {} as a library dependency to {}",
                dep.name(),
                workspace.name()
            )],
        }),
        WorkspaceDependency::Reverse(dep) => Some(DependencyIssue {
            description: "Reverse Use references.".into(),
            workspace: workspace.name().into(),
            related_workspaces: vec![dep.name().into()],
            df26: None,
            df25: None,
            impacted_files,
            referenced_files: referenced_files
                .into_iter()
                .filter_map(|f| tree.source_file_in_workspace(&f, dep))
                .cloned()
                .collect(),
            suggestions: vec![format!(
                "Consider moving impacted files from {} up the tree to {}",
                workspace.name(),
                dep.name(),
            )],
        }),
        WorkspaceDependency::Ambiguous(ref candidates) => {
            let indirect_dependencies = tree.defined_indirect_workspace_dependencies(workspace);
            let df25_resolution = match tree.resolve_workspace_dependency_df25(dep.clone()) {
                WorkspaceDependency::Dependency(df25_resolution) => Some(df25_resolution),
                _ => None,
            };
            let other = WorkspaceDependency::Ambiguous(
                dep.all()
                    .into_iter()
                    .filter(|d| {
                        df25_resolution
                            .map(|df25| d.name() != df25.name())
                            .unwrap_or(true)
                    })
                    .collect(),
            );
            let suggestion = if other
                .all()
                .iter()
                .all(|w| indirect_dependencies.contains(&w.sws_path))
            {
                let is_multiple = other.all().len() > 1;
                if is_multiple {
                    format!(
                        "Consider removing {} as library dependencies to {}. They're already included via indirect dependencies.",
                        other.to_string(),
                        workspace.name()
                    )
                } else {
                    format!(
                        "Consider removing {} as a library dependency to {}. It's already included via indirect dependencies.",
                        other.to_string(),
                        workspace.name()
                    )
                }
            } else {
                format!(
                    "Consider pushing down {} in the dependency tree",
                    other.to_string(),
                )
            };
            Some(DependencyIssue {
                description: "Ambiguous Use references.".into(),
                workspace: workspace.name().into(),
                related_workspaces: candidates
                    .into_iter()
                    .map(|w| w.name().to_string())
                    .collect(),
                df26: Some("(Unresolved)".into()),
                df25: df25_resolution.map(|df25| df25.name().into()),
                impacted_files,
                referenced_files: referenced_files
                    .into_iter()
                    .flat_map(|f| {
                        candidates
                            .iter()
                            .filter_map(move |dep| tree.source_file_in_workspace(&f, dep))
                    })
                    .cloned()
                    .collect(),
                suggestions: vec![suggestion],
            })
        }
        WorkspaceDependency::Conflicting(df26_resolution, df25_resolution) => {
            let candidates = [df26_resolution, df25_resolution];
            Some(DependencyIssue {
                description: "Conflicting File Resolution.".into(),
                workspace: workspace.name().into(),
                related_workspaces: vec![
                    df26_resolution.name().into(),
                    df25_resolution.name().into(),
                ],
                df26: Some(df26_resolution.name().into()),
                df25: Some(df25_resolution.name().into()),
                impacted_files,
                referenced_files: referenced_files
                    .into_iter()
                    .flat_map(|f| {
                        candidates
                            .iter()
                            .filter_map(move |dep| tree.source_file_in_workspace(&f, dep))
                    })
                    .cloned()
                    .collect(),
                suggestions: vec![
                    format!(
                        "Consider whether it's possible to push {} down in the dependency tree",
                        df26_resolution.name(),
                    ),
                    format!(
                        "Manually merge and resolve the source file conflicts with the same name."
                    ),
                ],
            })
        }
    }
}
