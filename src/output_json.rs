use std::collections::HashSet;

use serde::Serialize;

use super::*;

#[derive(Debug, Serialize)]
struct Output {
    version: String,
    root_workspace: Workspace,
    dependencies: Vec<Workspace>,
    tree: DependencyNode,
}

#[derive(Debug, Serialize)]
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

pub fn analyze_and_output_json(root_workspace: Workspace) {
    let tree = WorkspaceDependencyTree::new(root_workspace);
    let json_tree = json_workspace_dependency_tree(&tree);
    let (root_workspace, dependencies) = tree.to_root_workspace_and_dependencies();
    let output = Output {
        version: clap::crate_version!().into(),
        root_workspace: root_workspace,
        dependencies: dependencies,
        tree: json_tree,
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn json_workspace_dependency_tree(tree: &WorkspaceDependencyTree) -> DependencyNode {
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
