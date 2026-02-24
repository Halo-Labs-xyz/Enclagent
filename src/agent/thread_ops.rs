//! Thread and session operations for the agent.
//!
//! Extracted from `agent_loop.rs` to isolate thread management (user input
//! processing, undo/redo, approval, auth, persistence) from the core loop.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::compaction::ContextCompactor;
use crate::agent::dispatcher::{AgenticLoopResult, detect_auth_awaiting, parse_auth_result};
use crate::agent::session::{Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::context::JobContext;
use crate::error::{
    Error, LlmError, RuntimeStage, RuntimeStageState, RuntimeStatusPayload, SafetyError,
};
use crate::llm::ChatMessage;

impl Agent {
    async fn send_runtime_status(&self, message: &IncomingMessage, payload: RuntimeStatusPayload) {
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::Status(payload.to_status_line()),
                &message.metadata,
            )
            .await;
    }

    async fn send_runtime_stage(
        &self,
        message: &IncomingMessage,
        stage: RuntimeStage,
        state: RuntimeStageState,
        intent: Option<&str>,
        detail: Option<&str>,
    ) {
        let mut payload = RuntimeStatusPayload::new(stage, state);
        if let Some(intent) = intent {
            payload = payload.with_intent(intent);
        }
        if let Some(detail) = detail {
            payload = payload.with_detail(detail);
        }
        self.send_runtime_status(message, payload).await;
    }

    async fn send_runtime_error(
        &self,
        message: &IncomingMessage,
        stage: RuntimeStage,
        error: &Error,
        intent: Option<&str>,
    ) {
        let mut payload = error.to_runtime_status_payload(stage);
        if let Some(intent) = intent {
            payload = payload.with_intent(intent);
        }
        self.send_runtime_status(message, payload).await;
    }

    /// Hydrate a historical thread from DB into memory if not already present.
    ///
    /// Called before `resolve_thread` so that the session manager finds the
    /// thread on lookup instead of creating a new one.
    ///
    /// Creates an in-memory thread with the exact UUID the frontend sent,
    /// even when the conversation has zero messages (e.g. a brand-new
    /// assistant thread). Without this, `resolve_thread` would mint a
    /// fresh UUID and all messages would land in the wrong conversation.
    pub(super) async fn maybe_hydrate_thread(
        &self,
        message: &IncomingMessage,
        external_thread_id: &str,
    ) {
        // Only hydrate UUID-shaped thread IDs (web gateway uses UUIDs)
        let thread_uuid = match Uuid::parse_str(external_thread_id) {
            Ok(id) => id,
            Err(_) => return,
        };

        // Check if already in memory
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        {
            let sess = session.lock().await;
            if sess.threads.contains_key(&thread_uuid) {
                return;
            }
        }

        // Load history from DB (may be empty for a newly created thread).
        let mut chat_messages: Vec<ChatMessage> = Vec::new();
        let msg_count;

        if let Some(store) = self.store() {
            let db_messages = store
                .list_conversation_messages(thread_uuid)
                .await
                .unwrap_or_default();
            msg_count = db_messages.len();
            chat_messages = db_messages
                .iter()
                .filter_map(|m| match m.role.as_str() {
                    "user" => Some(ChatMessage::user(&m.content)),
                    "assistant" => Some(ChatMessage::assistant(&m.content)),
                    _ => None,
                })
                .collect();
        } else {
            msg_count = 0;
        }

        // Create thread with the historical ID and restore messages
        let session_id = {
            let sess = session.lock().await;
            sess.id
        };

        let mut thread = crate::agent::session::Thread::with_id(thread_uuid, session_id);
        if !chat_messages.is_empty() {
            thread.restore_from_messages(chat_messages);
        }

        // Restore response chain from conversation metadata
        if let Some(store) = self.store()
            && let Ok(Some(metadata)) = store.get_conversation_metadata(thread_uuid).await
            && let Some(rid) = metadata
                .get("last_response_id")
                .and_then(|v| v.as_str())
                .map(String::from)
        {
            thread.last_response_id = Some(rid.clone());
            self.llm()
                .seed_response_chain(&thread_uuid.to_string(), rid);
            tracing::debug!("Restored response chain for thread {}", thread_uuid);
        }

        // Insert into session and register with session manager
        {
            let mut sess = session.lock().await;
            sess.threads.insert(thread_uuid, thread);
            sess.active_thread = Some(thread_uuid);
            sess.last_active_at = chrono::Utc::now();
        }

        self.session_manager
            .register_thread(
                &message.user_id,
                &message.channel,
                thread_uuid,
                Arc::clone(&session),
            )
            .await;

        tracing::debug!(
            "Hydrated thread {} from DB ({} messages)",
            thread_uuid,
            msg_count
        );
    }

    pub(super) async fn process_user_input(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        content: &str,
    ) -> Result<SubmissionResult, Error> {
        // First check thread state without holding lock during I/O
        let thread_state = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.state
        };

        // Check thread state
        match thread_state {
            ThreadState::Processing => {
                return Ok(SubmissionResult::error(
                    "Turn in progress. Use /interrupt to cancel.",
                ));
            }
            ThreadState::AwaitingApproval => {
                return Ok(SubmissionResult::error(
                    "Waiting for approval. Use /interrupt to cancel.",
                ));
            }
            ThreadState::Completed => {
                return Ok(SubmissionResult::error(
                    "Thread completed. Use /thread new.",
                ));
            }
            ThreadState::Idle | ThreadState::Interrupted => {
                // Can proceed
            }
        }

        self.send_runtime_stage(
            message,
            RuntimeStage::Intent,
            RuntimeStageState::Started,
            None,
            Some("Classifying incoming message intent"),
        )
        .await;

        // Safety validation for user input
        let validation = self.safety().validate_input(content);
        if !validation.is_valid {
            let details = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            let error = Error::from(SafetyError::ValidationFailed {
                reason: details.clone(),
            });
            self.send_runtime_error(message, RuntimeStage::Verification, &error, None)
                .await;
            return Ok(SubmissionResult::error(format!(
                "Input rejected by safety validation: {}",
                details
            )));
        }

        let violations = self.safety().check_policy(content);
        if violations
            .iter()
            .any(|rule| rule.action == crate::safety::PolicyAction::Block)
        {
            let policy = violations
                .iter()
                .find(|rule| rule.action == crate::safety::PolicyAction::Block)
                .map(|rule| rule.id.as_str())
                .unwrap_or("unknown");
            let error = Error::from(SafetyError::PolicyViolation {
                rule: policy.to_string(),
            });
            self.send_runtime_error(message, RuntimeStage::Verification, &error, None)
                .await;
            return Ok(SubmissionResult::error("Input rejected by safety policy."));
        }

        // Handle explicit commands (starting with /) directly
        // Everything else goes through the normal agentic loop with tools
        let temp_message = IncomingMessage {
            content: content.to_string(),
            ..message.clone()
        };

        if let Some(intent) = self.router.route_command(&temp_message) {
            let intent_label = intent.status_label();
            self.send_runtime_stage(
                message,
                RuntimeStage::Intent,
                RuntimeStageState::Completed,
                Some(&intent_label),
                Some("Parsed explicit command"),
            )
            .await;
            self.send_runtime_stage(
                message,
                RuntimeStage::Execution,
                RuntimeStageState::Started,
                Some(&intent_label),
                Some("Dispatching command handler"),
            )
            .await;

            // Explicit command like /status, /job, /list - handle directly
            let result = self.handle_job_or_command(intent, message).await;
            match &result {
                Ok(SubmissionResult::NeedApproval { .. }) => {
                    self.send_runtime_stage(
                        message,
                        RuntimeStage::Execution,
                        RuntimeStageState::AwaitingInput,
                        Some(&intent_label),
                        Some("Awaiting approval decision"),
                    )
                    .await;
                }
                Ok(SubmissionResult::Error { message: msg }) => {
                    self.send_runtime_stage(
                        message,
                        RuntimeStage::Execution,
                        RuntimeStageState::Failed,
                        Some(&intent_label),
                        Some(msg),
                    )
                    .await;
                }
                Ok(SubmissionResult::Interrupted) => {
                    self.send_runtime_stage(
                        message,
                        RuntimeStage::Execution,
                        RuntimeStageState::Blocked,
                        Some(&intent_label),
                        Some("Execution interrupted"),
                    )
                    .await;
                }
                Ok(_) => {
                    self.send_runtime_stage(
                        message,
                        RuntimeStage::Execution,
                        RuntimeStageState::Completed,
                        Some(&intent_label),
                        Some("Command execution finished"),
                    )
                    .await;
                }
                Err(e) => {
                    self.send_runtime_error(
                        message,
                        RuntimeStage::Execution,
                        e,
                        Some(&intent_label),
                    )
                    .await;
                }
            }
            return result;
        }

        self.send_runtime_stage(
            message,
            RuntimeStage::Intent,
            RuntimeStageState::Completed,
            Some("chat.message"),
            Some("Parsed natural-language input"),
        )
        .await;

        // Natural language goes through the agentic loop
        // Job tools (create_job, list_jobs, etc.) are in the tool registry

        // Auto-compact if needed BEFORE adding new turn
        {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            let messages = thread.messages();
            if let Some(strategy) = self.context_monitor.suggest_compaction(&messages) {
                let pct = self.context_monitor.usage_percent(&messages);
                tracing::info!("Context at {:.1}% capacity, auto-compacting", pct);

                // Notify the user that compaction is happening
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status(format!(
                            "Context at {:.0}% capacity, compacting...",
                            pct
                        )),
                        &message.metadata,
                    )
                    .await;

                let compactor = ContextCompactor::new(self.llm().clone());
                if let Err(e) = compactor
                    .compact(thread, strategy, self.workspace().map(|w| w.as_ref()))
                    .await
                {
                    tracing::warn!("Auto-compaction failed: {}", e);
                }
            }
        }

        // Create checkpoint before turn
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            let mut mgr = undo_mgr.lock().await;
            mgr.checkpoint(
                thread.turn_number(),
                thread.messages(),
                format!("Before turn {}", thread.turn_number()),
            );
        }

        // Start the turn and get messages
        let turn_messages = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.start_turn(content);
            thread.messages()
        };

        self.send_runtime_stage(
            message,
            RuntimeStage::Execution,
            RuntimeStageState::Started,
            Some("chat.message"),
            Some("Running execution loop"),
        )
        .await;

        // Send thinking status
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::Thinking("Execution stage: planning and running tools...".into()),
                &message.metadata,
            )
            .await;

        // Run the agentic tool execution loop
        let result = self
            .run_agentic_loop(message, session.clone(), thread_id, turn_messages, false)
            .await;

        // Re-acquire lock and check if interrupted
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        if thread.state == ThreadState::Interrupted {
            self.send_runtime_stage(
                message,
                RuntimeStage::Execution,
                RuntimeStageState::Blocked,
                Some("chat.message"),
                Some("Execution interrupted"),
            )
            .await;
            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Status("Interrupted".into()),
                    &message.metadata,
                )
                .await;
            return Ok(SubmissionResult::Interrupted);
        }

        // Complete, fail, or request approval
        match result {
            Ok(AgenticLoopResult::Response(response)) => {
                self.send_runtime_stage(
                    message,
                    RuntimeStage::Execution,
                    RuntimeStageState::Completed,
                    Some("chat.message"),
                    Some("Execution completed, verifying response"),
                )
                .await;
                self.send_runtime_stage(
                    message,
                    RuntimeStage::Verification,
                    RuntimeStageState::Started,
                    Some("chat.message"),
                    Some("Running output verification hooks"),
                )
                .await;

                // Hook: TransformResponse — allow hooks to modify or reject the final response
                let (response, verification_state, verification_detail) = {
                    let event = crate::hooks::HookEvent::ResponseTransform {
                        user_id: message.user_id.clone(),
                        thread_id: thread_id.to_string(),
                        response: response.clone(),
                    };
                    match self.hooks().run(&event).await {
                        Err(crate::hooks::HookError::Rejected { reason }) => (
                            format!("[Response filtered: {}]", reason),
                            RuntimeStageState::Blocked,
                            format!("Response filtered by hook: {}", reason),
                        ),
                        Err(err) => (
                            format!("[Response blocked by hook policy: {}]", err),
                            RuntimeStageState::Blocked,
                            format!("Response blocked by hook policy: {}", err),
                        ),
                        Ok(crate::hooks::HookOutcome::Continue {
                            modified: Some(new_response),
                        }) => (
                            new_response,
                            RuntimeStageState::Completed,
                            "Response transformed by hook".to_string(),
                        ),
                        _ => (
                            response,
                            RuntimeStageState::Completed,
                            "Response passed verification".to_string(),
                        ), // fail-open: use original
                    }
                };
                self.send_runtime_stage(
                    message,
                    RuntimeStage::Verification,
                    verification_state,
                    Some("chat.message"),
                    Some(&verification_detail),
                )
                .await;

                thread.complete_turn(&response);
                self.persist_response_chain(thread);
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status("Done".into()),
                        &message.metadata,
                    )
                    .await;

                // Fire-and-forget: persist turn to DB
                self.persist_turn(thread_id, &message.user_id, content, Some(&response));

                Ok(SubmissionResult::response(response))
            }
            Ok(AgenticLoopResult::NeedApproval { pending }) => {
                self.send_runtime_stage(
                    message,
                    RuntimeStage::Execution,
                    RuntimeStageState::AwaitingInput,
                    Some("chat.message"),
                    Some("Tool execution requires approval"),
                )
                .await;
                // Store pending approval in thread and update state
                let request_id = pending.request_id;
                let tool_name = pending.tool_name.clone();
                let description = pending.description.clone();
                let parameters = pending.parameters.clone();
                thread.await_approval(pending);
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status("Awaiting approval".into()),
                        &message.metadata,
                    )
                    .await;
                Ok(SubmissionResult::NeedApproval {
                    request_id,
                    tool_name,
                    description,
                    parameters,
                })
            }
            Err(e) => {
                self.send_runtime_error(message, RuntimeStage::Execution, &e, Some("chat.message"))
                    .await;
                thread.fail_turn(e.to_string());

                // Persist the user message even on failure
                self.persist_turn(thread_id, &message.user_id, content, None);

                Ok(SubmissionResult::error(e.to_string()))
            }
        }
    }

    /// Fire-and-forget: persist a turn (user message + optional assistant response) to the DB.
    pub(super) fn persist_turn(
        &self,
        thread_id: Uuid,
        user_id: &str,
        user_input: &str,
        response: Option<&str>,
    ) {
        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };

        let user_id = user_id.to_string();
        let user_input = user_input.to_string();
        let response = response.map(String::from);

        tokio::spawn(async move {
            if let Err(e) = store
                .ensure_conversation(thread_id, "gateway", &user_id, None)
                .await
            {
                tracing::warn!("Failed to ensure conversation {}: {}", thread_id, e);
                return;
            }

            if let Err(e) = store
                .add_conversation_message(thread_id, "user", &user_input)
                .await
            {
                tracing::warn!("Failed to persist user message: {}", e);
                return;
            }

            if let Some(ref resp) = response
                && let Err(e) = store
                    .add_conversation_message(thread_id, "assistant", resp)
                    .await
            {
                tracing::warn!("Failed to persist assistant message: {}", e);
            }
        });
    }

    /// Sync the provider's response chain ID to the thread and DB metadata.
    ///
    /// Call after a successful agentic loop to persist the latest
    /// `previous_response_id` so chaining survives restarts.
    pub(super) fn persist_response_chain(&self, thread: &mut crate::agent::session::Thread) {
        let tid = thread.id.to_string();
        let response_id = match self.llm().get_response_chain_id(&tid) {
            Some(rid) => rid,
            None => return,
        };

        // Update in-memory thread
        thread.last_response_id = Some(response_id.clone());

        // Fire-and-forget DB write
        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };
        let thread_id = thread.id;
        tokio::spawn(async move {
            let val = serde_json::json!(response_id);
            if let Err(e) = store
                .update_conversation_metadata_field(thread_id, "last_response_id", &val)
                .await
            {
                tracing::warn!(
                    "Failed to persist response chain for thread {}: {}",
                    thread_id,
                    e
                );
            }
        });
    }

    pub(super) async fn process_undo(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if !mgr.can_undo() {
            return Ok(SubmissionResult::ok_with_message("Nothing to undo."));
        }

        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        // Save current state to redo, get previous checkpoint
        let current_messages = thread.messages();
        let current_turn = thread.turn_number();

        if let Some(checkpoint) = mgr.undo(current_turn, current_messages) {
            // Extract values before consuming the reference
            let turn_number = checkpoint.turn_number;
            let messages = checkpoint.messages.clone();
            let undo_count = mgr.undo_count();
            // Restore thread from checkpoint
            thread.restore_from_messages(messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Undone to turn {}. {} undo(s) remaining.",
                turn_number, undo_count
            )))
        } else {
            Ok(SubmissionResult::error("Undo failed."))
        }
    }

    pub(super) async fn process_redo(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if !mgr.can_redo() {
            return Ok(SubmissionResult::ok_with_message("Nothing to redo."));
        }

        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        let current_messages = thread.messages();
        let current_turn = thread.turn_number();

        if let Some(checkpoint) = mgr.redo(current_turn, current_messages) {
            thread.restore_from_messages(checkpoint.messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Redone to turn {}.",
                checkpoint.turn_number
            )))
        } else {
            Ok(SubmissionResult::error("Redo failed."))
        }
    }

    pub(super) async fn process_interrupt(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        match thread.state {
            ThreadState::Processing | ThreadState::AwaitingApproval => {
                thread.interrupt();
                Ok(SubmissionResult::ok_with_message("Interrupted."))
            }
            _ => Ok(SubmissionResult::ok_with_message("Nothing to interrupt.")),
        }
    }

    pub(super) async fn process_compact(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        let messages = thread.messages();
        let usage = self.context_monitor.usage_percent(&messages);
        let strategy = self
            .context_monitor
            .suggest_compaction(&messages)
            .unwrap_or(
                crate::agent::context_monitor::CompactionStrategy::Summarize { keep_recent: 5 },
            );

        let compactor = ContextCompactor::new(self.llm().clone());
        match compactor
            .compact(thread, strategy, self.workspace().map(|w| w.as_ref()))
            .await
        {
            Ok(result) => {
                let mut msg = format!(
                    "Compacted: {} turns removed, {} → {} tokens (was {:.1}% full)",
                    result.turns_removed, result.tokens_before, result.tokens_after, usage
                );
                if result.summary_written {
                    msg.push_str(", summary saved to workspace");
                }
                Ok(SubmissionResult::ok_with_message(msg))
            }
            Err(e) => Ok(SubmissionResult::error(format!("Compaction failed: {}", e))),
        }
    }

    pub(super) async fn process_clear(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
        thread.turns.clear();
        thread.state = ThreadState::Idle;

        // Clear undo history too
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        undo_mgr.lock().await.clear();

        Ok(SubmissionResult::ok_with_message("Thread cleared."))
    }

    /// Process an approval or rejection of a pending tool execution.
    pub(super) async fn process_approval(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        request_id: Option<Uuid>,
        approved: bool,
        always: bool,
    ) -> Result<SubmissionResult, Error> {
        // Get thread state and pending approval
        let (_thread_state, pending) = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            if thread.state != ThreadState::AwaitingApproval {
                return Ok(SubmissionResult::error("No pending approval request."));
            }

            let pending = thread.take_pending_approval();
            (thread.state, pending)
        };

        let pending = match pending {
            Some(p) => p,
            None => return Ok(SubmissionResult::error("No pending approval request.")),
        };

        // Verify request ID if provided
        if let Some(req_id) = request_id
            && req_id != pending.request_id
        {
            // Put it back and return error
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
                thread.await_approval(pending);
            }
            self.send_runtime_stage(
                message,
                RuntimeStage::Verification,
                RuntimeStageState::Failed,
                Some("approval.response"),
                Some("Approval request id mismatch"),
            )
            .await;
            return Ok(SubmissionResult::error(
                "Request ID mismatch. Use the correct request ID.",
            ));
        }

        if approved {
            self.send_runtime_stage(
                message,
                RuntimeStage::Execution,
                RuntimeStageState::Started,
                Some("approval.execution"),
                Some("Executing approved tool call"),
            )
            .await;

            // If always, add to auto-approved set
            if always {
                let mut sess = session.lock().await;
                sess.auto_approve_tool(&pending.tool_name);
                tracing::info!(
                    "Auto-approved tool '{}' for session {}",
                    pending.tool_name,
                    sess.id
                );
            }

            // Reset thread state to processing
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id) {
                    thread.state = ThreadState::Processing;
                }
            }

            // Execute the approved tool and continue the loop
            let job_ctx =
                JobContext::with_user(&message.user_id, "chat", "Interactive chat session");

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::ToolStarted {
                        name: pending.tool_name.clone(),
                    },
                    &message.metadata,
                )
                .await;

            let tool_result = self
                .execute_chat_tool(&pending.tool_name, &pending.parameters, &job_ctx)
                .await;

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::ToolCompleted {
                        name: pending.tool_name.clone(),
                        success: tool_result.is_ok(),
                    },
                    &message.metadata,
                )
                .await;

            if let Ok(ref output) = tool_result
                && !output.is_empty()
            {
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::ToolResult {
                            name: pending.tool_name.clone(),
                            preview: output.clone(),
                        },
                        &message.metadata,
                    )
                    .await;
            }

            // Build context including the tool result
            let mut context_messages = pending.context_messages;

            // Record result in thread
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id)
                    && let Some(turn) = thread.last_turn_mut()
                {
                    match &tool_result {
                        Ok(output) => {
                            turn.record_tool_result(serde_json::json!(output));
                        }
                        Err(e) => {
                            turn.record_tool_error(e.to_string());
                        }
                    }
                }
            }

            // If tool_auth returned awaiting_token, enter auth mode and
            // return instructions directly (skip agentic loop continuation).
            if let Some((ext_name, instructions)) =
                detect_auth_awaiting(&pending.tool_name, &tool_result)
            {
                self.send_runtime_stage(
                    message,
                    RuntimeStage::Execution,
                    RuntimeStageState::AwaitingInput,
                    Some("approval.execution"),
                    Some("Execution paused pending extension authentication"),
                )
                .await;
                let auth_data = parse_auth_result(&tool_result);
                {
                    let mut sess = session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&thread_id) {
                        thread.enter_auth_mode(ext_name.clone());
                        thread.complete_turn(&instructions);
                    }
                }
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::AuthRequired {
                            extension_name: ext_name,
                            instructions: Some(instructions.clone()),
                            auth_url: auth_data.auth_url,
                            setup_url: auth_data.setup_url,
                        },
                        &message.metadata,
                    )
                    .await;
                return Ok(SubmissionResult::response(instructions));
            }

            if let Err(ref err) = tool_result {
                self.send_runtime_error(
                    message,
                    RuntimeStage::Execution,
                    err,
                    Some("approval.execution"),
                )
                .await;
            }

            // Add tool result to context
            let result_content = match tool_result {
                Ok(output) => {
                    let sanitized = self
                        .safety()
                        .sanitize_tool_output(&pending.tool_name, &output);
                    self.safety().wrap_for_llm(
                        &pending.tool_name,
                        &sanitized.content,
                        sanitized.was_modified,
                    )
                }
                Err(e) => format!("Error: {}", e),
            };

            context_messages.push(ChatMessage::tool_result(
                &pending.tool_call_id,
                &pending.tool_name,
                result_content,
            ));

            // Continue the agentic loop (a tool was already executed this turn)
            let result = self
                .run_agentic_loop(message, session.clone(), thread_id, context_messages, true)
                .await;

            // Handle the result
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            match result {
                Ok(AgenticLoopResult::Response(response)) => {
                    self.send_runtime_stage(
                        message,
                        RuntimeStage::Execution,
                        RuntimeStageState::Completed,
                        Some("approval.execution"),
                        Some("Execution completed, verifying response"),
                    )
                    .await;
                    self.send_runtime_stage(
                        message,
                        RuntimeStage::Verification,
                        RuntimeStageState::Started,
                        Some("approval.execution"),
                        Some("Running response verification"),
                    )
                    .await;
                    self.send_runtime_stage(
                        message,
                        RuntimeStage::Verification,
                        RuntimeStageState::Completed,
                        Some("approval.execution"),
                        Some("Verification completed"),
                    )
                    .await;
                    thread.complete_turn(&response);
                    self.persist_response_chain(thread);
                    let _ = self
                        .channels
                        .send_status(
                            &message.channel,
                            StatusUpdate::Status("Done".into()),
                            &message.metadata,
                        )
                        .await;
                    Ok(SubmissionResult::response(response))
                }
                Ok(AgenticLoopResult::NeedApproval {
                    pending: new_pending,
                }) => {
                    self.send_runtime_stage(
                        message,
                        RuntimeStage::Execution,
                        RuntimeStageState::AwaitingInput,
                        Some("approval.execution"),
                        Some("Additional approval required"),
                    )
                    .await;
                    let request_id = new_pending.request_id;
                    let tool_name = new_pending.tool_name.clone();
                    let description = new_pending.description.clone();
                    let parameters = new_pending.parameters.clone();
                    thread.await_approval(new_pending);
                    let _ = self
                        .channels
                        .send_status(
                            &message.channel,
                            StatusUpdate::Status("Awaiting approval".into()),
                            &message.metadata,
                        )
                        .await;
                    Ok(SubmissionResult::NeedApproval {
                        request_id,
                        tool_name,
                        description,
                        parameters,
                    })
                }
                Err(e) => {
                    self.send_runtime_error(
                        message,
                        RuntimeStage::Execution,
                        &e,
                        Some("approval.execution"),
                    )
                    .await;
                    thread.fail_turn(e.to_string());
                    Ok(SubmissionResult::error(e.to_string()))
                }
            }
        } else {
            // Rejected - clear approval and return to idle
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id) {
                    thread.clear_pending_approval();
                }
            }

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Status("Rejected".into()),
                    &message.metadata,
                )
                .await;
            self.send_runtime_stage(
                message,
                RuntimeStage::Execution,
                RuntimeStageState::Blocked,
                Some("approval.execution"),
                Some("User rejected tool execution"),
            )
            .await;

            Ok(SubmissionResult::response(format!(
                "Tool '{}' was rejected. The agent will not execute this tool.\n\n\
                 You can continue the conversation or try a different approach.",
                pending.tool_name
            )))
        }
    }

    /// Handle an auth token submitted while the thread is in auth mode.
    ///
    /// The token goes directly to the extension manager's credential store,
    /// completely bypassing logging, turn creation, history, and compaction.
    pub(super) async fn process_auth_token(
        &self,
        message: &IncomingMessage,
        pending: &crate::agent::session::PendingAuth,
        token: &str,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<Option<String>, Error> {
        let token = token.trim();
        self.send_runtime_stage(
            message,
            RuntimeStage::Execution,
            RuntimeStageState::Started,
            Some("auth.token"),
            Some("Submitting authentication token"),
        )
        .await;

        // Clear auth mode regardless of outcome
        {
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
                thread.pending_auth = None;
            }
        }

        let ext_mgr = match self.deps.extension_manager.as_ref() {
            Some(mgr) => mgr,
            None => {
                let error = Error::from(LlmError::AuthFailed {
                    provider: pending.extension_name.clone(),
                });
                self.send_runtime_error(
                    message,
                    RuntimeStage::Execution,
                    &error,
                    Some("auth.token"),
                )
                .await;
                return Ok(Some("Extension manager not available.".to_string()));
            }
        };

        match ext_mgr.auth(&pending.extension_name, Some(token)).await {
            Ok(result) if result.status == "authenticated" => {
                tracing::info!(
                    "Extension '{}' authenticated via auth mode",
                    pending.extension_name
                );

                // Auto-activate so tools are available immediately after auth
                match ext_mgr.activate(&pending.extension_name).await {
                    Ok(activate_result) => {
                        let tool_count = activate_result.tools_loaded.len();
                        let tool_list = if activate_result.tools_loaded.is_empty() {
                            String::new()
                        } else {
                            format!("\n\nTools: {}", activate_result.tools_loaded.join(", "))
                        };
                        let msg = format!(
                            "{} authenticated and activated ({} tools loaded).{}",
                            pending.extension_name, tool_count, tool_list
                        );
                        let _ = self
                            .channels
                            .send_status(
                                &message.channel,
                                StatusUpdate::AuthCompleted {
                                    extension_name: pending.extension_name.clone(),
                                    success: true,
                                    message: msg.clone(),
                                },
                                &message.metadata,
                            )
                            .await;
                        self.send_runtime_stage(
                            message,
                            RuntimeStage::Execution,
                            RuntimeStageState::Completed,
                            Some("auth.token"),
                            Some("Authentication and activation completed"),
                        )
                        .await;
                        Ok(Some(msg))
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Extension '{}' authenticated but activation failed: {}",
                            pending.extension_name,
                            e
                        );
                        let msg = format!(
                            "{} authenticated successfully, but activation failed: {}. \
                             Try activating manually.",
                            pending.extension_name, e
                        );
                        let _ = self
                            .channels
                            .send_status(
                                &message.channel,
                                StatusUpdate::AuthCompleted {
                                    extension_name: pending.extension_name.clone(),
                                    success: true,
                                    message: msg.clone(),
                                },
                                &message.metadata,
                            )
                            .await;
                        let error = Error::from(LlmError::SessionRenewalFailed {
                            provider: pending.extension_name.clone(),
                            reason: e.to_string(),
                        });
                        self.send_runtime_error(
                            message,
                            RuntimeStage::Execution,
                            &error,
                            Some("auth.token"),
                        )
                        .await;
                        Ok(Some(msg))
                    }
                }
            }
            Ok(result) => {
                // Invalid token, re-enter auth mode
                {
                    let mut sess = session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&thread_id) {
                        thread.enter_auth_mode(pending.extension_name.clone());
                    }
                }
                let msg = result
                    .instructions
                    .clone()
                    .unwrap_or_else(|| "Invalid token. Please try again.".to_string());
                // Re-emit AuthRequired so web UI re-shows the card
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::AuthRequired {
                            extension_name: pending.extension_name.clone(),
                            instructions: Some(msg.clone()),
                            auth_url: result.auth_url,
                            setup_url: result.setup_url,
                        },
                        &message.metadata,
                    )
                    .await;
                let error = Error::from(LlmError::SessionRenewalFailed {
                    provider: pending.extension_name.clone(),
                    reason: msg.clone(),
                });
                self.send_runtime_error(
                    message,
                    RuntimeStage::Execution,
                    &error,
                    Some("auth.token"),
                )
                .await;
                Ok(Some(msg))
            }
            Err(e) => {
                let msg = format!(
                    "Authentication failed for {}: {}",
                    pending.extension_name, e
                );
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::AuthCompleted {
                            extension_name: pending.extension_name.clone(),
                            success: false,
                            message: msg.clone(),
                        },
                        &message.metadata,
                    )
                    .await;
                let error = Error::from(LlmError::SessionRenewalFailed {
                    provider: pending.extension_name.clone(),
                    reason: e.to_string(),
                });
                self.send_runtime_error(
                    message,
                    RuntimeStage::Execution,
                    &error,
                    Some("auth.token"),
                )
                .await;
                Ok(Some(msg))
            }
        }
    }

    pub(super) async fn process_new_thread(
        &self,
        message: &IncomingMessage,
    ) -> Result<SubmissionResult, Error> {
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        let mut sess = session.lock().await;
        let thread = sess.create_thread();
        let thread_id = thread.id;
        Ok(SubmissionResult::ok_with_message(format!(
            "New thread: {}",
            thread_id
        )))
    }

    pub(super) async fn process_switch_thread(
        &self,
        message: &IncomingMessage,
        target_thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        let mut sess = session.lock().await;

        if sess.switch_thread(target_thread_id) {
            Ok(SubmissionResult::ok_with_message(format!(
                "Switched to thread {}",
                target_thread_id
            )))
        } else {
            Ok(SubmissionResult::error("Thread not found."))
        }
    }

    pub(super) async fn process_resume(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        checkpoint_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if let Some(checkpoint) = mgr.restore(checkpoint_id) {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.restore_from_messages(checkpoint.messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Resumed from checkpoint: {}",
                checkpoint.description
            )))
        } else {
            Ok(SubmissionResult::error("Checkpoint not found."))
        }
    }
}
