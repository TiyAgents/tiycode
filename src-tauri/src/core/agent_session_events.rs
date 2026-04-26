use std::sync::Mutex as StdMutex;

use tiycore::agent::AgentMessage;
use tiycore::types::{AssistantMessageEvent, Usage};
use tokio::sync::mpsc;

use crate::core::agent_session_compression::{
    observe_context_usage_calibration, ContextCompressionRuntimeState,
};
use crate::ipc::frontend_channels::ThreadStreamEvent;
use crate::model::thread::RunUsageDto;

pub(crate) fn handle_agent_event(
    run_id: &str,
    event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
    current_message_id: &StdMutex<Option<String>>,
    last_completed_message_id: &StdMutex<Option<String>>,
    current_reasoning_message_id: &StdMutex<Option<String>>,
    last_usage: &StdMutex<Option<Usage>>,
    context_compression_state: &StdMutex<ContextCompressionRuntimeState>,
    reasoning_buffer: &StdMutex<String>,
    current_turn_index: &StdMutex<Option<usize>>,
    context_window: &str,
    model_display_name: &str,
    event: &tiycore::agent::AgentEvent,
) {
    match event {
        tiycore::agent::AgentEvent::TurnRetrying {
            attempt,
            max_attempts,
            delay_ms,
            reason,
        } => {
            let _ = event_tx.send(ThreadStreamEvent::RunRetrying {
                run_id: run_id.to_string(),
                attempt: *attempt,
                max_attempts: *max_attempts,
                delay_ms: *delay_ms,
                reason: reason.clone(),
            });
        }
        tiycore::agent::AgentEvent::MessageUpdate {
            assistant_event,
            turn_index,
            ..
        } => {
            // Track the current turn_index for response boundary grouping
            if let Ok(mut guard) = current_turn_index.lock() {
                *guard = Some(*turn_index);
            }
            match assistant_event.as_ref() {
                AssistantMessageEvent::TextDelta { delta, .. } => {
                    let message_id = ensure_message_id(current_message_id);
                    let _ = event_tx.send(ThreadStreamEvent::MessageDelta {
                        run_id: run_id.to_string(),
                        message_id,
                        delta: delta.clone(),
                    });
                }
                AssistantMessageEvent::ThinkingStart { .. } => {
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                }
                AssistantMessageEvent::ThinkingDelta { delta, .. } => {
                    if let Ok(mut buffer) = reasoning_buffer.lock() {
                        buffer.push_str(delta);
                        let message_id = ensure_message_id(current_reasoning_message_id);
                        let ti = current_turn_index.lock().ok().and_then(|g| *g);
                        let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                            run_id: run_id.to_string(),
                            message_id,
                            reasoning: buffer.clone(),
                            thinking_signature: None,
                            turn_index: ti,
                        });
                    }
                }
                AssistantMessageEvent::ThinkingEnd {
                    content, partial, ..
                } => {
                    let reasoning = if let Ok(mut buffer) = reasoning_buffer.lock() {
                        buffer.clear();
                        buffer.push_str(content);
                        buffer.clone()
                    } else {
                        content.clone()
                    };

                    if reasoning.trim().is_empty() {
                        reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                        return;
                    }

                    // Extract thinking_signature from the partial message's last
                    // Thinking content block.  The signature is populated by the
                    // protocol layer during streaming and is complete by the time
                    // ThinkingEnd fires.
                    let thinking_signature = partial
                        .content
                        .iter()
                        .rev()
                        .find_map(|b| b.as_thinking())
                        .and_then(|t| t.thinking_signature.clone());

                    let message_id = ensure_message_id(current_reasoning_message_id);
                    let ti = current_turn_index.lock().ok().and_then(|g| *g);
                    let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                        run_id: run_id.to_string(),
                        message_id,
                        reasoning,
                        thinking_signature,
                        turn_index: ti,
                    });
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                }
                _ => {}
            }

            if let Some(partial) = assistant_event.partial_message() {
                emit_usage_update_if_changed(
                    run_id,
                    event_tx,
                    last_usage,
                    context_compression_state,
                    &partial.usage,
                    context_window,
                    model_display_name,
                );
            }
        }
        tiycore::agent::AgentEvent::MessageEnd { message, .. } => {
            if let AgentMessage::Assistant(assistant) = message {
                let content = assistant.text_content();

                // Skip emitting MessageCompleted when the assistant produced
                // no usable text content.  Two sub-cases:
                //
                // a) Empty content WITH tool calls — the tool-call-only path.
                //    Tool calls are persisted separately; no plain_message needed.
                //
                // b) Empty content WITHOUT tool calls — typically a provider
                //    error (transport error, 500, 403, etc.) that interrupted
                //    the stream before any text was generated.  Persisting an
                //    empty plain_message would poison the history: on the next
                //    run, convert_history_messages creates an AssistantMessage
                //    with only a Text("") block; tiycore serialises it with
                //    `content: null` (the empty text is filtered) while
                //    reasoning_content may be present, causing DeepSeek to
                //    reject the request with 400.
                if content.is_empty() {
                    emit_usage_update_if_changed(
                        run_id,
                        event_tx,
                        last_usage,
                        context_compression_state,
                        &assistant.usage,
                        context_window,
                        model_display_name,
                    );
                    reset_message_id(current_message_id);
                    reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                    return;
                }

                emit_usage_update_if_changed(
                    run_id,
                    event_tx,
                    last_usage,
                    context_compression_state,
                    &assistant.usage,
                    context_window,
                    model_display_name,
                );
                let message_id = take_or_create_message_id(current_message_id);
                set_last_completed_message_id(last_completed_message_id, Some(message_id.clone()));
                let ti = current_turn_index.lock().ok().and_then(|g| *g);
                let _ = event_tx.send(ThreadStreamEvent::MessageCompleted {
                    run_id: run_id.to_string(),
                    message_id,
                    content,
                    turn_index: ti,
                });
            }

            reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
        }
        tiycore::agent::AgentEvent::MessageDiscarded { reason, .. } => {
            if let Some(message_id) = read_last_completed_message_id(last_completed_message_id) {
                let _ = event_tx.send(ThreadStreamEvent::MessageDiscarded {
                    run_id: run_id.to_string(),
                    message_id,
                    reason: reason.clone(),
                });
            }
        }
        _ => {}
    }
}

fn emit_usage_update_if_changed(
    run_id: &str,
    event_tx: &mpsc::UnboundedSender<ThreadStreamEvent>,
    last_usage: &StdMutex<Option<Usage>>,
    context_compression_state: &StdMutex<ContextCompressionRuntimeState>,
    usage: &Usage,
    context_window: &str,
    model_display_name: &str,
) {
    let should_emit = if let Ok(mut previous_usage) = last_usage.lock() {
        if previous_usage.as_ref() == Some(usage) {
            return;
        }

        if usage.total_tokens == 0
            && usage.input == 0
            && usage.output == 0
            && usage.cache_read == 0
            && usage.cache_write == 0
        {
            return;
        }

        *previous_usage = Some(*usage);
        true
    } else {
        usage.total_tokens > 0
            || usage.input > 0
            || usage.output > 0
            || usage.cache_read > 0
            || usage.cache_write > 0
    };

    if !should_emit {
        return;
    }

    observe_context_usage_calibration(context_compression_state, usage);

    let _ = event_tx.send(ThreadStreamEvent::ThreadUsageUpdated {
        run_id: run_id.to_string(),
        model_display_name: Some(model_display_name.to_string()),
        context_window: Some(context_window.to_string()),
        usage: RunUsageDto::from(*usage),
    });
}

fn ensure_message_id(current_message_id: &StdMutex<Option<String>>) -> String {
    if let Ok(mut guard) = current_message_id.lock() {
        if let Some(existing) = guard.clone() {
            return existing;
        }

        let message_id = uuid::Uuid::now_v7().to_string();
        *guard = Some(message_id.clone());
        return message_id;
    }

    uuid::Uuid::now_v7().to_string()
}

fn take_or_create_message_id(current_message_id: &StdMutex<Option<String>>) -> String {
    if let Ok(mut guard) = current_message_id.lock() {
        if let Some(existing) = guard.take() {
            return existing;
        }
    }

    uuid::Uuid::now_v7().to_string()
}

fn reset_message_id(current_message_id: &StdMutex<Option<String>>) {
    if let Ok(mut guard) = current_message_id.lock() {
        *guard = None;
    }
}

fn set_last_completed_message_id(
    last_completed_message_id: &StdMutex<Option<String>>,
    value: Option<String>,
) {
    if let Ok(mut guard) = last_completed_message_id.lock() {
        *guard = value;
    }
}

fn read_last_completed_message_id(
    last_completed_message_id: &StdMutex<Option<String>>,
) -> Option<String> {
    last_completed_message_id
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

fn reset_reasoning_state(
    current_reasoning_message_id: &StdMutex<Option<String>>,
    reasoning_buffer: &StdMutex<String>,
) {
    reset_message_id(current_reasoning_message_id);
    if let Ok(mut buffer) = reasoning_buffer.lock() {
        buffer.clear();
    }
}
