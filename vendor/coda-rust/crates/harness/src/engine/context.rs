//! Model-window accounting. Provider usage anchors the estimate; content added
//! since that exchange is estimated from its characters.
use super::{Failure, Item, Turn, recipe::Request};
use serde::{Deserialize, Serialize};

// Prose tokenizes at about four characters per token, and the spec sizes the
// working window in characters at this rate.
pub(super) const CHARACTERS_PER_TOKEN: u64 = 4;
// Tool calls and results are mostly JSON, which tokenizes denser, as OpenClaw counts it.
const TOOL_CHARACTERS_PER_TOKEN: u64 = 3;
// Leave ten percent of usable input for a handoff before the input ceiling.
const HANDOFF_PERCENT: u64 = 90;
// An uncapped response still needs output space within its working window.
const REPLY_RESERVE_TOKENS: u64 = 16_384;

/// Context size at dispatch, distinct from cumulative run token usage.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsage {
    /// Estimated input size, calibrated by the latest completed provider exchange.
    pub input_tokens: u64,
    /// Last measured input and output included in this window, when known.
    pub reported_tokens: Option<u64>,
    /// Content estimate added to (or removed from) that measured context.
    pub estimated_delta: i64,
    /// Space kept free in the working window, independently of output limits.
    pub reply_reserve: u64,
}

/// Reconstructed from existing dispatch and response events, never a second ledger.
#[derive(Clone, Debug, Default)]
pub(super) struct Meter(Option<(u64, u64)>);

impl Meter {
    /// Opening history and later requests reserve the same handoff instruction.
    pub(super) fn needs_handoff(&self, request: &Request, ask: &str) -> Result<bool, Failure> {
        let capacity = input_limit(request)?;
        let threshold = capacity / 100 * HANDOFF_PERCENT + capacity % 100 * HANDOFF_PERCENT / 100;
        Ok(self.measure(&with_ask(request, ask))?.input_tokens >= threshold)
    }

    /// Input tokens this request leaves under the input limit once the ask is
    /// added, never negative.
    pub(super) fn tokens_left(&self, request: &Request, ask: &str) -> Result<u64, Failure> {
        let used = self.measure(&with_ask(request, ask))?.input_tokens;
        Ok(input_limit(request)?.saturating_sub(used))
    }

    /// Characters that `results` new tool results can fill in total and still
    /// keep this request, with the ask, within the input limit.
    pub(super) fn result_room(
        &self,
        request: &Request,
        ask: &str,
        results: usize,
    ) -> Result<usize, Failure> {
        // Each result's character count rounds up once.
        let tokens = self
            .tokens_left(request, ask)?
            .saturating_sub(u64::try_from(results).unwrap_or(u64::MAX));
        Ok(usize::try_from(tokens.saturating_mul(TOOL_CHARACTERS_PER_TOKEN)).unwrap_or(usize::MAX))
    }

    pub(super) fn observe(&mut self, request: &Request, turn: &Turn) -> Result<(), Failure> {
        if let Some(input) = turn.usage.input_tokens {
            let (reported, estimate) = match turn.usage.output_tokens {
                Some(output) => (
                    input.saturating_add(output),
                    estimate_request(request)?
                        .saturating_add(estimate_items(&turn.items).ok_or_else(unresolved_view)?),
                ),
                None => (input, estimate_request(request)?),
            };
            self.0 = Some((reported, estimate));
        }
        Ok(())
    }

    pub(super) fn measure(&self, request: &Request) -> Result<ContextUsage, Failure> {
        let estimate = estimate_request(request)?;
        let (input_tokens, reported_tokens, estimated_delta) = match self.0 {
            Some((reported, baseline)) => {
                let delta = i64::try_from(estimate)
                    .unwrap_or(i64::MAX)
                    .saturating_sub(i64::try_from(baseline).unwrap_or(i64::MAX));
                (reported.saturating_add_signed(delta), Some(reported), delta)
            }
            None => (estimate, None, i64::try_from(estimate).unwrap_or(i64::MAX)),
        };
        Ok(ContextUsage {
            input_tokens,
            reported_tokens,
            estimated_delta,
            reply_reserve: REPLY_RESERVE_TOKENS,
        })
    }
}

/// Estimate materialized model content, including the result owner's encoding.
/// Retained views must be resolved by their owner before they can be measured.
pub fn estimate_request(request: &Request) -> Result<u64, Failure> {
    Ok(tokens(request.instructions.chars().count())
        .saturating_add(tokens(
            request
                .tools
                .iter()
                .map(|tool| tool.to_string().chars().count())
                .sum(),
        ))
        .saturating_add(estimate_items(&request.items).ok_or_else(unresolved_view)?))
}

fn unresolved_view() -> Failure {
    Failure::Protocol("context accounting requires materialized result views".into())
}

pub(super) fn estimate_items(items: &[Item]) -> Option<u64> {
    items
        .iter()
        .map(|item| {
            let content = match item {
                Item::Message { role, text } => {
                    tokens(role.chars().count().saturating_add(text.chars().count()))
                }
                Item::Feedback { text } => tokens(
                    "developer"
                        .chars()
                        .count()
                        .saturating_add(text.chars().count()),
                ),
                Item::FinishRefused(refusal) => tokens(
                    "developer"
                        .chars()
                        .count()
                        .saturating_add(refusal.feedback().to_string().chars().count()),
                ),
                Item::ProductMessage(message) => tokens(
                    message
                        .role
                        .as_str()
                        .chars()
                        .count()
                        .saturating_add(message.text.chars().count()),
                ),
                Item::Call(call) => tool_tokens(
                    call.name
                        .chars()
                        .count()
                        .saturating_add(call.arguments.chars().count()),
                ),
                Item::Result { .. } => return None,
                Item::MaterializedResult { output, .. } => tool_tokens(output.chars().count()),
                Item::Opaque(value) => tokens(value.to_string().chars().count()),
                Item::OutputCutoff => return Some(0),
            };
            Some(content)
        })
        .try_fold(0_u64, |total, next| Some(total.saturating_add(next?)))
}

pub(super) fn tokens(characters: usize) -> u64 {
    u64::try_from(characters)
        .unwrap_or(u64::MAX)
        .div_ceil(CHARACTERS_PER_TOKEN)
}

fn tool_tokens(characters: usize) -> u64 {
    u64::try_from(characters)
        .unwrap_or(u64::MAX)
        .div_ceil(TOOL_CHARACTERS_PER_TOKEN)
}

fn with_ask(request: &Request, ask: &str) -> Request {
    let mut request = request.clone();
    request.items.push(Item::Message {
        role: "developer".into(),
        text: ask.into(),
    });
    request
}

/// Maximum input with enough room for the reply. The handoff prompt is input too.
pub(super) fn input_limit(request: &Request) -> Result<u64, Failure> {
    input_capacity(request.context_window_tokens, request.max_input_tokens)
}

/// Validate product configuration against the same reply allowance used at dispatch.
/// Startup must reject a working window with no room left for input.
pub fn input_capacity(context_window_tokens: u64, max_input_tokens: u64) -> Result<u64, Failure> {
    context_window_tokens
        .checked_sub(REPLY_RESERVE_TOKENS)
        .map(|limit| limit.min(max_input_tokens))
        .filter(|limit| *limit > 0)
        .ok_or_else(|| Failure::Protocol("model window has no room for its reply allowance".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_calls_and_results_count_denser_than_other_text() {
        let tokens = |item: Item| estimate_items(&[item]).unwrap();
        // The estimate counts characters, not UTF-8 bytes.
        for character in ['x', 'é', '界', '🩺'] {
            let text = character.to_string().repeat(1_200);
            assert_eq!(
                tokens(Item::MaterializedResult {
                    call_id: "read".into(),
                    output: text.clone(),
                }),
                400
            );
            assert_eq!(
                tokens(Item::Call(super::super::Invocation {
                    id: "read".into(),
                    name: String::new(),
                    arguments: text.clone(),
                })),
                400
            );
            assert_eq!(
                tokens(Item::Message {
                    role: String::new(),
                    text,
                }),
                300
            );
        }
    }

    #[test]
    fn reply_reserve_reduces_input_even_without_an_output_cap() {
        let mut request = Request {
            cache_key: None,
            model: "configured".into(),
            context_window_tokens: 100_000,
            max_input_tokens: 100_000,
            options: serde_json::json!({}),
            instructions: String::new(),
            tools: vec![],
            items: vec![],
        };
        assert_eq!(input_limit(&request).unwrap(), 83_616);
        request.max_input_tokens = 50_000;
        assert_eq!(input_limit(&request).unwrap(), 50_000);
        request.context_window_tokens = 16_384;
        assert!(input_limit(&request).is_err());
        request.context_window_tokens = 16_385;
        assert_eq!(input_limit(&request).unwrap(), 1);
    }
}
