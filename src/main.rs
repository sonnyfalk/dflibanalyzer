use std::collections::HashSet;
use std::path::PathBuf;

use clap::Parser;
use colored::Colorize;

mod workspace;
mod workspace_tree;

use workspace::*;
use workspace_tree::*;

#[derive(Debug, Parser)]
struct Options {
    sws_file: PathBuf,
    #[arg(short, long, action)]
    verbose: bool,
}

struct MissingDependency<'a> {
    workspace: &'a Workspace,
    dependency: WorkspaceDependency<'a>,
}

static OPTIONS: std::sync::OnceLock<Options> = std::sync::OnceLock::new();

impl Options {
    fn shared() -> &'static Options {
        OPTIONS.get().unwrap()
    }
}

fn print_workspace_dependency_tree(tree: &WorkspaceDependencyTree) -> Vec<MissingDependency<'_>> {
    let root_workspace = tree.root_workspace();

    println!();
    println!("{}", root_workspace.name().bold());

    let mut missing_dependencies = Vec::new();
    if root_workspace.dependencies.is_empty() {
        println!("└── {}", "(No dependencies)".green());
        return vec![];
    }

    print_workspace_dependencies(
        tree,
        root_workspace,
        "",
        &mut HashSet::new(),
        &mut missing_dependencies,
    );

    missing_dependencies
}

fn print_workspace_dependencies<'a>(
    tree: &'a WorkspaceDependencyTree,
    workspace: &'a Workspace,
    prefix: &str,
    visited: &mut HashSet<PathBuf>,
    missing_dependencies: &mut Vec<MissingDependency<'a>>,
) {
    let level_str = "│   ";
    let connector_str = "├──";
    let last_connector_str = "└──";

    let defined_dependencies = tree.defined_transitive_workspace_dependencies(workspace);
    let calculated_dependencies = tree.calculated_workspace_dependencies(workspace);
    let dependencies: Vec<WorkspaceDependency> = workspace
        .dependencies
        .iter()
        .filter_map(|p| tree.workspace(p))
        .map(|w| WorkspaceDependency::Dependency(w))
        .chain(
            calculated_dependencies
                .into_iter()
                .filter_map(|dep| match dep {
                    WorkspaceDependency::Dependency(workspace) => {
                        if !defined_dependencies.contains(&workspace.sws_path) {
                            Some(WorkspaceDependency::Missing(workspace))
                        } else {
                            None
                        }
                    }
                    WorkspaceDependency::Ambiguous(_) => Some(dep),
                    WorkspaceDependency::Missing(_) => Some(dep),
                }),
        )
        .collect();

    let last_index = dependencies.len().saturating_sub(1);
    for (index, dependency) in dependencies.into_iter().enumerate() {
        let connector = if index < last_index {
            connector_str
        } else {
            last_connector_str
        };
        let is_specified = matches!(dependency, WorkspaceDependency::Dependency(_));
        let color = if is_specified {
            colored::Color::Green
        } else {
            colored::Color::Red
        };
        println!(
            "{}{} {}{}",
            prefix,
            connector,
            dependency.to_string().color(color),
            match is_specified {
                true => "",
                false => match dependency {
                    WorkspaceDependency::Dependency(_) => "",
                    WorkspaceDependency::Missing(_) => " (Missing)",
                    WorkspaceDependency::Ambiguous(_) => " (Ambiguous)",
                },
            }
        );

        if let WorkspaceDependency::Dependency(dependency) = dependency
            && visited.insert(dependency.sws_path.clone())
        {
            let new_prefix = if index < last_index {
                format!("{prefix}{level_str}")
            } else {
                format!("{prefix}    ")
            };
            print_workspace_dependencies(
                tree,
                dependency,
                &new_prefix,
                visited,
                missing_dependencies,
            );
        }
        if !is_specified {
            missing_dependencies.push(MissingDependency {
                workspace,
                dependency,
            });
        }
    }
}

fn print_workspace_duplicate_files(tree: &WorkspaceDependencyTree) {
    print!("Files with same name in multiple libraries:",);
    let all_duplicate_filenames = tree.all_duplicate_filenames();
    if all_duplicate_filenames.is_empty() {
        println!(" {}", "None".green());
        return;
    }

    println!(" {}", all_duplicate_filenames.len());

    println!("{:─<25}───{:─<20}───{:─<40}", "", "", "");
    println!(
        "{: <25} │ {: <20} │ {}",
        "File", "Resolved", "All Libraries"
    );
    println!("{:─<25}─│─{:─<20}─│─{:─<40}", "", "", "");
    for (file, dependency) in all_duplicate_filenames {
        let file = file.to_string();
        let all_libraries = dependency.to_string();
        let resolved = match tree.resolve_workspace_dependency_bfs(dependency) {
            WorkspaceDependency::Dependency(dep) => dep.name().to_string(),
            _ => "(Unresolved)".to_string(),
        };
        println!("{: <25} │ {: <20} │ {}", file, resolved, all_libraries);
    }
    println!("{:─<25}───{:─<20}───{:─<40}", "", "", "");
}

fn print_missing_dependency(
    missing_dependency: &MissingDependency,
    tree: &WorkspaceDependencyTree,
) {
    println!();
    println!("{}", missing_dependency.workspace.name());
    println!(
        "{} {}",
        "└──",
        missing_dependency.dependency.to_string().red()
    );
    println!();
    if let Some(dep) = missing_dependency.dependency.only_one() {
        print!(
            "Missing library dependency. Consider adding {} as a library dependency to {}.",
            dep.name().bold(),
            missing_dependency.workspace.name().bold()
        );
    } else {
        print!(
            "Ambiguous library dependency with source file dependencies found in multiple libraries: {}.",
            missing_dependency.dependency.to_string().bold()
        );
        print!(
            " Check library dependencies of {}. Consider specifying unique direct library dependencies to match expected overriding behavior. Dependencies on all or none of {} makes it ambiguous.",
            missing_dependency.workspace.name().bold(),
            missing_dependency.dependency.to_string().bold()
        );
    }
    if let Some(source_dependencies) =
        tree.analyze_source_dependency(missing_dependency.workspace, &missing_dependency.dependency)
    {
        print!(" Analysis detected the following ambiguous source file dependencies: ");
        if let Some(source_file) = source_dependencies.first() {
            print!(
                "{} references {}",
                FileName::from(source_file.path.file_name().unwrap()),
                source_file
                    .dependencies
                    .iter()
                    .take(3)
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if source_file.dependencies.len() > 3 {
                print!("...")
            } else {
                print!(".");
            }
        }
        if source_dependencies.len() > 1 {
            print!(
                " And more from {}",
                source_dependencies
                    .iter()
                    .skip(1)
                    .take(3)
                    .map(|source_file| {
                        FileName::from(source_file.path.file_name().unwrap()).to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(","),
            );
            if source_dependencies.len() > 4 {
                println!("...")
            } else {
                println!(".")
            }
        }
    }
    println!();
}

fn main() -> Result<(), String> {
    _ = OPTIONS.set(Options::parse());

    let options = Options::shared();
    let root_workspace = Workspace::new(&options.sws_file)?;
    if options.verbose {
        let libraries = root_workspace.all_defined_dependency_workspaces();
        println!("Root workspace config:");
        println!("{:#?}", root_workspace);
        println!();
        println!("Library workspaces config:");
        println!("{:#?}", libraries);
    }
    println!();

    println!(
        "Analyzing local workspace dependencies for {}",
        root_workspace.name().bold()
    );
    let tree = WorkspaceDependencyTree::new(root_workspace);
    let missing_dependencies = print_workspace_dependency_tree(&tree);

    println!();
    print_workspace_duplicate_files(&tree);

    if !missing_dependencies.is_empty() {
        println!();
        println!(
            "{}: {}",
            "Dependency Error".red(),
            "Analysis found potential missing/ambiguous dependencies".bold()
        );
        for missing_dependency in missing_dependencies {
            print_missing_dependency(&missing_dependency, &tree);
        }
        println!()
    } else if tree.root_workspace().dependencies.is_empty() {
        println!();
        println!(
            "The workspace {} has no local library/package dependencies",
            tree.root_workspace().name().bold()
        );
    } else {
        println!();
        println!(
            "{}: {}",
            "Success".green(),
            "Analysis completed and all local library dependencies match".bold()
        );
    }
    println!();
    Ok(())
}
