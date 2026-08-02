# DataFlex Workspace Library Dependency Analyzer

This command line tool analyzes DataFlex source files in a workspace, and displays the implicit library dependencies.
It identifies missing or ambiguous library dependencies based on the actual `Use` statements in source files,
and suggests `.sws` file updates accordingly.

## Usage

```
Usage: dflibanalyzer.exe [OPTIONS] <SWS_FILE>

Arguments:
  <SWS_FILE>

Options:
  -v, --verbose
  -h, --help     Print help
```

## Examples

#### Example workspace with expected dependency tree

![](example1.png)

#### Example workspace with missing dependencies

![](example2.png)
