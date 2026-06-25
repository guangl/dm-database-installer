## Conventions

**Naming & Organization**
- `snake_case` for modules and functions (`load_config`, `validate_install_config`, `load_standalone_specific`)
- `PascalCase` for public structs/enums (`InstallConfig`, `CommonConfig`, `DwClusterConfig`, `NodeRole`)
- Enums serialize to kebab/lowercase via `#[serde(rename_all = "lowercase")]` (`src/config/dw.rs:11`)
- Inline Chinese comments for domain-specific logic (platform detection, archive modes, backup policies)

**Error Handling**
- `anyhow::Result` + `Context`/`bail!` for application code (`main.rs`, `config/mod.rs`, `install/`)
- `thiserror` for structured domain errors with `#[source]` chains (`src/ssh/error.rs`)
- Semantic validation failures use `bail!("message")`, distinct from I/O errors (`src/config/mod.rs`)

**Tests**
- Inline `#[cfg(test)] mod tests` at the bottom of the module under test, not a separate test tree
- `tempfile::NamedTempFile` for config-parsing fixtures
- Test names follow `test_<function>_<scenario>`

**Comments**
- `///` doc comments only for high-value public API context — used sparingly
- `//` inline comments for non-obvious logic (platform heuristics, archive semantics)
- `// ──` separator lines for logical sections within large config structs

**Module Organization**
- Each feature area has a `mod.rs` entry point; submodules stay private unless re-exported
- Single-file modules for focused concerns (e.g. `ssh/error.rs` is errors only)
- Const strings for file paths/keys (`CONFIG_FILE`, checkpoint file names)

**Async**
- `#[tokio::main]`; async functions return `anyhow::Result<()>`
- Trait objects (`&dyn CommandRunner`) abstract local vs. SSH execution so the same step logic runs in both standalone and cluster paths
