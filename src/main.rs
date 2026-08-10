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
    /// Scan source files in AppSrc/DdSrc recursively.
    #[arg(short, long, action)]
    recursive_scan: bool,
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

    missing_dependencies.sort_by_key(|d| d.dependency.sort_order());
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
        println!(
            "{}{} {}{}",
            prefix,
            connector,
            dependency.to_string().color(dependency.color()),
            match dependency {
                WorkspaceDependency::Dependency(_) => "",
                WorkspaceDependency::Missing(_) => " (Missing)",
                WorkspaceDependency::Ambiguous(_) => " (Ambiguous)",
            },
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
    use comfy_table::{Cell, Color, ContentArrangement, Table};

    print!("Files with same name in multiple libraries:",);
    let all_duplicate_filenames = tree.all_duplicate_filenames();
    if all_duplicate_filenames.is_empty() {
        println!(" {}", "None".green());
        return;
    }

    println!(" {}", all_duplicate_filenames.len());

    let mut table = Table::new();
    table
        .load_style(comfy_table::presets::UTF8_FULL.with_rounded_corners())
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec!["File", "Resolved DF26 / DF25", "Candidate Libraries"]);

    for (file, dependency) in all_duplicate_filenames {
        let file = file.to_string();
        let resolved26 = match tree.resolve_workspace_dependency_df26(dependency.clone()) {
            WorkspaceDependency::Dependency(dep) => Some(dep.name().to_string()),
            _ => None,
        };
        let resolved25 = match tree.resolve_workspace_dependency_df25(dependency.clone()) {
            WorkspaceDependency::Dependency(dep) => Some(dep.name().to_string()),
            _ => None,
        };
        let resolved = match (resolved26, resolved25) {
            (Some(resolved26), Some(resolved25)) if resolved26 == resolved25 => {
                Cell::new(resolved26).fg(Color::DarkGreen)
            }
            (resolved26, resolved25) => Cell::new(format!(
                "{} / {}",
                resolved26.unwrap_or("(Unresolved)".into()),
                resolved25.unwrap_or("(Unresolved)".into())
            ))
            .fg(Color::DarkRed),
        };
        table.add_row(vec![
            file.into(),
            resolved,
            format!("{:#}", dependency).into(),
        ]);
    }
    println!("{table}");
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
        missing_dependency
            .dependency
            .to_string()
            .color(missing_dependency.dependency.color())
    );
    println!();
    if let Some(dep) = missing_dependency.dependency.only_one() {
        println!("Missing library dependency.",);
        println!(
            "Solution: Consider adding {} as a library dependency to {}",
            dep.name().bold(),
            missing_dependency.workspace.name().bold()
        )
    } else {
        println!("Ambiguous library dependency.");
        if let WorkspaceDependency::Dependency(df25_resolution) =
            tree.resolve_workspace_dependency_df25(missing_dependency.dependency.clone())
        {
            let other = WorkspaceDependency::Ambiguous(
                missing_dependency
                    .dependency
                    .all()
                    .into_iter()
                    .filter(|d| d.name() != df25_resolution.name())
                    .collect(),
            );
            println!("DataFlex 25: {}", df25_resolution.name());
            let indirect_dependencies =
                tree.defined_indirect_workspace_dependencies(missing_dependency.workspace);

            if other
                .all()
                .iter()
                .all(|w| indirect_dependencies.contains(&w.sws_path))
            {
                let is_multiple = other.all().len() > 1;
                if is_multiple {
                    println!(
                        "Solution: Consider removing {} as library dependencies to {}. They're already included via indirect dependencies.",
                        other.to_string().bold(),
                        missing_dependency.workspace.name().bold()
                    );
                } else {
                    println!(
                        "Solution: Consider removing {} as a library dependency to {}. It's already included via indirect dependencies.",
                        other.to_string().bold(),
                        missing_dependency.workspace.name().bold()
                    );
                }
            } else {
                println!(
                    "Solution: Consider pushing down {} in the dependency tree",
                    other.to_string().bold(),
                );
                if missing_dependency.workspace.sws_path != tree.root_workspace().sws_path {
                    println!(
                        "Alternative Solution: Consider pulling up {} in the dependency tree.",
                        df25_resolution.name().bold(),
                    );
                }
            }
        }
    }

    if Options::shared().verbose
        && let Some(source_dependencies) = tree
            .analyze_source_dependency(missing_dependency.workspace, &missing_dependency.dependency)
    {
        println!(
            "Impacted source files: {}",
            source_dependencies
                .iter()
                .filter_map(|s| s
                    .path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "Referenced files: {}",
            source_dependencies
                .iter()
                .flat_map(|s| s.dependencies.iter())
                .collect::<HashSet<_>>()
                .into_iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
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
