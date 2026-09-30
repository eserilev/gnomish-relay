//! `config.toml`: the ceiling for every message from the game (SPEC.md 6.6.2, 12).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use protocol::folder::resolve_folder;
use protocol::policy::{Level, effective_level};
use protocol::record::is_valid_id;
use serde::{Deserialize, Serialize};

use crate::agent_wall::AgentNetwork;
use crate::allow::{self, AllowFile, AllowTable};
use crate::allow_hosts::{Defaults, HostList, check_host_name};
use crate::ci_checks::CiChecks;
use crate::claude;
use crate::folder_path::path_bytes;
use crate::model::{ModelChoice, ModelSpec};
use crate::model_local::{self, LocalModel};
use crate::relay::Folders;
use crate::{harness_args, harness_presets};

pub const FILE: &str = "config.toml";
const MAX_FILE: u64 = 64 * 1024;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    #[default]
    Ask,
    AutoEdit,
    FullAuto,
}

impl Permission {
    /// An unknown level from the game counts as the strictest one.
    pub fn from_game(word: &str) -> Permission {
        match word {
            "auto-edit" => Permission::AutoEdit,
            "full-auto" => Permission::FullAuto,
            _ => Permission::Ask,
        }
    }

    /// The word of the config and of the game.
    pub fn word(self) -> &'static str {
        match self {
            Permission::Ask => "ask",
            Permission::AutoEdit => "auto-edit",
            Permission::FullAuto => "full-auto",
        }
    }

    /// What the level lets an agent do in a chat from the game (SPEC.md 9.3).
    pub fn meaning(self) -> &'static str {
        match self {
            Permission::Ask => "It asks in the game before each edit and each command.",
            Permission::AutoEdit => {
                "It edits files in the chat folder without asking, and asks in the game before each command."
            }
            Permission::FullAuto => "It edits files and runs commands without asking.",
        }
    }

    fn level(self) -> Level {
        match self {
            Permission::Ask => Level::Ask,
            Permission::AutoEdit => Level::AutoEdit,
            Permission::FullAuto => Level::FullAuto,
        }
    }

    fn from_level(level: Level) -> Permission {
        match level {
            Level::Ask => Permission::Ask,
            Level::AutoEdit => Permission::AutoEdit,
            Level::FullAuto => Permission::FullAuto,
        }
    }

    /// The game can lower the level of the config, never raise it (S6).
    #[must_use]
    pub fn ceiling(self, requested: Option<Permission>) -> Permission {
        match requested {
            Some(requested) => {
                Permission::from_level(effective_level(self.level(), requested.level()))
            }
            None => self,
        }
    }
}

/// What a message from the game can reach.
pub struct Policy {
    pub folders: Folders,
    pub agents: BTreeMap<String, Permission>,
    pub default_agent: String,
}

pub struct Config {
    /// The game folder that holds `Interface`, `Screenshots`, and `WTF`.
    pub wow: PathBuf,
    /// A player with only Timeways has no relay part (SPEC.md 9.7, decision 15).
    pub relay: Option<RelayConfig>,
    /// With no `[story]`, Timeways answers each message with a fixed error.
    pub story: Option<StoryConfig>,
}

impl Config {
    pub fn require_relay(&self) -> Result<&RelayConfig> {
        self.relay
            .as_ref()
            .context("coding agents are off. To turn them on, run gnomish-relay setup --relay")
    }
}

/// The coding agents of the relay and the ceiling of each message to them.
pub struct RelayConfig {
    pub policy: Policy,
    pub agents: BTreeMap<String, AgentSpec>,
    pub timeout: Duration,
    pub permission_timeout: Duration,
    /// Messages over this limit wait for their turn (SPEC.md 8.2).
    pub max_parallel_runs: usize,
    /// At this cost in a UTC day, no new run starts (SPEC.md 9.10).
    pub daily_cost_cap_usd: Option<f64>,
    /// Commands that run from the game with no question (SPEC.md 12).
    pub allow: AllowTable,
    /// The hosts that commands reach through the proxy of the sandbox (SPEC.md 6.6.4).
    pub hosts: HostList,
    /// The ports of this computer that the agent and its commands reach.
    pub local_ports: Vec<u16>,
    /// Which hosts the agent process reaches through its proxy (SPEC.md 6.6.4).
    pub agent_network: AgentNetwork,
    /// `[git] ci_checks`: the CI checks of a chat branch through `gh` (SPEC.md 9.11).
    pub ci_checks: CiChecks,
}

/// The story program of Timeways (SPEC.md 9.8).
#[derive(Debug, PartialEq, Eq)]
pub struct StoryConfig {
    /// Setup cannot know it before Timeways ships its program, so it can be missing.
    pub program: Option<StoryProgram>,
    /// The longest wait for the reply to one message.
    pub timeout: Duration,
    pub model: ModelSpec,
    /// `[sandbox] agent_network` of the relay, for the `claude` model calls.
    pub agent_network: AgentNetwork,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StoryProgram {
    /// An absolute path, never a name to look up on `PATH`.
    pub program: PathBuf,
    /// The `SQLite` file of the lore. The story program gets it as its first argument.
    pub lore_pack: PathBuf,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Any agent that speaks the Agent Client Protocol.
    Acp,
    /// Claude Code with no adapter, through `claude -p` (SPEC.md 9.2).
    Claude,
    /// Codex with no adapter, through `codex app-server` (SPEC.md 9.2).
    Codex,
    /// Answers with the message. It tests the path through the game with no agent.
    Echo,
    /// A harness with only a command line, inside the sandbox (SPEC.md 9.2).
    Command,
}

impl Kind {
    /// The word of the config.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Acp => "acp",
            Kind::Claude => "claude",
            Kind::Codex => "codex",
            Kind::Echo => "echo",
            Kind::Command => "command",
        }
    }
}

/// How to start one agent. A new ACP agent is one `[agents.<name>]` entry.
#[derive(Debug, PartialEq, Eq)]
pub struct AgentSpec {
    pub kind: Kind,
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub modes: BTreeMap<Permission, String>,
    /// The hosts of the agent in `strict` mode, besides the model hosts (SPEC.md 6.6.4).
    pub agent_hosts: Vec<String>,
    /// `command`: the arguments when the chat goes on (SPEC.md 9.2).
    pub resume: Vec<String>,
    /// `command`: more arguments at `ask`, from a preset.
    pub ask_args: Vec<String>,
}

/// Only the keys that the bridge uses. Any other key is an error, so a typo never
/// leaves a wider default in place.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    allowed_roots: Option<Vec<String>>,
    default_cwd: Option<String>,
    default_agent: Option<String>,
    timeout_minutes: Option<u64>,
    permission_timeout_minutes: Option<u64>,
    max_parallel_runs: Option<usize>,
    daily_cost_cap_usd: Option<f64>,
    wow: Wow,
    agents: Option<BTreeMap<String, Agent>>,
    allow: Option<AllowFile>,
    sandbox: Option<SandboxFile>,
    git: Option<GitFile>,
    story: Option<Story>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GitFile {
    /// Off by default: the only network call of the bridge with a login of the user.
    #[serde(default)]
    ci_checks: bool,
}

fn ci_checks(git: Option<&GitFile>) -> CiChecks {
    match git {
        Some(GitFile { ci_checks: true }) => CiChecks::On {
            program: PathBuf::from("gh"),
        },
        _ => CiChecks::Off,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SandboxFile {
    #[serde(default)]
    allow_hosts: Vec<String>,
    /// `false` leaves only `allow_hosts`. No host at all turns the proxy off.
    #[serde(default = "keep_defaults")]
    default_hosts: bool,
    #[serde(default)]
    local_ports: Vec<u16>,
    #[serde(default)]
    agent_network: AgentNetwork,
}

fn keep_defaults() -> bool {
    true
}

/// Docker over TCP and the debug port of a browser run any code, so no config opens
/// them. The forwarder inside each sandbox holds `INNER_PORT`.
const CLOSED_PORTS: [u16; 5] = [0, 2375, 2376, 9222, crate::forward::INNER_PORT];

fn local_ports(file: Option<&SandboxFile>) -> Result<Vec<u16>> {
    let mut ports = file.map(|f| f.local_ports.clone()).unwrap_or_default();
    if let Some(port) = ports.iter().find(|p| CLOSED_PORTS.contains(p)) {
        bail!(
            "[sandbox] local_ports: port {port} stays closed: 2375 and 2376 (Docker) and 9222 (a browser debugger) run any code, and the sandbox uses 3128 itself"
        );
    }
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

fn hosts(file: Option<&SandboxFile>) -> Result<HostList> {
    let (defaults, more) = match file {
        Some(file) if !file.default_hosts => (Defaults::Off, file.allow_hosts.as_slice()),
        Some(file) => (Defaults::Keep, file.allow_hosts.as_slice()),
        None => (Defaults::Keep, [].as_slice()),
    };
    HostList::new(defaults, more).map_err(|e| anyhow::anyhow!("[sandbox] allow_hosts: {e}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wow {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Story {
    program: Option<String>,
    lore_pack: Option<String>,
    timeout_seconds: Option<u64>,
    model: Option<ModelName>,
    claude_model: Option<String>,
    local_url: Option<String>,
    local_model: Option<String>,
    model_timeout_seconds: Option<u64>,
    budget_window_minutes: Option<u64>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum ModelName {
    Claude,
    Local,
}

const DEFAULT_STORY_SECONDS: u64 = 120;
const MAX_STORY_SECONDS: u64 = 600;
/// A model call belongs to a batch that waits 120 seconds, so it ends first.
const DEFAULT_MODEL_SECONDS: u64 = 60;
/// 10 model calls in any 20 minutes: about 30 in an hour.
const DEFAULT_BUDGET_MINUTES: u64 = 20;
const MAX_BUDGET_MINUTES: u64 = 1440;
const MAX_MODEL_NAME: usize = 200;

fn story(file: Option<&Story>, network: AgentNetwork, home: &Path) -> Result<Option<StoryConfig>> {
    let Some(story) = file else {
        return Ok(None);
    };
    let seconds = story.timeout_seconds.unwrap_or(DEFAULT_STORY_SECONDS);
    if !(1..=MAX_STORY_SECONDS).contains(&seconds) {
        bail!("[story] timeout_seconds must be 1 to {MAX_STORY_SECONDS}");
    }
    Ok(Some(StoryConfig {
        program: story_program(story, home)?,
        timeout: Duration::from_secs(seconds),
        model: model_spec(story)?,
        agent_network: network,
    }))
}

fn story_program(story: &Story, home: &Path) -> Result<Option<StoryProgram>> {
    let (program, lore_pack) = match (&story.program, &story.lore_pack) {
        (None, None) => return Ok(None),
        (Some(program), Some(lore_pack)) => (program, lore_pack),
        _ => bail!("[story] program and lore_pack go together"),
    };
    Ok(Some(StoryProgram {
        program: expand(program, home).context("[story] program")?,
        lore_pack: expand(lore_pack, home).context("[story] lore_pack")?,
    }))
}

fn model_spec(story: &Story) -> Result<ModelSpec> {
    let seconds = story.model_timeout_seconds.unwrap_or(DEFAULT_MODEL_SECONDS);
    if !(1..=MAX_STORY_SECONDS).contains(&seconds) {
        bail!("[story] model_timeout_seconds must be 1 to {MAX_STORY_SECONDS}");
    }
    let minutes = story
        .budget_window_minutes
        .unwrap_or(DEFAULT_BUDGET_MINUTES);
    let Some(minutes) = u32::try_from(minutes)
        .ok()
        .filter(|m| (1..=MAX_BUDGET_MINUTES).contains(&u64::from(*m)))
    else {
        bail!("[story] budget_window_minutes must be 1 to {MAX_BUDGET_MINUTES}");
    };
    Ok(ModelSpec {
        choice: model_choice(story)?,
        timeout: Duration::from_secs(seconds),
        budget_window_minutes: minutes,
    })
}

/// The keys of one model are an error with the other model, so a typo never leaves a
/// model that the user did not mean.
fn model_choice(story: &Story) -> Result<ModelChoice> {
    let claude_keys = story.claude_model.is_some();
    let local_keys = story.local_url.is_some() || story.local_model.is_some();
    match story.model {
        None if claude_keys || local_keys => bail!("[story] a model key needs `model`"),
        None => Ok(ModelChoice::None),
        Some(ModelName::Claude) if local_keys => {
            bail!("[story] local_url and local_model need model = \"local\"")
        }
        Some(ModelName::Claude) => Ok(ModelChoice::Claude {
            command: vec!["claude".into()],
            model: story
                .claude_model
                .as_deref()
                .map(model_name)
                .transpose()?
                .map(str::to_owned),
        }),
        Some(ModelName::Local) if claude_keys => {
            bail!("[story] claude_model needs model = \"claude\"")
        }
        Some(ModelName::Local) => {
            let url = story
                .local_url
                .as_deref()
                .context("[story] local_url is missing")?;
            let model = story
                .local_model
                .as_deref()
                .context("[story] local_model is missing")?;
            Ok(ModelChoice::Local(LocalModel {
                url: model_local::check_url(url).with_context(|| {
                    format!("[story] local_url must be http://127.0.0.1:<port> or http://[::1]:<port>, not {url}")
                })?,
                model: model_name(model)?.to_owned(),
            }))
        }
    }
}

pub fn is_model_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_MODEL_NAME
        && !name.starts_with('-')
        && !name.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// A model name goes into an argument of `claude` or into a JSON body. One that starts
/// with `-` would read as a flag.
fn model_name(name: &str) -> Result<&str> {
    if !is_model_name(name) {
        bail!(
            "[story] a model name must be 1 to {MAX_MODEL_NAME} bytes with no space, and must not start with -"
        );
    }
    Ok(name)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Agent {
    kind: Kind,
    permission: Permission,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    modes: BTreeMap<Permission, String>,
    /// The hosts of the agent in `strict` mode, besides the model hosts of its kind.
    #[serde(default, rename = "agent_hosts")]
    hosts: Vec<String>,
    /// `command`: a harness that the bridge knows (`harness_presets.rs`).
    preset: Option<String>,
    #[serde(default)]
    resume: Vec<String>,
}

const DEFAULT_TIMEOUT_MINUTES: u64 = 30;
const MAX_TIMEOUT_MINUTES: u64 = 240;
const DEFAULT_PERMISSION_MINUTES: u64 = 10;
pub const DEFAULT_MAX_PARALLEL_RUNS: usize = 3;
const MAX_PARALLEL_RUNS: usize = 16;
const MAX_COST_CAP: f64 = 10_000.0;
const MAX_PERMISSION_MINUTES: u64 = 60;

fn minutes(value: Option<u64>, default: u64, max: u64, key: &str) -> Result<Duration> {
    let minutes = value.unwrap_or(default);
    if !(1..=max).contains(&minutes) {
        bail!("{key} must be 1 to {max}");
    }
    Ok(Duration::from_mins(minutes))
}

/// A cap above 0 and at most `MAX_COST_CAP` dollars. A NaN fails the range too.
fn cost_cap(cap: Option<f64>) -> Result<Option<f64>> {
    let Some(cap) = cap else {
        return Ok(None);
    };
    if !(cap > 0.0 && cap <= MAX_COST_CAP) {
        bail!("daily_cost_cap_usd must be above 0 and at most {MAX_COST_CAP}");
    }
    Ok(Some(cap))
}

fn is_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

fn check_agent(name: &str, agent: &Agent) -> Result<()> {
    if !is_valid_id(name.as_bytes()) {
        bail!("agent name {name:?} is not a valid id");
    }
    if agent.kind != Kind::Command && (agent.preset.is_some() || !agent.resume.is_empty()) {
        bail!("[agents.{name}] preset and resume are only for kind command");
    }
    match agent.kind {
        Kind::Acp | Kind::Claude | Kind::Codex
            if agent.command.first().is_none_or(String::is_empty) =>
        {
            bail!("[agents.{name}] needs a command")
        }
        Kind::Echo if !agent.command.is_empty() => {
            bail!("[agents.{name}] is kind echo, so it has no command")
        }
        _ => {}
    }
    if let Some(bad) = agent.env.iter().find(|n| !is_env_name(n)) {
        bail!("[agents.{name}] env name {bad:?} is not A-Z, 0-9, and _");
    }
    for host in &agent.hosts {
        check_host_name(host).map_err(|e| anyhow::anyhow!("[agents.{name}] agent_hosts: {e}"))?;
    }
    if agent.kind == Kind::Claude {
        check_claude_modes(name, &agent.modes)?;
    }
    // Codex has sandboxes and approval policies, not modes. The level picks them.
    if agent.kind == Kind::Codex && !agent.modes.is_empty() {
        bail!("[agents.{name}] is kind codex, so it has no modes");
    }
    if agent.kind == Kind::Command {
        check_command_agent(name, agent)?;
    }
    Ok(())
}

/// A harness with no permission channel has no modes: the level picks its walls.
fn check_command_agent(name: &str, agent: &Agent) -> Result<()> {
    if !agent.modes.is_empty() {
        bail!("[agents.{name}] is kind command, so it has no modes");
    }
    let spec = command_spec(agent).map_err(|e| anyhow::anyhow!("[agents.{name}] {e}"))?;
    harness_args::check_template(&spec.command).map_err(|e| anyhow::anyhow!("[agents.{name}] {e}"))
}

/// The template, the resume and `ask` arguments, and the hosts of a `command` entry: a
/// preset fills them, and the `command` of the entry replaces its program.
struct CommandSpec {
    command: Vec<String>,
    resume: Vec<String>,
    ask_args: Vec<String>,
    hosts: Vec<String>,
}

fn command_spec(agent: &Agent) -> std::result::Result<CommandSpec, String> {
    let Some(name) = &agent.preset else {
        return Ok(CommandSpec {
            command: agent.command.clone(),
            resume: agent.resume.clone(),
            ask_args: Vec::new(),
            hosts: agent.hosts.clone(),
        });
    };
    let preset = harness_presets::find(name).ok_or_else(|| {
        format!(
            "has no preset {name:?}. The presets are {}",
            harness_presets::names()
        )
    })?;
    let mut command = match agent.command.as_slice() {
        [] => vec![preset.program.to_owned()],
        given => given.to_vec(),
    };
    command.extend(harness_presets::words(preset.args));
    let resume = match agent.resume.as_slice() {
        [] => harness_presets::words(preset.resume),
        given => given.to_vec(),
    };
    let mut hosts = agent.hosts.clone();
    hosts.extend(harness_presets::words(preset.hosts));
    Ok(CommandSpec {
        command,
        resume,
        ask_args: harness_presets::words(preset.ask_args),
        hosts,
    })
}

/// A typo in a mode name fails here, not at the first message from the game.
fn check_claude_modes(name: &str, modes: &BTreeMap<Permission, String>) -> Result<()> {
    for mode in modes.values() {
        if mode == claude::REFUSED_MODE {
            bail!("[agents.{name}] mode {mode} asks nothing, so the game cannot bound it");
        }
        if !claude::MODES.contains(&mode.as_str()) {
            bail!(
                "[agents.{name}] has no mode {mode:?}. The modes are {}",
                claude::MODES.join(", ")
            );
        }
    }
    Ok(())
}

pub fn expand(path: &str, home: &Path) -> Result<PathBuf> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_owned(),
        None => PathBuf::from(path),
    };
    if !path.is_absolute() {
        bail!("{} is not an absolute path", path.display());
    }
    Ok(path)
}

/// `canonicalize` resolves links, so a folder that is a link names its real folder
/// (SPEC.md 6.2, rule 10).
fn real_folder(path: &str, home: &Path, key: &str) -> Result<Vec<u8>> {
    let path = expand(path, home)?;
    let real = path
        .canonicalize()
        .with_context(|| format!("{key} {} does not exist", path.display()))?;
    Ok(path_bytes(&real))
}

pub fn parse(text: &str, home: &Path) -> Result<Config> {
    let file: File = toml::from_str(text)?;
    let network = file
        .sandbox
        .as_ref()
        .map(|s| s.agent_network)
        .unwrap_or_default();
    let story = story(file.story.as_ref(), network, home)?;
    let wow = expand(&file.wow.path, home)?;
    Ok(Config {
        wow,
        relay: relay(file, home)?,
        story,
    })
}

fn no_relay_keys(file: &File) -> Result<()> {
    let keys = [
        ("default_agent", file.default_agent.is_some()),
        ("default_cwd", file.default_cwd.is_some()),
        ("timeout_minutes", file.timeout_minutes.is_some()),
        (
            "permission_timeout_minutes",
            file.permission_timeout_minutes.is_some(),
        ),
        ("max_parallel_runs", file.max_parallel_runs.is_some()),
        ("daily_cost_cap_usd", file.daily_cost_cap_usd.is_some()),
        ("[agents]", file.agents.is_some()),
        ("[allow]", file.allow.is_some()),
        ("[sandbox]", file.sandbox.is_some()),
        ("[git]", file.git.is_some()),
    ];
    match keys.iter().find(|(_, given)| *given) {
        Some((key, _)) => bail!("{key} needs allowed_roots"),
        None => Ok(()),
    }
}

/// `allowed_roots` alone turns the relay on. A relay key with no roots is an error, so
/// a typo never leaves a relay half set up.
fn relay(file: File, home: &Path) -> Result<Option<RelayConfig>> {
    let Some(allowed_roots) = file.allowed_roots.as_ref() else {
        return no_relay_keys(&file).map(|()| None);
    };
    let roots = allowed_roots
        .iter()
        .map(|root| real_folder(root, home, "allowed root"))
        .collect::<Result<Vec<_>>>()?;
    // With no roots, every folder needs a click on the desktop first (SPEC.md 9.12).
    let real_home = home.canonicalize().ok().map(|h| path_bytes(&h));
    let base = match (&file.default_cwd, roots.first()) {
        (Some(cwd), _) => real_folder(cwd, home, "default_cwd")?,
        (None, Some(first)) => first.clone(),
        (None, None) => real_home
            .clone()
            .context("the home folder does not exist")?,
    };
    if Some(&base) != real_home.as_ref() && resolve_folder(&roots, &base, b"").is_none() {
        bail!("default_cwd is outside allowed_roots");
    }
    let file_agents = file.agents.unwrap_or_default();
    for (name, agent) in &file_agents {
        check_agent(name, agent)?;
    }
    let timeout = minutes(
        file.timeout_minutes,
        DEFAULT_TIMEOUT_MINUTES,
        MAX_TIMEOUT_MINUTES,
        "timeout_minutes",
    )?;
    let permission_timeout = minutes(
        file.permission_timeout_minutes,
        DEFAULT_PERMISSION_MINUTES,
        MAX_PERMISSION_MINUTES,
        "permission_timeout_minutes",
    )?;
    let max_parallel_runs = file.max_parallel_runs.unwrap_or(DEFAULT_MAX_PARALLEL_RUNS);
    if !(1..=MAX_PARALLEL_RUNS).contains(&max_parallel_runs) {
        bail!("max_parallel_runs must be 1 to {MAX_PARALLEL_RUNS}");
    }
    let daily_cost_cap_usd = cost_cap(file.daily_cost_cap_usd)?;
    let default_agent = file
        .default_agent
        .context("allowed_roots needs default_agent")?;
    if !file_agents.contains_key(&default_agent) {
        bail!("default_agent {default_agent:?} has no [agents] entry");
    }
    let levels = file_agents
        .iter()
        .map(|(name, agent)| (name.clone(), agent.permission))
        .collect();
    let agents = file_agents
        .into_iter()
        .map(|(name, agent)| (name, agent_spec(agent)))
        .collect();
    let allow = allow::parse(&file.allow.unwrap_or_default(), home)?;
    let hosts = hosts(file.sandbox.as_ref())?;
    let local_ports = local_ports(file.sandbox.as_ref())?;
    let agent_network = file
        .sandbox
        .as_ref()
        .map(|s| s.agent_network)
        .unwrap_or_default();
    Ok(Some(RelayConfig {
        policy: Policy {
            folders: Folders { roots, base },
            agents: levels,
            default_agent,
        },
        agents,
        timeout,
        permission_timeout,
        max_parallel_runs,
        daily_cost_cap_usd,
        allow,
        hosts,
        local_ports,
        agent_network,
        ci_checks: ci_checks(file.git.as_ref()),
    }))
}

/// `check_agent` passed, so the preset exists.
fn agent_spec(agent: Agent) -> AgentSpec {
    let command = match agent.kind {
        Kind::Command => command_spec(&agent).ok(),
        _ => None,
    };
    match command {
        Some(command) => AgentSpec {
            kind: agent.kind,
            command: command.command,
            env: agent.env,
            modes: agent.modes,
            agent_hosts: command.hosts,
            resume: command.resume,
            ask_args: command.ask_args,
        },
        None => AgentSpec {
            kind: agent.kind,
            command: agent.command,
            env: agent.env,
            modes: agent.modes,
            agent_hosts: agent.hosts,
            resume: Vec::new(),
            ask_args: Vec::new(),
        },
    }
}

#[cfg(unix)]
fn others_can_write(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o022 != 0
}

// TODO: check the ACL when the Windows build ships.
#[cfg(not(unix))]
fn others_can_write(_meta: &fs::Metadata) -> bool {
    false
}

pub fn load(dir: &Path, home: &Path) -> Result<Config> {
    let text = read_text(dir)?;
    let path = dir.join(FILE);
    parse(&text, home).with_context(|| format!("{} is not valid", path.display()))
}

/// The text of `config.toml`, only from a plain file that no other user can write.
pub fn read_text(dir: &Path) -> Result<String> {
    let path = dir.join(FILE);
    let meta = fs::symlink_metadata(&path).with_context(|| {
        format!(
            "can't read {}. To create it, run gnomish-relay setup <wow folder>",
            path.display()
        )
    })?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        bail!("{} is not a config file", path.display());
    }
    // Any user who can write the config can raise the ceiling of every message.
    if others_can_write(&meta) {
        bail!(
            "other users can write {}. Run: chmod 600 {}",
            path.display(),
            path.display()
        );
    }
    Ok(fs::read_to_string(&path)?)
}

/// An agent that setup found: its entry name, its kind, and its command.
pub type Found<'a> = (&'a str, Kind, &'a [&'a str]);

#[cfg(test)]
mod tests {
    use super::*;

    /// The TOML code blocks of one section of SPEC.md, in their order.
    fn spec_toml_blocks(section: &str) -> Vec<String> {
        let spec = include_str!("../../../SPEC.md");
        let start = spec.find(section).unwrap();
        let rest = &spec[start + section.len()..];
        let end = rest.find("\n## ").unwrap_or(rest.len());
        rest[..end]
            .split("```toml\n")
            .skip(1)
            .map(|block| block.split("```").next().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn the_config_example_of_the_spec_loads() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join("Documents/Code")).unwrap();
        fs::create_dir_all(home.path().join("Code/lighthouse")).unwrap();
        let blocks = spec_toml_blocks("## 12. Config");
        let example = blocks
            .iter()
            .find(|block| block.contains("allowed_roots ="))
            .unwrap();
        let allow = blocks.iter().find(|b| b.starts_with("[allow]")).unwrap();
        let sandbox = blocks.iter().find(|b| b.starts_with("[sandbox]")).unwrap();
        let text = format!("{example}\n{allow}\n{sandbox}");

        let config = parse(&text, home.path()).unwrap_or_else(|e| panic!("{e:#}\n{text}"));

        assert!(config.relay.is_some());
    }

    struct Home {
        dir: tempfile::TempDir,
    }

    impl Home {
        fn new() -> Home {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir_all(dir.path().join("Code/lighthouse")).unwrap();
            Home { dir }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn parse(&self, text: &str) -> Result<Config> {
            parse(text, self.path())
        }
    }

    const GOOD: &str = r#"
        allowed_roots = ["~/Code"]
        default_agent = "claude"
        [wow]
        path = "~/wow"
        [agents.claude]
        kind = "acp"
        command = ["claude-agent-acp"]
        permission = "auto-edit"
    "#;

    #[test]
    fn a_good_config_gives_the_roots_the_agents_and_the_game_folder() {
        let home = Home::new();
        let config = home.parse(GOOD).unwrap();
        let root = home.path().join("Code").canonicalize().unwrap();
        assert_eq!(
            config.require_relay().unwrap().policy.folders.roots,
            [path_bytes(&root)]
        );
        assert_eq!(
            config.require_relay().unwrap().policy.folders.base,
            path_bytes(&root)
        );
        assert_eq!(
            config.require_relay().unwrap().policy.agents["claude"],
            Permission::AutoEdit
        );
        assert_eq!(config.wow, home.path().join("wow"));
    }

    #[test]
    fn an_unknown_key_is_an_error() {
        let home = Home::new();
        assert!(
            home.parse(&format!("{GOOD}\nfull_auto_everything = true"))
                .is_err()
        );
        let typo = GOOD.replace("permission = ", "permision = ");
        assert!(home.parse(&typo).is_err());
    }

    #[test]
    fn an_agent_entry_gives_its_command_its_variables_and_its_modes() {
        let home = Home::new();
        let text = format!(
            "{GOOD}\nenv = [\"ANTHROPIC_API_KEY\"]\nmodes = {{ ask = \"plan\", auto-edit = \"default\" }}\n"
        );
        let config = home.parse(&text).unwrap();
        let claude = &config.require_relay().unwrap().agents["claude"];
        assert_eq!(claude.kind, Kind::Acp);
        assert_eq!(claude.command, ["claude-agent-acp"]);
        assert_eq!(claude.env, ["ANTHROPIC_API_KEY"]);
        assert_eq!(claude.modes[&Permission::Ask], "plan");
        assert_eq!(
            config.require_relay().unwrap().timeout,
            Duration::from_mins(30)
        );
    }

    #[test]
    fn a_bad_agent_entry_is_an_error() {
        let home = Home::new();
        let bad = [
            GOOD.replace("kind = \"acp\"\n", ""),
            GOOD.replace("[\"claude-agent-acp\"]", "[]"),
            GOOD.replace("kind = \"acp\"", "kind = \"echo\""),
            format!("{GOOD}\nenv = [\"PATH; rm\"]\n"),
            format!("{GOOD}\nmodes = {{ root = \"x\" }}\n"),
        ];
        for text in bad {
            assert!(home.parse(&text).is_err(), "{text}");
        }
    }

    const CLAUDE: &str = r#"
        allowed_roots = ["~/Code"]
        default_agent = "claude"
        [wow]
        path = "~/wow"
        [agents.claude]
        kind = "claude"
        command = ["claude"]
        permission = "ask"
    "#;

    #[test]
    fn a_claude_entry_takes_the_modes_of_claude_code() {
        let home = Home::new();
        let text = format!("{CLAUDE}\nmodes = {{ ask = \"manual\", full-auto = \"auto\" }}\n");
        let config = home.parse(&text).unwrap();
        assert_eq!(
            config.require_relay().unwrap().agents["claude"].kind,
            Kind::Claude
        );
        assert_eq!(
            config.require_relay().unwrap().agents["claude"].modes[&Permission::Ask],
            "manual"
        );
    }

    #[test]
    fn a_claude_entry_refuses_an_unknown_mode_and_the_mode_that_asks_nothing() {
        let home = Home::new();
        for mode in ["default", "bypassPermissions"] {
            let text = format!("{CLAUDE}\nmodes = {{ ask = \"{mode}\" }}\n");
            let error = format!("{:#}", home.parse(&text).err().unwrap());
            assert!(error.contains(mode), "{error}");
        }
        assert!(home.parse(&CLAUDE.replace("[\"claude\"]", "[]")).is_err());
    }

    #[test]
    fn the_allow_table_gives_rules_and_a_bad_pattern_is_an_error() {
        let home = Home::new();
        let text = format!("{GOOD}\n[allow]\ncommands = [\"cargo test *\"]\n");
        let config = home.parse(&text).unwrap();
        let chat = home.path().join("Code/lighthouse");
        let rules = config.require_relay().unwrap().allow.rules_for(&chat);
        assert_eq!(rules, [vec!["cargo".to_owned(), "test".to_owned()]]);
        let bad = format!("{GOOD}\n[allow]\ncommands = [\"rm -rf ~\"]\n");
        assert!(home.parse(&bad).is_err());
    }

    #[test]
    fn a_config_with_no_sandbox_section_allows_the_default_hosts() {
        let home = Home::new();

        let config = home.parse(GOOD).unwrap();

        let hosts = &config.require_relay().unwrap().hosts;
        assert!(hosts.allows("index.crates.io"));
        assert!(!hosts.allows("nodejs.org"));
    }

    #[test]
    fn the_sandbox_section_adds_hosts_or_turns_the_defaults_off() {
        let home = Home::new();
        let more = format!("{GOOD}\n[sandbox]\nallow_hosts = [\"nodejs.org\"]\n");
        let only = format!("{more}default_hosts = false\n");

        let more = home.parse(&more).unwrap();
        let only = home.parse(&only).unwrap();

        let more = &more.require_relay().unwrap().hosts;
        assert!(more.allows("nodejs.org") && more.allows("github.com"));
        let only = &only.require_relay().unwrap().hosts;
        assert!(only.allows("nodejs.org") && !only.allows("github.com"));
    }

    #[test]
    fn a_bad_host_or_key_in_the_sandbox_section_is_an_error() {
        let home = Home::new();
        for section in [
            "allow_hosts = [\"127.0.0.1\"]",
            "allow_hosts = [\"localhost\"]",
            "allow_hosts = [\"*.github.com\"]",
            "allow_hosts = [\"https://nodejs.org\"]",
            "allow_host = [\"nodejs.org\"]",
        ] {
            let text = format!("{GOOD}\n[sandbox]\n{section}\n");
            assert!(home.parse(&text).is_err(), "{section}");
        }
    }

    #[test]
    fn the_agent_network_is_open_unless_the_config_says_strict() {
        let home = Home::new();
        let strict = format!("{GOOD}\n[sandbox]\nagent_network = \"strict\"\n");
        let bad = format!("{GOOD}\n[sandbox]\nagent_network = \"closed\"\n");

        let relay = |text: &str| home.parse(text).unwrap().relay.unwrap().agent_network;

        assert_eq!(relay(GOOD), AgentNetwork::Open);
        assert_eq!(relay(&strict), AgentNetwork::Strict);
        assert!(home.parse(&bad).is_err());
    }

    #[test]
    fn the_agent_hosts_of_an_entry_are_host_names() {
        let home = Home::new();
        let good = GOOD.replace(
            "kind = \"acp\"",
            "kind = \"acp\"\nagent_hosts = [\"bedrock.example.com\"]",
        );
        let bad = GOOD.replace(
            "kind = \"acp\"",
            "kind = \"acp\"\nagent_hosts = [\"10.0.0.1\"]",
        );

        let config = home.parse(&good).unwrap();

        let relay = config.require_relay().unwrap();
        let hosts: Vec<&Vec<String>> = relay.agents.values().map(|a| &a.agent_hosts).collect();
        assert!(
            hosts.contains(&&vec!["bedrock.example.com".to_owned()]),
            "{good}"
        );
        assert!(home.parse(&bad).is_err());
    }

    #[test]
    fn the_local_ports_come_sorted_and_once_each() {
        let home = Home::new();
        let text = format!("{GOOD}\n[sandbox]\nlocal_ports = [5432, 3000, 5432]\n");

        let config = home.parse(&text).unwrap();

        assert_eq!(
            config.require_relay().unwrap().local_ports,
            vec![3000, 5432]
        );
        let plain = home.parse(GOOD).unwrap();
        assert!(plain.require_relay().unwrap().local_ports.is_empty());
    }

    #[test]
    fn a_local_port_that_runs_any_code_or_that_the_sandbox_uses_is_an_error() {
        let home = Home::new();
        for port in ["2375", "2376", "9222", "3128", "0", "70000", "\"5432\""] {
            let text = format!("{GOOD}\n[sandbox]\nlocal_ports = [{port}]\n");
            assert!(home.parse(&text).is_err(), "{port}");
        }
    }

    #[test]
    fn a_config_with_no_allow_table_has_an_empty_one() {
        let home = Home::new();
        let config = home.parse(GOOD).unwrap();
        assert!(
            config
                .require_relay()
                .unwrap()
                .allow
                .rules_for(home.path())
                .is_empty()
        );
    }

    #[test]
    fn a_codex_entry_has_a_command_and_no_modes() {
        let home = Home::new();
        let codex = CLAUDE.replace("kind = \"claude\"", "kind = \"codex\"");
        assert_eq!(
            home.parse(&codex).unwrap().require_relay().unwrap().agents["claude"].kind,
            Kind::Codex
        );
        let with_modes = format!("{codex}\nmodes = {{ ask = \"plan\" }}\n");
        assert!(home.parse(&with_modes).is_err());
    }

    /// A config with one entry `[agents.x]` of kind command and `lines`.
    fn command_entry(lines: &str) -> String {
        format!(
            "allowed_roots = [\"~/Code\"]\ndefault_agent = \"x\"\n[wow]\npath = \"~/wow\"\n\
             [agents.x]\nkind = \"command\"\npermission = \"auto-edit\"\n{lines}\n"
        )
    }

    fn command_spec_of(lines: &str) -> Result<AgentSpec> {
        let home = Home::new();
        let config = home.parse(&command_entry(lines))?;
        let mut relay = config.relay.unwrap();
        Ok(relay.agents.remove("x").unwrap())
    }

    #[test]
    fn a_preset_fills_the_template_the_resume_arguments_and_the_hosts() {
        let spec = command_spec_of("preset = \"aider\"\nagent_hosts = [\"api.x.com\"]").unwrap();

        assert_eq!(spec.kind, Kind::Command);
        assert_eq!(spec.command[..2], ["aider", "--message-file={prompt_file}"]);
        assert_eq!(spec.resume, ["--restore-chat-history"]);
        assert!(spec.ask_args.contains(&"--dry-run".to_owned()));
        assert_eq!(spec.agent_hosts, ["api.x.com"]);
        let gemini = command_spec_of("preset = \"gemini\"").unwrap();
        assert!(
            gemini
                .agent_hosts
                .contains(&"generativelanguage.googleapis.com".to_owned())
        );
    }

    #[test]
    fn the_command_of_a_preset_entry_replaces_its_program_and_adds_flags() {
        let spec = command_spec_of(
            "preset = \"aider\"\ncommand = [\"/opt/aider\", \"--model\", \"o3\"]\nresume = [\"--x\"]",
        )
        .unwrap();

        assert_eq!(
            spec.command[..4],
            [
                "/opt/aider",
                "--model",
                "o3",
                "--message-file={prompt_file}"
            ]
        );
        assert_eq!(spec.resume, ["--x"]);
    }

    #[test]
    fn a_custom_command_entry_keeps_its_template() {
        let spec = command_spec_of("command = [\"tool\", \"-p\", \"{prompt}\"]\nresume = [\"-c\"]")
            .unwrap();

        assert_eq!(spec.command, ["tool", "-p", "{prompt}"]);
        assert_eq!(spec.resume, ["-c"]);
        assert!(spec.ask_args.is_empty());
    }

    #[test]
    fn a_bad_command_entry_fails_with_the_reason() {
        let error = |lines: &str| format!("{:#}", command_spec_of(lines).unwrap_err());

        assert!(error("preset = \"vim\"").contains("has no preset \"vim\""));
        assert!(error("command = [\"t\", \"{promt}\"]").contains("{promt}"));
        assert!(error("").contains("needs a command"));
        assert!(error("command = [\"t\"]\nmodes = { ask = \"x\" }").contains("no modes"));
        let claude = CLAUDE.replace(
            "permission = \"ask\"",
            "permission = \"ask\"\npreset = \"aider\"",
        );
        let text = format!("{:#}", Home::new().parse(&claude).err().unwrap());
        assert!(text.contains("only for kind command"), "{text}");
    }

    #[test]
    fn an_old_entry_with_the_acp_adapter_of_claude_still_works() {
        let home = Home::new();
        let config = home.parse(GOOD).unwrap();
        assert_eq!(
            config.require_relay().unwrap().agents["claude"].kind,
            Kind::Acp
        );
        assert_eq!(
            config.require_relay().unwrap().agents["claude"].command,
            ["claude-agent-acp"]
        );
    }

    #[test]
    fn the_timeout_has_limits() {
        let home = Home::new();
        let with = |minutes: u64| {
            GOOD.replace(
                "default_agent",
                &format!("timeout_minutes = {minutes}\ndefault_agent"),
            )
        };
        assert_eq!(
            home.parse(&with(5))
                .unwrap()
                .require_relay()
                .unwrap()
                .timeout,
            Duration::from_mins(5)
        );
        assert!(home.parse(&with(0)).is_err());
        assert!(home.parse(&with(241)).is_err());
    }

    #[test]
    fn max_parallel_runs_is_three_by_default_and_one_to_sixteen() {
        let home = Home::new();
        let with = |runs: u64| {
            GOOD.replace(
                "default_agent",
                &format!("max_parallel_runs = {runs}\ndefault_agent"),
            )
        };
        let runs = |text: &str| home.parse(text).unwrap().relay.unwrap().max_parallel_runs;

        assert_eq!(runs(GOOD), 3);
        assert_eq!(runs(&with(1)), 1);
        assert_eq!(runs(&with(16)), 16);
        assert!(home.parse(&with(0)).is_err());
        assert!(home.parse(&with(17)).is_err());
    }

    #[test]
    fn the_daily_cost_cap_is_off_by_default_and_a_positive_number_of_dollars() {
        let home = Home::new();
        let with = |cap: &str| {
            GOOD.replace(
                "default_agent",
                &format!("daily_cost_cap_usd = {cap}\ndefault_agent"),
            )
        };
        let cap = |text: &str| home.parse(text).unwrap().relay.unwrap().daily_cost_cap_usd;

        assert_eq!(cap(GOOD), None);
        assert_eq!(cap(&with("5.5")), Some(5.5));
        assert_eq!(cap(&with("5")), Some(5.0));
        assert_eq!(cap(&with("10000")), Some(10000.0));
        for bad in ["0", "-1.0", "10000.5", "nan", "inf", "\"5\""] {
            assert!(home.parse(&with(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn daily_cost_cap_usd_needs_allowed_roots() {
        let home = Home::new();

        let error = home
            .parse("daily_cost_cap_usd = 2.0\n[wow]\npath = \"~/wow\"\n")
            .err()
            .unwrap();

        assert_eq!(error.to_string(), "daily_cost_cap_usd needs allowed_roots");
    }

    #[test]
    fn max_parallel_runs_needs_allowed_roots() {
        let home = Home::new();

        let error = home
            .parse("max_parallel_runs = 2\n[wow]\npath = \"~/wow\"\n")
            .err()
            .unwrap();

        assert_eq!(error.to_string(), "max_parallel_runs needs allowed_roots");
    }

    #[test]
    fn a_config_with_no_story_section_has_no_story_program() {
        let home = Home::new();
        assert_eq!(home.parse(GOOD).unwrap().story, None);
    }

    #[test]
    fn the_story_section_gives_an_absolute_program_a_lore_pack_and_a_timeout() {
        let home = Home::new();
        let text = format!(
            "{GOOD}\n[story]\nprogram = \"~/bin/timeways-story\"\nlore_pack = \"~/lore.sqlite\"\n"
        );
        let story = home.parse(&text).unwrap().story.unwrap();
        let program = story.program.unwrap();
        assert_eq!(program.program, home.path().join("bin/timeways-story"));
        assert_eq!(program.lore_pack, home.path().join("lore.sqlite"));
        assert_eq!(story.timeout, Duration::from_mins(2));
        let text = format!("{text}timeout_seconds = 5\n");
        assert_eq!(
            home.parse(&text).unwrap().story.unwrap().timeout,
            Duration::from_secs(5)
        );
    }

    const STORY: &str = "[story]\nprogram = \"~/x\"\nlore_pack = \"~/l\"\n";

    fn model_of(home: &Home, keys: &str) -> Result<ModelSpec> {
        let text = format!("{GOOD}\n{STORY}{keys}");
        Ok(home.parse(&text)?.story.context("no story")?.model)
    }

    #[test]
    fn a_story_section_with_no_model_has_none_and_the_default_budget() {
        let home = Home::new();
        let model = model_of(&home, "").unwrap();
        assert_eq!(model.choice, ModelChoice::None);
        assert_eq!(model.timeout, Duration::from_mins(1));
        assert_eq!(model.budget_window_minutes, 20);
    }

    #[test]
    fn the_claude_model_runs_the_claude_program_with_an_optional_model_name() {
        let home = Home::new();
        let keys = "model = \"claude\"\nclaude_model = \"haiku\"\nmodel_timeout_seconds = 30\nbudget_window_minutes = 60\n";
        let model = model_of(&home, keys).unwrap();
        assert_eq!(
            model.choice,
            ModelChoice::Claude {
                command: vec!["claude".into()],
                model: Some("haiku".into()),
            }
        );
        assert_eq!(model.timeout, Duration::from_secs(30));
        assert_eq!(model.budget_window_minutes, 60);
    }

    /// Decisions 4 and 18 of SPEC.md 9.7: the story route never reaches a relay agent.
    #[test]
    fn a_story_model_takes_nothing_from_the_agents_of_the_relay() {
        let home = Home::new();
        let agent = CLAUDE.replace(
            "command = [\"claude\"]",
            "command = [\"/opt/relay-claude\"]\nenv = [\"RELAY_SECRET\"]",
        );
        let text = format!("{agent}\n{STORY}model = \"claude\"\n");
        let config = home.parse(&text).unwrap();
        let ModelChoice::Claude { command, .. } =
            config.story.as_ref().unwrap().model.choice.clone()
        else {
            panic!("not claude");
        };
        assert_eq!(command, ["claude"]);
        assert_eq!(
            config.require_relay().unwrap().agents["claude"].command,
            ["/opt/relay-claude"]
        );
    }

    #[test]
    fn a_local_model_needs_a_loopback_url_and_a_model_name() {
        let home = Home::new();
        let keys =
            "model = \"local\"\nlocal_url = \"http://[::1]:1234\"\nlocal_model = \"qwen3\"\n";
        assert_eq!(
            model_of(&home, keys).unwrap().choice,
            ModelChoice::Local(LocalModel {
                url: "http://[::1]:1234".into(),
                model: "qwen3".into(),
            })
        );
    }

    #[test]
    fn a_local_url_with_localhost_or_another_host_is_refused_at_load() {
        let home = Home::new();
        for url in [
            "http://localhost:11434",
            "http://192.168.1.2:11434",
            "http://example.com:80",
        ] {
            let keys = format!("model = \"local\"\nlocal_url = \"{url}\"\nlocal_model = \"m\"\n");
            let error = format!("{:#}", model_of(&home, &keys).err().unwrap());
            assert!(error.contains("local_url must be"), "{error}");
        }
    }

    #[test]
    fn model_keys_that_do_not_fit_the_model_are_refused() {
        let home = Home::new();
        let bad = [
            "model = \"gpt\"\n",
            "claude_model = \"haiku\"\n",
            "local_url = \"http://127.0.0.1:1\"\n",
            "model = \"claude\"\nlocal_model = \"m\"\n",
            "model = \"local\"\nclaude_model = \"haiku\"\nlocal_url = \"http://127.0.0.1:1\"\nlocal_model = \"m\"\n",
            "model = \"local\"\nlocal_model = \"m\"\n",
            "model = \"local\"\nlocal_url = \"http://127.0.0.1:1\"\n",
            "model = \"claude\"\nclaude_model = \"--dangerously-skip-permissions\"\n",
            "model = \"claude\"\nclaude_model = \"\"\n",
            "model = \"claude\"\nclaude_model = \"two words\"\n",
            "model_timeout_seconds = 0\n",
            "model_timeout_seconds = 601\n",
            "budget_window_minutes = 0\n",
            "budget_window_minutes = 1441\n",
        ];
        for keys in bad {
            assert!(model_of(&home, keys).is_err(), "{keys}");
        }
    }

    #[test]
    fn a_story_program_by_name_only_is_an_error_because_the_bridge_never_looks_on_path() {
        let home = Home::new();
        let text =
            format!("{GOOD}\n[story]\nprogram = \"timeways-story\"\nlore_pack = \"~/lore\"\n");
        let error = format!("{:#}", home.parse(&text).err().unwrap());
        assert!(error.contains("[story] program"), "{error}");
    }

    #[test]
    fn a_bad_story_section_is_an_error() {
        let home = Home::new();
        let bad = [
            "[story]\nprogram = \"~/x\"\n",
            "[story]\nlore_pack = \"~/l\"\n",
            "[story]\nprogram = \"~/x\"\nlore_pack = \"lore.sqlite\"\n",
            "[story]\nprogram = \"~/x\"\nlore_pack = \"~/l\"\ntimeout_seconds = 0\n",
            "[story]\nprogram = \"~/x\"\nlore_pack = \"~/l\"\ntimeout_seconds = 601\n",
            "[story]\nprogram = \"~/x\"\nlore_pack = \"~/l\"\nargs = [\"--yolo\"]\n",
        ];
        for story in bad {
            assert!(home.parse(&format!("{GOOD}\n{story}")).is_err(), "{story}");
        }
    }

    const TIMEWAYS_ONLY: &str = "[wow]\npath = \"~/wow\"\n\n[story]\nmodel = \"claude\"\n";

    #[test]
    fn a_config_with_no_allowed_roots_has_no_relay_and_keeps_its_story() {
        let home = Home::new();
        let config = home.parse(TIMEWAYS_ONLY).unwrap();
        assert!(config.relay.is_none());
        assert!(config.require_relay().is_err());
        let story = config.story.unwrap();
        assert_eq!(story.program, None);
        assert!(matches!(story.model.choice, ModelChoice::Claude { .. }));
    }

    #[test]
    fn a_relay_key_with_no_allowed_roots_is_an_error() {
        let home = Home::new();
        let keys = [
            "default_agent = \"echo\"\n",
            "default_cwd = \"~/Code\"\n",
            "timeout_minutes = 5\n",
            "permission_timeout_minutes = 5\n",
        ];
        for key in keys {
            let error = format!(
                "{:#}",
                home.parse(&format!("{key}{TIMEWAYS_ONLY}")).err().unwrap()
            );
            assert!(error.contains("needs allowed_roots"), "{error}");
        }
        for table in [
            "[agents.echo]\nkind = \"echo\"\npermission = \"ask\"\n",
            "[allow]\n",
            "[sandbox]\n",
            "[git]\nci_checks = true\n",
        ] {
            let error = format!(
                "{:#}",
                home.parse(&format!("{TIMEWAYS_ONLY}{table}"))
                    .err()
                    .unwrap()
            );
            assert!(error.contains("needs allowed_roots"), "{error}");
        }
    }

    #[test]
    fn ci_checks_are_off_unless_the_git_table_turns_them_on() {
        let home = Home::new();
        let relay = |text: &str| home.parse(text).unwrap().relay.unwrap();

        let off = relay(GOOD);
        let on = relay(&format!("{GOOD}\n[git]\nci_checks = true\n"));

        assert_eq!(off.ci_checks, CiChecks::Off);
        assert_eq!(
            on.ci_checks,
            CiChecks::On {
                program: PathBuf::from("gh")
            }
        );
        assert!(
            home.parse(&format!("{GOOD}\n[git]\npush = true\n"))
                .is_err()
        );
    }

    #[test]
    fn allowed_roots_with_no_default_agent_is_an_error() {
        let home = Home::new();
        let text = GOOD.replace("default_agent = \"claude\"", "");
        assert!(home.parse(&text).is_err());
    }

    #[test]
    fn a_story_program_and_its_lore_pack_go_together() {
        let home = Home::new();
        let text = format!("{GOOD}\n[story]\nprogram = \"~/x\"\n");
        let error = format!("{:#}", home.parse(&text).err().unwrap());
        assert!(error.contains("go together"), "{error}");
    }

    #[test]
    fn an_unknown_permission_is_an_error() {
        let home = Home::new();
        assert!(home.parse(&GOOD.replace("auto-edit", "yolo")).is_err());
    }

    #[test]
    fn a_missing_root_is_an_error() {
        let home = Home::new();
        assert!(home.parse(&GOOD.replace("~/Code", "~/Nope")).is_err());
    }

    #[test]
    fn a_relative_root_is_an_error() {
        let home = Home::new();
        assert!(home.parse(&GOOD.replace("~/Code", "Code")).is_err());
    }

    #[test]
    fn an_empty_list_of_roots_keeps_the_relay_on_with_the_home_folder_as_the_base() {
        let home = Home::new();
        let text = GOOD.replace("[\"~/Code\"]", "[]");

        let config = home.parse(&text).unwrap();

        let folders = &config.require_relay().unwrap().policy.folders;
        assert!(folders.roots.is_empty());
        assert_eq!(
            folders.base,
            path_bytes(&home.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn the_home_folder_can_be_the_default_folder() {
        let home = Home::new();
        let text = GOOD.replace("default_agent", "default_cwd = \"~\"\ndefault_agent");

        let config = home.parse(&text).unwrap();

        let folders = &config.require_relay().unwrap().policy.folders;
        assert_eq!(
            folders.base,
            path_bytes(&home.path().canonicalize().unwrap())
        );
    }

    #[test]
    fn a_default_folder_outside_the_roots_is_an_error() {
        let home = Home::new();
        let text = GOOD.replace("default_agent", "default_cwd = \"/etc\"\ndefault_agent");
        assert!(home.parse(&text).is_err());
    }

    // The temp folder of macOS is such a link: /var is /private/var.
    #[cfg(unix)]
    #[test]
    fn a_default_folder_through_a_link_is_the_real_folder_in_the_root() {
        let home = Home::new();
        std::os::unix::fs::symlink(home.path().join("Code"), home.path().join("link")).unwrap();
        let text = GOOD.replace("default_agent", "default_cwd = \"~/link\"\ndefault_agent");

        let config = home.parse(&text).unwrap();

        let root = home.path().join("Code").canonicalize().unwrap();
        assert_eq!(
            config.require_relay().unwrap().policy.folders.base,
            path_bytes(&root)
        );
    }

    #[test]
    fn a_default_agent_with_no_entry_is_an_error() {
        let home = Home::new();
        assert!(
            home.parse(&GOOD.replace("\"claude\"", "\"codex\""))
                .is_err()
        );
    }

    #[test]
    fn the_game_can_lower_the_level_but_never_raise_it() {
        let ask = Permission::Ask;
        let edit = Permission::AutoEdit;
        assert_eq!(edit.ceiling(Some(Permission::FullAuto)), edit);
        assert_eq!(edit.ceiling(Some(ask)), ask);
        assert_eq!(edit.ceiling(None), edit);
        assert_eq!(Permission::from_game("root"), ask);
    }

    #[cfg(unix)]
    #[test]
    fn a_config_that_others_can_write_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let home = Home::new();
        let dir = home.path().join("config");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join(FILE), GOOD).unwrap();
        fs::set_permissions(dir.join(FILE), fs::Permissions::from_mode(0o666)).unwrap();
        assert!(load(&dir, home.path()).is_err());
        fs::set_permissions(dir.join(FILE), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load(&dir, home.path()).is_ok());
    }
}
