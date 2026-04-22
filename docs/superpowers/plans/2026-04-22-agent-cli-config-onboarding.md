# Agent CLI Config Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a natural CLI configuration onboarding flow with first-run full-screen wizard, editable config commands, and REPL `/config`, while preserving existing runtime behavior.

**Architecture:** Implement a dedicated config subsystem in `agent-cli` (`config_model`, `config_store`, `config_resolver`, `config_wizard`, `config_commands`) and route all config entry points through it. Keep runtime path unchanged except replacing direct args usage with resolved config and adding command dispatch. Maintain precedence `CLI > ENV > FILE > INTERACTIVE` and preserve existing `--prompt`/REPL behavior.

**Tech Stack:** Rust 2021, clap subcommands, serde/serde_json, tokio, tempfile (tests), existing `agent-core` runtime path

---

## Scope Check

This plan covers one subsystem: `agent-cli` configuration UX and initialization flow. It does not change `agent-core` logic, tool execution backend, or persistence outside `agent-cli` local config file handling.

## File Structure

| File | Responsibility |
|---|---|
| `crates/agent-cli/src/args.rs` | CLI model upgrade: optional runtime args + `config` subcommands + run mode routing |
| `crates/agent-cli/src/config_model.rs` | Stored/resolved config structs, defaults, masking, validation |
| `crates/agent-cli/src/config_store.rs` | Config path resolution, load/save/backup (corrupt file recovery) |
| `crates/agent-cli/src/config_resolver.rs` | Merge precedence (`CLI > ENV > FILE > INTERACTIVE`) and missing-field detection |
| `crates/agent-cli/src/config_wizard.rs` | Full-screen interactive wizard for init/edit flows |
| `crates/agent-cli/src/config_commands.rs` | `config init/edit/show/path` command handlers |
| `crates/agent-cli/src/repl_commands.rs` | REPL slash command parsing (`/config`, `/help`) |
| `crates/agent-cli/src/repl.rs` | Integrate slash command classification with existing submit/exit semantics |
| `crates/agent-cli/src/main.rs` | Composition root: command dispatch + startup config bootstrap + runtime wiring |
| `docs/superpowers/crates-agent-handoff.md` | Document new config UX and command surface for downstream agents |

---

### Task 1: Upgrade CLI Argument Model and Run Modes

**Files:**
- Modify: `crates/agent-cli/src/args.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/args.rs`

- [ ] **Step 1: Write failing tests for subcommands and new run mode**

```rust
#[test]
fn detects_config_subcommand_mode() {
    let args = Args::parse_from(["agent-runtime", "config", "path"]);
    assert_eq!(args.run_mode(), RunMode::Command);
}

#[test]
fn parses_config_edit_subcommand() {
    let args = Args::parse_from(["agent-runtime", "config", "edit"]);
    assert!(matches!(
        args.command,
        Some(Command::Config(ConfigSubcommand::Edit))
    ));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p agent-cli args::tests::detects_config_subcommand_mode -v`  
Expected: FAIL with missing `RunMode::Command` or missing `Command` parsing.

- [ ] **Step 3: Implement optional args + subcommands model**

```rust
use clap::{Parser, Subcommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Command,
    SingleTurn,
    Repl,
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub enum ConfigSubcommand {
    Init,
    Edit,
    Show,
    Path,
}

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub enum Command {
    Config {
        #[command(subcommand)]
        action: ConfigSubcommand,
    },
}

#[derive(Parser, Debug, Clone)]
#[command(name = "agent-runtime", version)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(long, env = "AGENT_API_KEY")]
    pub api_key: Option<String>,

    #[arg(long, env = "AGENT_PROVIDER")]
    pub provider: Option<String>,

    #[arg(long, env = "AGENT_MODEL")]
    pub model: Option<String>,

    #[arg(long, env = "AGENT_BASE_URL")]
    pub base_url: Option<String>,

    #[arg(long, default_value = ".")]
    pub project_path: String,

    #[arg(long)]
    pub prompt: Option<String>,

    #[arg(long, default_value = "cli-tab")]
    pub tab_id: String,

    #[arg(long, default_value = "human")]
    pub output: String,
}

impl Args {
    pub fn run_mode(&self) -> RunMode {
        if self.command.is_some() {
            RunMode::Command
        } else if self.prompt.as_deref().is_some_and(|p| !p.trim().is_empty()) {
            RunMode::SingleTurn
        } else {
            RunMode::Repl
        }
    }
}
```

- [ ] **Step 4: Wire imports in `main.rs` for new enums**

```rust
use args::{Args, Command, ConfigSubcommand, RunMode};
```

- [ ] **Step 5: Run tests and build**

Run: `cargo test -p agent-cli args::tests -v`  
Expected: PASS.

Run: `cargo build -p agent-cli`  
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/args.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add config subcommands and command run mode"
```

---

### Task 2: Add Config Model + Store (File Path, Load/Save, Masking)

**Files:**
- Create: `crates/agent-cli/src/config_model.rs`
- Create: `crates/agent-cli/src/config_store.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/config_model.rs`
- Test: `crates/agent-cli/src/config_store.rs`

- [ ] **Step 1: Write failing tests for masking and store roundtrip**

```rust
#[test]
fn masks_api_key_in_show_output() {
    let masked = mask_secret("sk-test-secret");
    assert!(masked.starts_with("sk-"));
    assert!(masked.ends_with("et"));
    assert!(!masked.contains("test-secret"));
}

#[test]
fn store_roundtrip_load_save() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
    let path = dir.path().join("config.json");
    let cfg = StoredConfig {
        provider: Some("minimax".to_string()),
        model: Some("MiniMax-M1".to_string()),
        api_key: Some("k".to_string()),
        base_url: Some("https://api.minimax.chat/v1".to_string()),
        output: Some("human".to_string()),
    };

    save_config_atomic(&path, &cfg).unwrap_or_else(|e| panic!("save: {e}"));
    let loaded = load_config(&path).unwrap_or_else(|e| panic!("load: {e}"));
    assert_eq!(loaded, Some(cfg));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli config_store::tests::store_roundtrip_load_save -v`  
Expected: FAIL because modules/functions are missing.

- [ ] **Step 3: Implement config model**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StoredConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub output: String,
}

pub fn default_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "minimax" => Some("https://api.minimax.chat/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        _ => None,
    }
}

pub fn mask_secret(raw: &str) -> String {
    if raw.len() <= 5 {
        "*****".to_string()
    } else {
        format!("{}***{}", &raw[..3], &raw[raw.len() - 2..])
    }
}
```

- [ ] **Step 4: Implement config store path + atomic save + corrupt backup**

```rust
use std::fs;
use std::path::{Path, PathBuf};

use crate::config_model::StoredConfig;

pub fn default_config_path() -> Result<PathBuf, String> {
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(base).join("claude-prism/agent-cli/config.json"));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| "HOME is not set; cannot resolve config path".to_string())?;
    Ok(PathBuf::from(home).join(".config/claude-prism/agent-cli/config.json"))
}

pub fn load_config(path: &Path) -> Result<Option<StoredConfig>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|e| format!("read config failed: {e}"))?;
    match serde_json::from_str::<StoredConfig>(&content) {
        Ok(cfg) => Ok(Some(cfg)),
        Err(err) => {
            backup_corrupt_file(path)?;
            Err(format!("config parse failed: {err}"))
        }
    }
}

pub fn save_config_atomic(path: &Path, cfg: &StoredConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config dir failed: {e}"))?;
    }
    let tmp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(cfg).map_err(|e| format!("encode config failed: {e}"))?;
    fs::write(&tmp_path, bytes).map_err(|e| format!("write temp config failed: {e}"))?;
    fs::rename(&tmp_path, path).map_err(|e| format!("commit config failed: {e}"))
}

fn backup_corrupt_file(path: &Path) -> Result<(), String> {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let backup = path.with_extension(format!("json.bak.{ts}"));
    std::fs::copy(path, backup).map_err(|e| format!("backup corrupt config failed: {e}"))?;
    Ok(())
}
```

- [ ] **Step 5: Register modules in `main.rs`**

```rust
mod config_model;
mod config_store;
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p agent-cli config_model::tests -v`  
Expected: PASS.

Run: `cargo test -p agent-cli config_store::tests -v`  
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/agent-cli/src/config_model.rs crates/agent-cli/src/config_store.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add local config model and atomic file store"
```

---

### Task 3: Add Resolver for `CLI > ENV > FILE > INTERACTIVE`

**Files:**
- Create: `crates/agent-cli/src/config_resolver.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/config_resolver.rs`

- [ ] **Step 1: Write failing precedence test**

```rust
#[test]
fn cli_overrides_env_and_file() {
    let cli = RawConfig {
        provider: Some("deepseek".to_string()),
        model: None,
        api_key: None,
        base_url: None,
        output: None,
    };
    let env = RawConfig {
        provider: Some("minimax".to_string()),
        model: Some("MiniMax-M1".to_string()),
        api_key: Some("env-key".to_string()),
        base_url: None,
        output: None,
    };
    let file = RawConfig {
        provider: Some("minimax".to_string()),
        model: Some("file-model".to_string()),
        api_key: Some("file-key".to_string()),
        base_url: None,
        output: Some("jsonl".to_string()),
    };

    let merged = merge_sources(&cli, &env, &file);
    assert_eq!(merged.provider.as_deref(), Some("deepseek"));
    assert_eq!(merged.model.as_deref(), Some("MiniMax-M1"));
    assert_eq!(merged.api_key.as_deref(), Some("env-key"));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p agent-cli config_resolver::tests::cli_overrides_env_and_file -v`  
Expected: FAIL because resolver module is missing.

- [ ] **Step 3: Implement resolver and missing-field detection**

```rust
use crate::config_model::{default_base_url, ResolvedConfig, StoredConfig};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingField {
    Provider,
    Model,
    ApiKey,
}

pub fn merge_sources(cli: &RawConfig, env: &RawConfig, file: &RawConfig) -> RawConfig {
    RawConfig {
        provider: cli.provider.clone().or_else(|| env.provider.clone()).or_else(|| file.provider.clone()),
        model: cli.model.clone().or_else(|| env.model.clone()).or_else(|| file.model.clone()),
        api_key: cli.api_key.clone().or_else(|| env.api_key.clone()).or_else(|| file.api_key.clone()),
        base_url: cli.base_url.clone().or_else(|| env.base_url.clone()).or_else(|| file.base_url.clone()),
        output: cli.output.clone().or_else(|| env.output.clone()).or_else(|| file.output.clone()),
    }
}

pub fn detect_missing(raw: &RawConfig) -> Vec<MissingField> {
    let mut missing = Vec::new();
    if raw.provider.as_deref().is_none_or(|v| v.trim().is_empty()) {
        missing.push(MissingField::Provider);
    }
    if raw.model.as_deref().is_none_or(|v| v.trim().is_empty()) {
        missing.push(MissingField::Model);
    }
    if raw.api_key.as_deref().is_none_or(|v| v.trim().is_empty()) {
        missing.push(MissingField::ApiKey);
    }
    missing
}

pub fn finalize(raw: RawConfig) -> Result<ResolvedConfig, String> {
    let provider = raw.provider.ok_or_else(|| "provider is required".to_string())?;
    let model = raw.model.ok_or_else(|| "model is required".to_string())?;
    let api_key = raw.api_key.ok_or_else(|| "api_key is required".to_string())?;
    let base_url = raw.base_url.or_else(|| default_base_url(provider.trim()).map(|v| v.to_string()))
        .ok_or_else(|| format!("unsupported provider '{}'", provider))?;
    let output = raw.output.unwrap_or_else(|| "human".to_string());

    Ok(ResolvedConfig { provider, model, api_key, base_url, output })
}

pub fn file_to_raw(file: Option<StoredConfig>) -> RawConfig {
    if let Some(cfg) = file {
        RawConfig {
            provider: cfg.provider,
            model: cfg.model,
            api_key: cfg.api_key,
            base_url: cfg.base_url,
            output: cfg.output,
        }
    } else {
        RawConfig::default()
    }
}
```

- [ ] **Step 4: Register resolver module in `main.rs`**

```rust
mod config_resolver;
```

- [ ] **Step 5: Run resolver tests**

Run: `cargo test -p agent-cli config_resolver::tests -v`  
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/config_resolver.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add config source resolver with precedence and validation"
```

---

### Task 4: Implement Full-Screen Config Wizard

**Files:**
- Create: `crates/agent-cli/src/config_wizard.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/config_wizard.rs`

- [ ] **Step 1: Write failing wizard transcript test**

```rust
#[test]
fn wizard_collects_required_fields_in_order() {
    let mut io = FakeWizardIo::new(vec![
        "1".to_string(),
        "MiniMax-M1".to_string(),
        "sk-test".to_string(),
        "".to_string(),
        "human".to_string(),
        "save".to_string(),
    ]);

    let cfg = run_wizard(&mut io, None).unwrap_or_else(|e| panic!("wizard failed: {e}"));
    assert_eq!(cfg.provider.as_deref(), Some("minimax"));
    assert_eq!(cfg.model.as_deref(), Some("MiniMax-M1"));
    assert_eq!(cfg.api_key.as_deref(), Some("sk-test"));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p agent-cli config_wizard::tests::wizard_collects_required_fields_in_order -v`  
Expected: FAIL because module is missing.

- [ ] **Step 3: Implement wizard IO and run loop**

```rust
use crate::config_model::{default_base_url, StoredConfig};

pub trait WizardIo {
    fn print_line(&mut self, line: &str) -> Result<(), String>;
    fn read_line(&mut self, prompt: &str) -> Result<String, String>;
}

pub fn run_wizard(io: &mut dyn WizardIo, existing: Option<&StoredConfig>) -> Result<StoredConfig, String> {
    io.print_line("=== Agent Runtime Setup Wizard ===")?;

    let provider = ask_provider(io, existing.and_then(|c| c.provider.as_deref()))?;
    let default_model = if provider == "minimax" { "MiniMax-M1" } else { "deepseek-chat" };
    let model = ask_text(io, "Model", existing.and_then(|c| c.model.as_deref()).unwrap_or(default_model))?;
    let api_key = ask_text(io, "API Key", existing.and_then(|c| c.api_key.as_deref()).unwrap_or(""))?;
    let base_url_default = default_base_url(&provider).unwrap_or("");
    let base_url = ask_text(io, "Base URL", existing.and_then(|c| c.base_url.as_deref()).unwrap_or(base_url_default))?;
    let output = ask_text(io, "Output (human/jsonl)", existing.and_then(|c| c.output.as_deref()).unwrap_or("human"))?;

    io.print_line("Type 'save' to persist config, or 'cancel' to abort")?;
    let confirm = io.read_line("Action")?;
    if confirm.trim().eq_ignore_ascii_case("save") {
        Ok(StoredConfig {
            provider: Some(provider),
            model: Some(model),
            api_key: Some(api_key),
            base_url: Some(base_url),
            output: Some(output),
        })
    } else {
        Err("wizard cancelled".to_string())
    }
}
```

- [ ] **Step 4: Register module in `main.rs`**

```rust
mod config_wizard;
```

- [ ] **Step 5: Run wizard tests**

Run: `cargo test -p agent-cli config_wizard::tests -v`  
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/config_wizard.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add interactive config wizard for init and edit flows"
```

---

### Task 5: Add Config Command Handlers (`init/edit/show/path`)

**Files:**
- Create: `crates/agent-cli/src/config_commands.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/config_commands.rs`

- [ ] **Step 1: Write failing tests for `show` mask and `path` output**

```rust
#[test]
fn show_masks_api_key() {
    let cfg = StoredConfig {
        provider: Some("minimax".to_string()),
        model: Some("MiniMax-M1".to_string()),
        api_key: Some("sk-very-secret".to_string()),
        base_url: Some("https://api.minimax.chat/v1".to_string()),
        output: Some("human".to_string()),
    };
    let shown = render_show(&cfg);
    assert!(shown.contains("sk-***et"));
    assert!(!shown.contains("very-secret"));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p agent-cli config_commands::tests::show_masks_api_key -v`  
Expected: FAIL because command module is missing.

- [ ] **Step 3: Implement command handlers**

```rust
use crate::args::ConfigSubcommand;
use crate::config_model::{mask_secret, StoredConfig};
use crate::config_store::{default_config_path, load_config, save_config_atomic};
use crate::config_wizard::{run_wizard, WizardIo};

pub fn execute_config_command(
    action: &ConfigSubcommand,
    io: &mut dyn WizardIo,
) -> Result<String, String> {
    let path = default_config_path()?;

    match action {
        ConfigSubcommand::Path => Ok(path.display().to_string()),
        ConfigSubcommand::Show => {
            let cfg = load_config(&path)?.unwrap_or_default();
            Ok(render_show(&cfg))
        }
        ConfigSubcommand::Init => {
            let cfg = run_wizard(io, None)?;
            save_config_atomic(&path, &cfg)?;
            Ok(format!("Config saved: {}", path.display()))
        }
        ConfigSubcommand::Edit => {
            let existing = load_config(&path)?;
            let cfg = run_wizard(io, existing.as_ref())?;
            save_config_atomic(&path, &cfg)?;
            Ok(format!("Config updated: {}", path.display()))
        }
    }
}

pub fn render_show(cfg: &StoredConfig) -> String {
    let api = cfg.api_key.as_deref().map(mask_secret).unwrap_or_else(|| "<unset>".to_string());
    format!(
        "provider: {}\nmodel: {}\napi_key: {}\nbase_url: {}\noutput: {}",
        cfg.provider.as_deref().unwrap_or("<unset>"),
        cfg.model.as_deref().unwrap_or("<unset>"),
        api,
        cfg.base_url.as_deref().unwrap_or("<unset>"),
        cfg.output.as_deref().unwrap_or("human")
    )
}
```

- [ ] **Step 4: Wire subcommand dispatch in `main.rs`**

```rust
if let RunMode::Command = args.run_mode() {
    if let Some(Command::Config { action }) = &args.command {
        let mut io = config_wizard::StdioWizardIo::default();
        match config_commands::execute_config_command(action, &mut io) {
            Ok(msg) => {
                println!("{}", msg);
                return ExitCode::SUCCESS;
            }
            Err(err) => {
                eprintln!("agent-runtime error: {}", err);
                return ExitCode::FAILURE;
            }
        }
    }
}
```

- [ ] **Step 5: Run config command tests**

Run: `cargo test -p agent-cli config_commands::tests -v`  
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/config_commands.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add config init/edit/show/path command handlers"
```

---

### Task 6: Add REPL Slash Command Parsing and `/config`

**Files:**
- Create: `crates/agent-cli/src/repl_commands.rs`
- Modify: `crates/agent-cli/src/repl.rs`
- Modify: `crates/agent-cli/src/main.rs`
- Test: `crates/agent-cli/src/repl_commands.rs`
- Test: `crates/agent-cli/src/repl.rs`

- [ ] **Step 1: Write failing tests for slash parsing**

```rust
#[test]
fn parses_config_command() {
    assert_eq!(parse_repl_command("/config"), ReplCommand::Config);
}

#[test]
fn parses_help_command() {
    assert_eq!(parse_repl_command("/help"), ReplCommand::Help);
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p agent-cli repl_commands::tests::parses_config_command -v`  
Expected: FAIL because module is missing.

- [ ] **Step 3: Implement `repl_commands.rs`**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplCommand {
    Config,
    Help,
    Unknown(String),
    None,
}

pub fn parse_repl_command(input: &str) -> ReplCommand {
    match input.trim() {
        "/config" => ReplCommand::Config,
        "/help" => ReplCommand::Help,
        cmd if cmd.starts_with('/') => ReplCommand::Unknown(cmd.to_string()),
        _ => ReplCommand::None,
    }
}
```

- [ ] **Step 4: Route `/config` in REPL runtime loop**

```rust
match repl_commands::parse_repl_command(&prompt) {
    repl_commands::ReplCommand::Config => {
        let mut io = config_wizard::StdioWizardIo::default();
        let _ = config_commands::execute_config_command(&ConfigSubcommand::Edit, &mut io);
        return Ok(());
    }
    repl_commands::ReplCommand::Help => {
        println!("Commands: /config, /help, exit, quit");
        return Ok(());
    }
    repl_commands::ReplCommand::Unknown(cmd) => {
        println!("Unknown command: {}", cmd);
        return Ok(());
    }
    repl_commands::ReplCommand::None => {}
}
```

- [ ] **Step 5: Run REPL tests**

Run: `cargo test -p agent-cli repl_commands::tests -v`  
Expected: PASS.

Run: `cargo test -p agent-cli repl::tests -v`  
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/repl_commands.rs crates/agent-cli/src/repl.rs crates/agent-cli/src/main.rs
git commit -m "feat(agent-cli): add repl slash commands with /config and /help"
```

---

### Task 7: Bootstrap Startup with Resolver + Wizard + Runtime Wiring

**Files:**
- Modify: `crates/agent-cli/src/main.rs`
- Modify: `crates/agent-cli/src/turn_runner.rs`
- Test: `crates/agent-cli/src/main.rs`

- [ ] **Step 1: Write failing test for startup missing-config path**

```rust
#[test]
fn startup_requests_wizard_when_required_fields_are_missing() {
    let merged = config_resolver::RawConfig::default();
    let missing = config_resolver::detect_missing(&merged);
    assert_eq!(missing.len(), 3);
}
```

- [ ] **Step 2: Run test to verify failure baseline**

Run: `cargo test -p agent-cli tests::startup_requests_wizard_when_required_fields_are_missing -v`  
Expected: FAIL before bootstrap refactor.

- [ ] **Step 3: Implement unified startup bootstrap in `main.rs`**

```rust
let file_path = config_store::default_config_path()?;
let file_cfg = match config_store::load_config(&file_path) {
    Ok(value) => value,
    Err(err) => {
        eprintln!("config warning: {}", err);
        None
    }
};

let cli_raw = config_resolver::RawConfig {
    provider: args.provider.clone(),
    model: args.model.clone(),
    api_key: args.api_key.clone(),
    base_url: args.base_url.clone(),
    output: Some(args.output.clone()),
};
let env_raw = config_resolver::RawConfig {
    provider: std::env::var("AGENT_PROVIDER").ok(),
    model: std::env::var("AGENT_MODEL").ok(),
    api_key: std::env::var("AGENT_API_KEY").ok(),
    base_url: std::env::var("AGENT_BASE_URL").ok(),
    output: std::env::var("AGENT_OUTPUT").ok(),
};
let file_raw = config_resolver::file_to_raw(file_cfg.clone());

let mut merged = config_resolver::merge_sources(&cli_raw, &env_raw, &file_raw);
if !config_resolver::detect_missing(&merged).is_empty() {
    let mut io = config_wizard::StdioWizardIo::default();
    let wizard_cfg = config_wizard::run_wizard(&mut io, file_cfg.as_ref())?;
    config_store::save_config_atomic(&file_path, &wizard_cfg)?;
    merged = config_resolver::merge_sources(&cli_raw, &env_raw, &config_resolver::file_to_raw(Some(wizard_cfg)));
}

let resolved = config_resolver::finalize(merged)?;
```

- [ ] **Step 4: Build runtime config from resolved data**

```rust
let mut runtime = AgentRuntimeConfig::default_local_agent();
runtime.provider = resolved.provider.clone();
runtime.model = resolved.model.clone();
runtime.api_key = Some(resolved.api_key.clone());
runtime.base_url = resolved.base_url.clone();

let output_mode = args::parse_output_mode(&resolved.output)?;
```

- [ ] **Step 5: Run full test + build + clippy**

Run: `cargo test -p agent-cli -v`  
Expected: PASS.

Run: `cargo build -p agent-cli`  
Expected: PASS.

Run: `cargo clippy -p agent-cli -- -D warnings`  
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/agent-cli/src/main.rs crates/agent-cli/src/turn_runner.rs
git commit -m "feat(agent-cli): bootstrap runtime config via resolver and first-run wizard"
```

---

### Task 8: Document Operator UX and Agent Handoff

**Files:**
- Modify: `docs/superpowers/crates-agent-handoff.md`
- Test: n/a (docs)

- [ ] **Step 1: Add command UX section**

```md
## agent-cli config onboarding

- First run: auto full-screen wizard when required config is missing
- Precedence: CLI > ENV > FILE > INTERACTIVE
- Commands:
  - `agent-runtime config init`
  - `agent-runtime config edit`
  - `agent-runtime config show`
  - `agent-runtime config path`
- REPL:
  - `/config` opens edit wizard
  - `/help` prints available commands
```

- [ ] **Step 2: Commit docs**

```bash
git add docs/superpowers/crates-agent-handoff.md
git commit -m "docs(agent-cli): document config onboarding and command UX"
```

---

## Final Verification Gate

Run:

```bash
cargo build -p agent-cli
cargo test -p agent-cli -v
cargo clippy -p agent-cli -- -D warnings
```

Expected:
- all commands PASS.

---

## Self-Review

### 1. Spec coverage

- 架构：Task 2/3/4/5/6/7 覆盖。
- 命令面：Task 1 + Task 5 + Task 6 覆盖。
- 数据流：Task 3 + Task 7 覆盖。
- 错误处理：Task 2（损坏备份）+ Task 5/7（路径/解析/缺失处理）覆盖。
- 测试面：每个新增模块都有单元测试；Task 7 包含全量验证门。

### 2. Placeholder scan

- 无 `TODO/TBD/implement later`。
- 每个代码步骤含完整代码块。
- 每个测试步骤含明确命令与预期。

### 3. Type consistency

- `RunMode::Command`、`Command::Config`、`ConfigSubcommand` 在 Task 1 定义并在 Task 5/7 使用。
- `StoredConfig`/`ResolvedConfig`/`RawConfig` 在 Task 2/3 定义并在 Task 7 使用。
- `run_wizard`、`execute_config_command`、`parse_repl_command` 命名在各任务中保持一致。
