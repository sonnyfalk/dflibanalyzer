use std::path::PathBuf;

use clap::Parser;

mod output_json;
mod output_text;
mod workspace;
mod workspace_tree;

use workspace::*;
use workspace_tree::*;

#[derive(Debug, Parser)]
#[command(version)]
struct Options {
    sws_file: PathBuf,
    #[arg(short, long, action)]
    verbose: bool,
    /// Output structured json instead of human readable text.
    #[arg(short, long, action)]
    json: bool,
    /// Scan source files in AppSrc/DdSrc recursively.
    #[arg(short, long, action)]
    recursive_scan: bool,
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

fn main() -> Result<(), String> {
    let options = Options::init_current(Options::parse());
    let root_workspace = Workspace::new(&options.sws_file)?;

    if options.json {
        output_json::analyze_and_output_json(root_workspace);
    } else {
        output_text::analyze_and_output_text(root_workspace);
    }

    Ok(())
}
