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
    dependency: &'a Workspace,
}

fn print_workspace_dependency_tree(tree: &WorkspaceDependencyTree) -> Vec<MissingDependency<'_>> {
    let root_workspace = tree.root_workspace();

    println!();
    println!("{}", root_workspace.name().bold());

    let mut missing_dependencies = Vec::new();
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

    let specified_dependencies = &workspace.dependencies;
    let dependencies = tree.workspace_dependencies(workspace);
    let last_index = dependencies.len().saturating_sub(1);
    for (index, dependency) in dependencies.into_iter().enumerate() {
        let connector = if index < last_index {
            connector_str
        } else {
            last_connector_str
        };
        let is_specified = specified_dependencies.contains(&dependency.sws_path);
        if !is_specified {
            missing_dependencies.push(MissingDependency {
                workspace,
                dependency,
            });
        }
        let color = if is_specified {
            colored::Color::Green
        } else {
            colored::Color::Red
        };
        println!("{}{} {}", prefix, connector, dependency.name().color(color));
        if visited.insert(dependency.sws_path.clone()) {
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
        } else {
            println!("{}{}{}", prefix, last_connector_str, "(*)");
            break;
        }
    }
}

fn main() -> Result<(), String> {
    let options = Options::parse();

    let root_workspace = Workspace::new(options.sws_file)?;
    if options.verbose {
        let libraries = root_workspace.recursively_specified_dependencies();
        println!("Root workspace config:");
        println!("{:#?}", root_workspace);
        println!();
        println!("Library workspaces config:");
        println!("{:#?}", libraries);
        println!();
    }
    let tree = WorkspaceDependencyTree::new(root_workspace);
    let missing_dependencies = print_workspace_dependency_tree(&tree);
    if !missing_dependencies.is_empty() {
        println!();
        println!("{}", "Potential missing dependencies:".red().bold());
        for missing_dependency in missing_dependencies {
            println!("{}", missing_dependency.workspace.name());
            println!("{} {}", "└──", missing_dependency.dependency.name().red());
            println!();
            print!(
                "Consider adding {} as library dependency to {}.",
                missing_dependency.dependency.name().bold(),
                missing_dependency.workspace.name().bold()
            );
            if let Some(source_dependencies) = tree.analyze_source_dependency(
                missing_dependency.workspace,
                missing_dependency.dependency,
            ) {
                print!("The following source file dependencies were detected: ");
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
        }
        println!()
    }
    println!();
    Ok(())
}
