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

static CURRENT_OPTIONS: std::sync::OnceLock<Options> = std::sync::OnceLock::new();

impl Options {
    fn init_current(current: Options) -> &'static Options {
        _ = CURRENT_OPTIONS.set(current);
        Self::current()
    }

    fn current() -> &'static Options {
        CURRENT_OPTIONS.get().unwrap()
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
                    WorkspaceDependency::Reverse(_) => Some(dep),
                    WorkspaceDependency::Conflicting(_, _) => Some(dep),
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
            "{}{} {}",
            prefix,
            connector,
            match dependency {
                WorkspaceDependency::Dependency(_) =>
                    format!("{}", dependency.to_string().color(dependency.color())),
                WorkspaceDependency::Missing(_) =>
                    format!("({}: {})", "Missing".color(dependency.color()), dependency),
                WorkspaceDependency::Reverse(_) => format!(
                    "({}: {})",
                    "Reverse Use references".color(dependency.color()),
                    dependency
                ),
                WorkspaceDependency::Ambiguous(_) => format!(
                    "({}: {})",
                    "Ambiguous Use references".color(dependency.color()),
                    dependency
                ),
                WorkspaceDependency::Conflicting(_, _) => format!(
                    "({}: {})",
                    "Conflicting File Resolution".color(dependency.color()),
                    dependency
                ),
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

    for (file, dependency) in &all_duplicate_filenames {
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

    if Options::current().verbose {
        let mut table = Table::new();
        table
            .load_style(comfy_table::presets::UTF8_FULL.with_rounded_corners())
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_header(vec!["File / Workspace", "Identical", "Path"]);
        for (file, dependency) in &all_duplicate_filenames {
            let workspace_names: Vec<_> = dependency.all().iter().map(|dep| dep.name()).collect();
            let file_paths: Vec<_> = dependency
                .all()
                .iter()
                .filter_map(|dep| tree.source_file_in_workspace(file, dep))
                .collect();
            let identical = all_files_identical(&file_paths);
            table.add_row(vec![
                format!(
                    "{}\n    {}",
                    file.to_string(),
                    workspace_names.join("\n    ")
                )
                .into(),
                if identical {
                    Cell::new("Yes").fg(Color::DarkGreen)
                } else {
                    Cell::new("No")
                },
                format!(
                    "\n{}",
                    file_paths
                        .iter()
                        .map(|p| p.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
                .into(),
            ]);
        }
        println!("{table}");
    }
}

fn all_files_identical(files: &Vec<&PathBuf>) -> bool {
    let Ok(entries) = files
        .iter()
        .map(|path| std::fs::read(path))
        .collect::<std::io::Result<Vec<_>>>()
    else {
        return false;
    };

    entries.first().is_some_and(|first_content| {
        entries
            .iter()
            .skip(1)
            .all(|content| content == first_content)
    })
}

fn print_missing_dependency(
    missing_dependency: &MissingDependency,
    tree: &WorkspaceDependencyTree,
) {
    println!();
    println!("{}", missing_dependency.workspace.name());
    println!(
        "{} ({}: {})",
        "└──",
        match missing_dependency.dependency {
            WorkspaceDependency::Dependency(_) => "".color(missing_dependency.dependency.color()),
            WorkspaceDependency::Missing(_) =>
                "Missing".color(missing_dependency.dependency.color()),
            WorkspaceDependency::Reverse(_) =>
                "Reverse Use references".color(missing_dependency.dependency.color()),
            WorkspaceDependency::Ambiguous(_) =>
                "Ambiguous Use references".color(missing_dependency.dependency.color()),
            WorkspaceDependency::Conflicting(_, _) =>
                "Conflicting File Resolution".color(missing_dependency.dependency.color()),
        },
        missing_dependency.dependency
    );
    println!();
    match missing_dependency.dependency {
        WorkspaceDependency::Dependency(_) => {}
        WorkspaceDependency::Missing(dep) => {
            println!("Missing library dependency.",);
            println!(
                "Solution: Consider adding {} as a library dependency to {}",
                dep.name().bold(),
                missing_dependency.workspace.name().bold()
            )
        }
        WorkspaceDependency::Reverse(dep) => {
            println!("Reverse Use references.",);
            println!(
                "Solution: Consider moving impacted files from {} up the tree to {}",
                missing_dependency.workspace.name().bold(),
                dep.name().bold(),
            )
        }
        WorkspaceDependency::Ambiguous(_) => {
            println!("Ambiguous Use references.");
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
        WorkspaceDependency::Conflicting(df26_resolution, df25_resolution) => {
            println!("Conflicting File Resolution.");
            println!("DataFlex 25: {}", df25_resolution.name());
            println!("DataFlex 26: {}", df26_resolution.name());
            println!(
                "Solution: Consider whether it's possible to push {} down in the dependency tree",
                df26_resolution.name().bold(),
            );
            println!(
                "Alternative Solution: Manually merge and resolve the source file conflicts with the same name."
            )
        }
    }

    if let Some(source_dependencies) =
        tree.analyze_source_dependency(missing_dependency.workspace, &missing_dependency.dependency)
    {
        println!(
            "Impacted source files from {}: {}",
            missing_dependency.workspace.name().bold(),
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
            "Referenced files found in {}: {}",
            missing_dependency.dependency.to_string().bold(),
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
    let options = Options::init_current(Options::parse());

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
