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
            {
                let mut guard = lock_or_recover(current_turn_index, "current_turn_index");
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
                    let mut buffer =
                        lock_or_recover(reasoning_buffer, "ThinkingDelta:reasoning_buffer");
                    buffer.push_str(delta);
                    let message_id = ensure_message_id(current_reasoning_message_id);
                    let ti =
                        lock_or_recover(current_turn_index, "ThinkingDelta:turn_index").clone();
                    let _ = event_tx.send(ThreadStreamEvent::ReasoningUpdated {
                        run_id: run_id.to_string(),
                        message_id,
                        reasoning: buffer.clone(),
                        thinking_signature: None,
                        turn_index: ti,
                    });
                }
                AssistantMessageEvent::ThinkingEnd {
                    content, partial, ..
                } => {
                    let reasoning = {
                        let mut buffer =
                            lock_or_recover(reasoning_buffer, "ThinkingEnd:reasoning_buffer");
                        buffer.clear();
                        buffer.push_str(content);
                        buffer.clone()
                    };

                    if reasoning.trim().is_empty() {
                        reset_reasoning_state(current_reasoning_message_id, reasoning_buffer);
                        return;
                    }

                    // Extract thinking_signature from the partial message's final
                    // Thinking content block. The protocol layer populates this
                    // during streaming, so search from the end of partial.content
                    // to find the complete block when ThinkingEnd fires.
                    let thinking_signature = partial
                        .content
                        .iter()
                        .rev()
                        .find_map(|b| b.as_thinking())
                        .and_then(|t| t.thinking_signature.clone());

                    let message_id = ensure_message_id(current_reasoning_message_id);
                    let ti = lock_or_recover(current_turn_index, "ThinkingEnd:turn_index").clone();
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
                let ti = lock_or_recover(current_turn_index, "MessageEnd:turn_index").clone();
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
    let should_emit = {
        let mut previous_usage = lock_or_recover(last_usage, "emit_usage:last_usage");
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

/// Helper to recover a poisoned `StdMutex`. All mutex helpers below use this
/// pattern so that a panic in an unrelated thread never silently corrupts
/// message-tracking state.
fn lock_or_recover<'a, T>(mutex: &'a StdMutex<T>, context: &str) -> std::sync::MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("{context}: mutex poisoned, recovering");
        poisoned.into_inner()
    })
}

fn ensure_message_id(current_message_id: &StdMutex<Option<String>>) -> String {
    let mut guard = lock_or_recover(current_message_id, "ensure_message_id");
    if let Some(existing) = guard.clone() {
        return existing;
    }
    let message_id = uuid::Uuid::now_v7().to_string();
    *guard = Some(message_id.clone());
    message_id
}

fn take_or_create_message_id(current_message_id: &StdMutex<Option<String>>) -> String {
    let mut guard = lock_or_recover(current_message_id, "take_or_create_message_id");
    if let Some(existing) = guard.take() {
        return existing;
    }
    uuid::Uuid::now_v7().to_string()
}

fn reset_message_id(current_message_id: &StdMutex<Option<String>>) {
    let mut guard = lock_or_recover(current_message_id, "reset_message_id");
    *guard = None;
}

fn set_last_completed_message_id(
    last_completed_message_id: &StdMutex<Option<String>>,
    value: Option<String>,
) {
    let mut guard = lock_or_recover(last_completed_message_id, "set_last_completed_message_id");
    *guard = value;
}

fn read_last_completed_message_id(
    last_completed_message_id: &StdMutex<Option<String>>,
) -> Option<String> {
    let guard = lock_or_recover(last_completed_message_id, "read_last_completed_message_id");
    guard.clone()
}

fn reset_reasoning_state(
    current_reasoning_message_id: &StdMutex<Option<String>>,
    reasoning_buffer: &StdMutex<String>,
) {
    reset_message_id(current_reasoning_message_id);
    let mut buffer = lock_or_recover(reasoning_buffer, "reset_reasoning_state");
    buffer.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_or_recover_recovers_poisoned_mutex() {
        let mutex = StdMutex::new(String::from("before"));

        let result = std::panic::catch_unwind(|| {
            let _guard = mutex.lock().expect("mutex should lock before poison");
            panic!("poison mutex for recovery test");
        });
        assert!(result.is_err());

        {
            let mut guard = lock_or_recover(&mutex, "test");
            assert_eq!(guard.as_str(), "before");
            guard.clear();
            guard.push_str("after");
        }

        let guard = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(guard.as_str(), "after");
    }
}
