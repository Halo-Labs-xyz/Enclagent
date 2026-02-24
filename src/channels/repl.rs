//! Interactive REPL channel with line editing and markdown rendering.
//!
//! Provides the primary CLI interface for interacting with the agent.
//! Uses rustyline for line editing, history, and tab-completion.
//! Uses termimad for rendering markdown responses inline.
//!
//! ## Commands
//!
//! - `/help` - Show available commands
//! - `/quit` or `/exit` - Exit the REPL
//! - `/debug` - Toggle debug mode (verbose tool output)
//! - `/undo` - Undo the last turn
//! - `/redo` - Redo an undone turn
//! - `/clear` - Clear the conversation
//! - `/compact` - Compact the context
//! - `/new` - Start a new thread
//! - `yes`/`no`/`always` - Respond to tool approval prompts

use std::borrow::Cow;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use regex::Regex;
use rustyline::completion::Completer;
use rustyline::config::Config;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Editor, Helper};
use serde::Deserialize;
use termimad::MadSkin;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::agent::truncate_for_preview;
use crate::channels::{Channel, IncomingMessage, MessageStream, OutgoingResponse, StatusUpdate};
use crate::error::ChannelError;

/// Max characters for tool result previews in the terminal.
const CLI_TOOL_RESULT_MAX: usize = 200;

/// Max characters for thinking/status messages in the terminal.
const CLI_STATUS_MAX: usize = 200;

/// Slash commands available in the REPL.
const SLASH_COMMANDS: &[&str] = &[
    "/help",
    "/quit",
    "/exit",
    "/debug",
    "/model",
    "/positions",
    "/exposure",
    "/funding",
    "/vault",
    "/risk",
    "/pause-agent",
    "/resume-agent",
    "/verify",
    "/receipts",
    "/undo",
    "/redo",
    "/clear",
    "/compact",
    "/new",
    "/interrupt",
    "/version",
    "/tools",
    "/ping",
    "/job",
    "/status",
    "/cancel",
    "/list",
    "/heartbeat",
    "/summarize",
    "/suggest",
    "/thread",
    "/resume",
];

/// Stable kind label for WS-2 runtime status payloads.
const WS2_RUNTIME_STATUS_KIND: &str = "ws2_runtime_status";

/// Transport envelope for WS-2 runtime status updates over `StatusUpdate::Status`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct Ws2RuntimeStatusPayload {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    version: Option<u8>,
    #[serde(default)]
    stage: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    error: Option<Ws2RuntimeErrorPayload>,
}

/// Structured failure data for WS-2 runtime status events.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct Ws2RuntimeErrorPayload {
    #[serde(default)]
    domain: String,
    #[serde(default)]
    code: String,
    #[serde(default)]
    retryable: bool,
    #[serde(default)]
    message: String,
}

/// Rustyline helper for slash-command tab completion.
struct ReplHelper;

impl Completer for ReplHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        if !line.starts_with('/') {
            return Ok((0, vec![]));
        }

        let prefix = &line[..pos];
        let matches: Vec<String> = SLASH_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| cmd.to_string())
            .collect();

        Ok((0, matches))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        if !line.starts_with('/') || pos < line.len() {
            return None;
        }

        SLASH_COMMANDS
            .iter()
            .find(|cmd| cmd.starts_with(line) && **cmd != line)
            .map(|cmd| cmd[line.len()..].to_string())
    }
}

impl Highlighter for ReplHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[90m{hint}\x1b[0m"))
    }
}

impl Validator for ReplHelper {}
impl Helper for ReplHelper {}

/// Build a termimad skin with our color scheme.
fn make_skin() -> MadSkin {
    let mut skin = MadSkin::default();
    skin.set_headers_fg(termimad::crossterm::style::Color::Yellow);
    skin.bold.set_fg(termimad::crossterm::style::Color::White);
    skin.italic
        .set_fg(termimad::crossterm::style::Color::Magenta);
    skin.inline_code
        .set_fg(termimad::crossterm::style::Color::Green);
    skin.code_block
        .set_fg(termimad::crossterm::style::Color::Green);
    skin.code_block.left_margin = 2;
    skin
}

/// Format JSON params as `key: value` lines for the approval card.
fn format_json_params(params: &serde_json::Value, indent: &str) -> String {
    match params {
        serde_json::Value::Object(map) => {
            let mut lines = Vec::new();
            for (key, value) in map {
                let val_str = match value {
                    serde_json::Value::String(s) => {
                        let display = if s.len() > 120 { &s[..120] } else { s };
                        format!("\x1b[32m\"{display}\"\x1b[0m")
                    }
                    other => {
                        let rendered = other.to_string();
                        if rendered.len() > 120 {
                            format!("{}...", &rendered[..120])
                        } else {
                            rendered
                        }
                    }
                };
                lines.push(format!("{indent}\x1b[36m{key}\x1b[0m: {val_str}"));
            }
            lines.join("\n")
        }
        other => {
            let pretty = serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string());
            let truncated = if pretty.len() > 300 {
                format!("{}...", &pretty[..300])
            } else {
                pretty
            };
            truncated
                .lines()
                .map(|l| format!("{indent}\x1b[90m{l}\x1b[0m"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

/// REPL channel with line editing and markdown rendering.
pub struct ReplChannel {
    /// Optional single message to send (for -m flag).
    single_message: Option<String>,
    /// Debug mode flag (shared with input thread).
    debug_mode: Arc<AtomicBool>,
    /// Whether we're currently streaming (chunks have been printed without a trailing newline).
    is_streaming: Arc<AtomicBool>,
    /// When true, the one-liner startup banner is suppressed (boot screen shown instead).
    suppress_banner: Arc<AtomicBool>,
}

impl ReplChannel {
    /// Create a new REPL channel.
    pub fn new() -> Self {
        Self {
            single_message: None,
            debug_mode: Arc::new(AtomicBool::new(false)),
            is_streaming: Arc::new(AtomicBool::new(false)),
            suppress_banner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a REPL channel that sends a single message and exits.
    pub fn with_message(message: String) -> Self {
        Self {
            single_message: Some(message),
            debug_mode: Arc::new(AtomicBool::new(false)),
            is_streaming: Arc::new(AtomicBool::new(false)),
            suppress_banner: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Suppress the one-liner startup banner (boot screen will be shown instead).
    pub fn suppress_banner(&self) {
        self.suppress_banner.store(true, Ordering::Relaxed);
    }

    fn is_debug(&self) -> bool {
        self.debug_mode.load(Ordering::Relaxed)
    }
}

impl Default for ReplChannel {
    fn default() -> Self {
        Self::new()
    }
}

fn print_help() {
    // Bold white for section headers, bold cyan for commands, dim gray for descriptions
    let h = "\x1b[1m"; // bold (section headers)
    let c = "\x1b[1;36m"; // bold cyan (commands)
    let d = "\x1b[90m"; // dim gray (descriptions)
    let r = "\x1b[0m"; // reset

    println!();
    println!("  {h}Enclagent REPL{r}");
    println!();
    println!("  {h}Commands{r}");
    println!("  {c}/help{r}              {d}show this help{r}");
    println!("  {c}/debug{r}             {d}toggle verbose output{r}");
    println!("  {c}/quit{r} {c}/exit{r}        {d}exit the repl{r}");
    println!();
    println!("  {h}WS-2 Workflows{r}");
    println!("  {c}/positions{r}         {d}positions workflow{r}");
    println!("  {c}/exposure{r}          {d}exposure workflow{r}");
    println!("  {c}/funding{r}           {d}funding workflow{r}");
    println!("  {c}/vault{r}             {d}vault workflow{r}");
    println!("  {c}/risk{r}              {d}risk workflow{r}");
    println!("  {c}/pause-agent{r}       {d}pause runtime workflow{r}");
    println!("  {c}/resume-agent{r}      {d}resume runtime workflow{r}");
    println!("  {c}/verify <id>{r}       {d}verify receipt workflow{r}");
    println!("  {c}/receipts <agent>{r}  {d}list receipt workflow{r}");
    println!();
    println!("  {h}Conversation{r}");
    println!("  {c}/undo{r}              {d}undo the last turn{r}");
    println!("  {c}/redo{r}              {d}redo an undone turn{r}");
    println!("  {c}/clear{r}             {d}clear conversation{r}");
    println!("  {c}/compact{r}           {d}compact context window{r}");
    println!("  {c}/new{r}               {d}new conversation thread{r}");
    println!("  {c}/interrupt{r}         {d}stop current operation{r}");
    println!();
    println!("  {h}Approval responses{r}");
    println!("  {c}yes{r} ({c}y{r})            {d}approve tool execution{r}");
    println!("  {c}no{r} ({c}n{r})             {d}deny tool execution{r}");
    println!("  {c}always{r} ({c}a{r})         {d}approve for this session{r}");
    println!();
}

/// Decode `StatusUpdate::Status` JSON payloads for WS-2 runtime status.
fn parse_ws2_runtime_status(msg: &str) -> Option<Ws2RuntimeStatusPayload> {
    let payload: Ws2RuntimeStatusPayload = serde_json::from_str(msg).ok()?;
    if payload.kind == WS2_RUNTIME_STATUS_KIND {
        Some(payload)
    } else {
        None
    }
}

fn ws2_stage_label(stage: &str) -> String {
    match stage {
        "intent" => "intent".to_string(),
        "execution" => "execution".to_string(),
        "verification" => "verification".to_string(),
        _ => stage.replace('_', " "),
    }
}

fn ws2_state_style(state: &str) -> (&'static str, &'static str, String) {
    match state {
        "started" => ("\u{25CB}", "\x1b[33m", "started".to_string()),
        "completed" => ("\u{25CF}", "\x1b[32m", "completed".to_string()),
        "failed" => ("\u{2717}", "\x1b[31m", "failed".to_string()),
        "blocked" => ("\u{26D4}", "\x1b[31m", "blocked".to_string()),
        "awaiting_input" => ("\u{23F3}", "\x1b[36m", "awaiting input".to_string()),
        _ => ("\u{25CB}", "\x1b[90m", state.replace('_', " ")),
    }
}

fn ws2_incident_style(domain: &str) -> (&'static str, &'static str) {
    match domain {
        "auth" => ("auth", "\x1b[33m"),
        "channel" => ("channel", "\x1b[35m"),
        "mcp" => ("mcp", "\x1b[36m"),
        "verification" => ("verification", "\x1b[31m"),
        _ => ("runtime", "\x1b[31m"),
    }
}

fn redact_sensitive_preview(raw: &str) -> String {
    let mut value = raw.to_string();

    let patterns = [
        (r"(?i)\b(bearer)\s+[a-z0-9._\-~+/]+=*", "$1 [REDACTED]"),
        (
            r"(?i)\b(token|api[_\-]?key|secret|password)\b(\s*[:=]\s*)([^,\s]+)",
            "$1$2[REDACTED]",
        ),
        (r"(?i)\bsk-[a-z0-9\-]{10,}\b", "sk-[REDACTED]"),
    ];

    for (pattern, replacement) in patterns {
        if let Ok(re) = Regex::new(pattern) {
            value = re.replace_all(&value, replacement).to_string();
        }
    }

    value
}

fn build_ws2_incident_lines(error: &Ws2RuntimeErrorPayload) -> Vec<String> {
    let (domain, color) = ws2_incident_style(&error.domain);
    let code = if error.code.trim().is_empty() {
        "runtime.unclassified"
    } else {
        error.code.as_str()
    };
    let message = if error.message.trim().is_empty() {
        "No incident message provided".to_string()
    } else {
        truncate_for_preview(&redact_sensitive_preview(&error.message), CLI_STATUS_MAX)
    };
    let retryable = if error.retryable { "yes" } else { "no" };

    vec![
        format!("  {color}\u{26A0} incident {domain}\x1b[0m"),
        format!("    \x1b[90mcode:\x1b[0m {code}"),
        format!("    \x1b[90mretryable:\x1b[0m {retryable}"),
        format!("    \x1b[90mmessage:\x1b[0m {message}"),
    ]
}

fn build_ws2_runtime_status_lines(payload: &Ws2RuntimeStatusPayload) -> Vec<String> {
    let mut lines = Vec::new();
    let stage = ws2_stage_label(&payload.stage);
    let (icon, color, state) = ws2_state_style(&payload.state);
    let mut headline = format!("  {color}{icon} {stage}: {state}\x1b[0m");

    if let Some(intent) = payload
        .intent
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        headline.push_str(&format!(" \x1b[90m({intent})\x1b[0m"));
    }
    lines.push(headline);

    if let Some(version) = payload.version
        && version != 1
    {
        lines.push(format!(
            "    \x1b[90mstatus payload version: {version}\x1b[0m"
        ));
    }

    if let Some(detail) = payload
        .detail
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!(
            "    \x1b[90m{}\x1b[0m",
            truncate_for_preview(&redact_sensitive_preview(detail), CLI_STATUS_MAX)
        ));
    }

    if let Some(error) = &payload.error {
        lines.extend(build_ws2_incident_lines(error));
    }

    lines
}

fn render_ws2_runtime_status(payload: &Ws2RuntimeStatusPayload) {
    for line in build_ws2_runtime_status_lines(payload) {
        eprintln!("{line}");
    }
}

/// Get the history file path (~/.enclagent/history).
fn history_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".enclagent")
        .join("history")
}

#[async_trait]
impl Channel for ReplChannel {
    fn name(&self) -> &str {
        "repl"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = mpsc::channel(32);
        let single_message = self.single_message.clone();
        let debug_mode = Arc::clone(&self.debug_mode);
        let suppress_banner = Arc::clone(&self.suppress_banner);

        std::thread::spawn(move || {
            // Single message mode: send it and return
            if let Some(msg) = single_message {
                let incoming = IncomingMessage::new("repl", "default", &msg);
                let _ = tx.blocking_send(incoming);
                return;
            }

            // Set up rustyline
            let config = Config::builder()
                .history_ignore_dups(true)
                .expect("valid config")
                .auto_add_history(true)
                .completion_type(CompletionType::List)
                .build();

            let mut rl = match Editor::with_config(config) {
                Ok(editor) => editor,
                Err(e) => {
                    eprintln!("Failed to initialize line editor: {e}");
                    return;
                }
            };

            rl.set_helper(Some(ReplHelper));

            // Load history
            let hist_path = history_path();
            if let Some(parent) = hist_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = rl.load_history(&hist_path);

            if !suppress_banner.load(Ordering::Relaxed) {
                println!("\x1b[1mEnclagent\x1b[0m  /help for commands, /quit to exit");
                println!();
            }

            loop {
                let prompt = if debug_mode.load(Ordering::Relaxed) {
                    "\x1b[33m[debug]\x1b[0m \x1b[1;36m\u{203A}\x1b[0m "
                } else {
                    "\x1b[1;36m\u{203A}\x1b[0m "
                };

                match rl.readline(prompt) {
                    Ok(line) => {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }

                        // Handle local REPL commands (only commands that need
                        // immediate local handling stay here)
                        match line.to_lowercase().as_str() {
                            "/quit" | "/exit" => break,
                            "/help" => {
                                print_help();
                                continue;
                            }
                            "/debug" => {
                                let current = debug_mode.load(Ordering::Relaxed);
                                debug_mode.store(!current, Ordering::Relaxed);
                                if !current {
                                    println!("\x1b[90mdebug mode on\x1b[0m");
                                } else {
                                    println!("\x1b[90mdebug mode off\x1b[0m");
                                }
                                continue;
                            }
                            _ => {}
                        }

                        let msg = IncomingMessage::new("repl", "default", line);
                        if tx.blocking_send(msg).is_err() {
                            break;
                        }
                    }
                    Err(ReadlineError::Interrupted) => {
                        // Ctrl+C: send /interrupt
                        let msg = IncomingMessage::new("repl", "default", "/interrupt");
                        if tx.blocking_send(msg).is_err() {
                            break;
                        }
                    }
                    Err(ReadlineError::Eof) => {
                        // Ctrl+D: send /quit so the agent loop runs graceful shutdown
                        let msg = IncomingMessage::new("repl", "default", "/quit");
                        let _ = tx.blocking_send(msg);
                        break;
                    }
                    Err(e) => {
                        eprintln!("Input error: {e}");
                        break;
                    }
                }
            }

            // Save history on exit
            let _ = rl.save_history(&history_path());
        });

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn respond(
        &self,
        _msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);

        // If we were streaming, the content was already printed via StreamChunk.
        // Just finish the line and reset.
        if self.is_streaming.swap(false, Ordering::Relaxed) {
            println!();
            println!();
            return Ok(());
        }

        // Dim separator line before the response
        let sep_width = width.min(80);
        eprintln!("\x1b[90m{}\x1b[0m", "\u{2500}".repeat(sep_width));

        // Render markdown
        let skin = make_skin();
        let text = termimad::FmtText::from(&skin, &response.content, Some(width));

        print!("{text}");
        println!();
        Ok(())
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        let debug = self.is_debug();

        match status {
            StatusUpdate::Thinking(msg) => {
                let display = truncate_for_preview(&msg, CLI_STATUS_MAX);
                eprintln!("  \x1b[90m\u{25CB} {display}\x1b[0m");
            }
            StatusUpdate::ToolStarted { name } => {
                eprintln!("  \x1b[33m\u{25CB} {name}\x1b[0m");
            }
            StatusUpdate::ToolCompleted { name, success } => {
                if success {
                    eprintln!("  \x1b[32m\u{25CF} {name}\x1b[0m");
                } else {
                    eprintln!("  \x1b[31m\u{2717} {name} (failed)\x1b[0m");
                }
            }
            StatusUpdate::ToolResult { name: _, preview } => {
                let display = truncate_for_preview(&preview, CLI_TOOL_RESULT_MAX);
                eprintln!("    \x1b[90m{display}\x1b[0m");
            }
            StatusUpdate::StreamChunk(chunk) => {
                // Print separator on the false-to-true transition
                if !self.is_streaming.swap(true, Ordering::Relaxed) {
                    let width = crossterm::terminal::size()
                        .map(|(w, _)| w as usize)
                        .unwrap_or(80);
                    let sep_width = width.min(80);
                    eprintln!("\x1b[90m{}\x1b[0m", "\u{2500}".repeat(sep_width));
                }
                print!("{chunk}");
                let _ = io::stdout().flush();
            }
            StatusUpdate::JobStarted {
                job_id,
                title,
                browse_url,
            } => {
                eprintln!(
                    "  \x1b[36m[job]\x1b[0m {title} \x1b[90m({job_id})\x1b[0m \x1b[4m{browse_url}\x1b[0m"
                );
            }
            StatusUpdate::Status(msg) => {
                if let Some(payload) = parse_ws2_runtime_status(&msg) {
                    render_ws2_runtime_status(&payload);
                } else if debug || msg.contains("approval") || msg.contains("Approval") {
                    let display =
                        truncate_for_preview(&redact_sensitive_preview(&msg), CLI_STATUS_MAX);
                    eprintln!("  \x1b[90m{display}\x1b[0m");
                }
            }
            StatusUpdate::ApprovalNeeded {
                request_id,
                tool_name,
                description,
                parameters,
            } => {
                let term_width = crossterm::terminal::size()
                    .map(|(w, _)| w as usize)
                    .unwrap_or(80);
                let box_width = (term_width.saturating_sub(4)).clamp(40, 60);

                // Short request ID for the bottom border
                let short_id = if request_id.len() > 8 {
                    &request_id[..8]
                } else {
                    &request_id
                };

                // Top border: ┌ tool_name requires approval ───
                let top_label = format!(" {tool_name} requires approval ");
                let top_fill = box_width.saturating_sub(top_label.len() + 1);
                let top_border = format!(
                    "\u{250C}\x1b[33m{top_label}\x1b[0m{}",
                    "\u{2500}".repeat(top_fill)
                );

                // Bottom border: └─ short_id ─────
                let bot_label = format!(" {short_id} ");
                let bot_fill = box_width.saturating_sub(bot_label.len() + 2);
                let bot_border = format!(
                    "\u{2514}\u{2500}\x1b[90m{bot_label}\x1b[0m{}",
                    "\u{2500}".repeat(bot_fill)
                );

                eprintln!();
                eprintln!("  {top_border}");
                eprintln!("  \u{2502} \x1b[90m{description}\x1b[0m");
                eprintln!("  \u{2502}");

                // Params
                let param_lines = format_json_params(&parameters, "  \u{2502}   ");
                // The format_json_params already includes the indent prefix
                // but we need to handle the case where each line already starts with it
                for line in param_lines.lines() {
                    eprintln!("{line}");
                }

                eprintln!("  \u{2502}");
                eprintln!(
                    "  \u{2502} \x1b[32myes\x1b[0m (y) / \x1b[34malways\x1b[0m (a) / \x1b[31mno\x1b[0m (n)"
                );
                eprintln!("  {bot_border}");
                eprintln!();
            }
            StatusUpdate::AuthRequired {
                extension_name,
                instructions,
                setup_url,
                ..
            } => {
                eprintln!();
                eprintln!("\x1b[33m  Authentication required for {extension_name}\x1b[0m");
                if let Some(ref instr) = instructions {
                    eprintln!("  {instr}");
                }
                if let Some(ref url) = setup_url {
                    eprintln!("  \x1b[4m{url}\x1b[0m");
                }
                eprintln!();
            }
            StatusUpdate::AuthCompleted {
                extension_name,
                success,
                message,
            } => {
                if success {
                    eprintln!("\x1b[32m  {extension_name}: {message}\x1b[0m");
                } else {
                    eprintln!("\x1b[31m  {extension_name}: {message}\x1b[0m");
                }
            }
        }
        Ok(())
    }

    async fn broadcast(
        &self,
        _user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let skin = make_skin();
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);

        eprintln!("\x1b[34m\u{25CF}\x1b[0m notification");
        let text = termimad::FmtText::from(&skin, &response.content, Some(width));
        eprint!("{text}");
        eprintln!();
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ws2_runtime_status_payload() {
        let payload = parse_ws2_runtime_status(
            r#"{"kind":"ws2_runtime_status","version":1,"stage":"execution","state":"started","intent":"command.positions","detail":"Dispatching command handler"}"#,
        )
        .expect("ws2 payload should parse");

        assert_eq!(payload.stage, "execution");
        assert_eq!(payload.state, "started");
        assert_eq!(payload.intent.as_deref(), Some("command.positions"));
    }

    #[test]
    fn ignores_non_ws2_status_payloads() {
        let payload = parse_ws2_runtime_status(r#"{"kind":"other_status","stage":"execution"}"#);
        assert!(payload.is_none());
    }

    #[test]
    fn formats_structured_incident_lines() {
        let payload = Ws2RuntimeStatusPayload {
            kind: WS2_RUNTIME_STATUS_KIND.to_string(),
            version: Some(1),
            stage: "verification".to_string(),
            state: "failed".to_string(),
            intent: Some("chat.message".to_string()),
            detail: Some("Verification blocked".to_string()),
            error: Some(Ws2RuntimeErrorPayload {
                domain: "verification".to_string(),
                code: "verification.safety_failed".to_string(),
                retryable: false,
                message: "Validation failed: policy violation".to_string(),
            }),
        };

        let lines = build_ws2_runtime_status_lines(&payload);
        let rendered = lines.join("\n");

        assert!(rendered.contains("verification: failed"));
        assert!(rendered.contains("incident verification"));
        assert!(rendered.contains("code:"));
        assert!(rendered.contains("retryable:"));
        assert!(rendered.contains("message:"));
    }

    #[test]
    fn redacts_sensitive_preview_tokens() {
        let message =
            "auth failed bearer abc.def.ghi token=abc123 api_key: xyz987 password=letmein";
        let redacted = redact_sensitive_preview(message);
        assert!(!redacted.contains("abc.def.ghi"));
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("xyz987"));
        assert!(!redacted.contains("letmein"));
    }
}
