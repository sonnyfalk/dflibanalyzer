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

fn print_workspace_dependency_tree(tree: &WorkspaceDependencyTree) {
    let Some(root_workspace) = tree.root_workspace() else {
        println!("No workspaces");
        return;
    };

    println!();
    println!("{}", root_workspace.name().underline());
    print_workspace_dependencies(tree, root_workspace, 0, &mut HashSet::new());
    println!();
}

fn print_workspace_dependencies(
    tree: &WorkspaceDependencyTree,
    workspace: &Workspace,
    level: usize,
    visited: &mut HashSet<PathBuf>,
) {
    let level_str = "│   ";
    let connector_str = "├──";
    let last_connector_str = "└──";

    let specified_dependencies = &workspace.dependencies;
    let dependencies = tree.workspace_dependencies(workspace);
    let last_index = dependencies.len().saturating_sub(1);
    for (index, workspace) in dependencies.into_iter().enumerate() {
        let connector = if index < last_index {
            connector_str
        } else {
            last_connector_str
        };
        let is_specified = specified_dependencies.contains(&workspace.sws_path);
        let color = if is_specified {
            colored::Color::Green
        } else {
            colored::Color::Red
        };
        println!(
            "{}{} {}",
            level_str.repeat(level),
            connector,
            workspace.name().color(color)
        );
        if visited.insert(workspace.sws_path.clone()) {
            print_workspace_dependencies(tree, workspace, level + 1, visited);
        } else {
            println!(
                "{}{}{}",
                level_str.repeat(level + 1),
                last_connector_str,
                "(*)"
            );
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
    print_workspace_dependency_tree(&tree);

    Ok(())
}
