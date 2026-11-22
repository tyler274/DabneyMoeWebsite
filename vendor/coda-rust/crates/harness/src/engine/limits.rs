//! Shared runaway backstops, separate from product commit policy.

// The harness specification gives each run at most one hundred model requests.
const MAX_REQUESTS: u64 = 100;
// Bound transport buffering and parsed turns independently of model output tokens.
// A response is held in memory while it streams and its turn is saved whole as
// one event, so a runaway response cannot exhaust memory or the product's record.
const MAX_RESPONSE_BYTES: usize = 4 * 1_048_576;
// Allow several fresh working windows before stopping runaway uncached work.
// Two million covers more than seven fully uncached 272K windows; cache hits
// do not consume this backstop, so ordinary continuation can run much longer.
const MAX_RUN_TOKENS: u64 = 2_000_000;
// Ten requests at the shared ceiling leave room to give the answer before the
// single final request. Token use jumps by a whole input window, so its warning
// cushion is larger: 400K at the shared ceiling covers a 272K input plus output.
const REQUEST_WARNING_DIVISOR: u64 = 10;
const TOKEN_WARNING_DIVISOR: u64 = 5;

/// Run backstop that reserved the terminal model request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LimitBackstop {
    /// Cumulative provider request ceiling.
    Requests,
    /// Cumulative output and uncached input tokens, measured or estimated.
    Tokens,
}

/// The backstops that reserved a terminal request. It is never empty.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct ReachedBackstops(Vec<LimitBackstop>);

impl ReachedBackstops {
    /// Reserve the terminal request only when at least one backstop was reached.
    pub fn new(reached: Vec<LimitBackstop>) -> Option<Self> {
        if reached.is_empty() {
            None
        } else {
            Some(Self(reached))
        }
    }
}

impl std::ops::Deref for ReachedBackstops {
    type Target = [LimitBackstop];

    fn deref(&self) -> &[LimitBackstop] {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for ReachedBackstops {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?)
            .ok_or_else(|| serde::de::Error::invalid_length(0, &"at least one reached backstop"))
    }
}

/// Shared runaway backstops for an accepted job.
#[derive(Debug, Clone, Copy)]
pub struct RunLimits {
    /// Cumulative requests, including earlier attempts.
    pub max_calls: u64,
    /// Maximum bytes on the provider response transport and parsed turn.
    pub max_response_bytes: usize,
    /// Output plus uncached input allowed across all attempts; missing counts use estimates.
    pub max_tokens: u64,
}

/// The shared request, response and token ceilings.
impl Default for RunLimits {
    fn default() -> Self {
        Self {
            max_calls: MAX_REQUESTS,
            max_response_bytes: MAX_RESPONSE_BYTES,
            max_tokens: MAX_RUN_TOKENS,
        }
    }
}

impl RunLimits {
    /// Callers may exercise a smaller budget, but cannot widen a shared ceiling.
    pub(super) fn bounded(self) -> Self {
        let ceiling = Self::default();
        Self {
            max_calls: self.max_calls.min(ceiling.max_calls),
            max_response_bytes: self.max_response_bytes.min(ceiling.max_response_bytes),
            max_tokens: self.max_tokens.min(ceiling.max_tokens),
        }
    }

    pub(crate) fn reached_backstops(
        &self,
        usage: &super::recipe::Progress,
    ) -> Option<ReachedBackstops> {
        let mut reached = Vec::new();
        if usage.model_steps().saturating_add(1) >= self.max_calls {
            reached.push(LimitBackstop::Requests);
        }
        if usage.budget_tokens >= self.max_tokens {
            reached.push(LimitBackstop::Tokens);
        }
        ReachedBackstops::new(reached)
    }

    pub(crate) fn near_limit(&self, usage: &super::recipe::Progress) -> bool {
        usage.model_steps().saturating_add(1)
            >= self
                .max_calls
                .saturating_sub((self.max_calls / REQUEST_WARNING_DIVISOR).max(1))
            || usage.budget_tokens
                >= self
                    .max_tokens
                    .saturating_sub(self.max_tokens / TOKEN_WARNING_DIVISOR)
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::RequestPurpose;

    #[test]
    fn a_saved_final_keeps_its_wire_shape_and_needs_a_reached_backstop() {
        let saved = r#"{"Final":{"backstops":["Requests","Tokens"]}}"#;
        let purpose: RequestPurpose = serde_json::from_str(saved).unwrap();
        assert_eq!(serde_json::to_string(&purpose).unwrap(), saved);
        assert!(serde_json::from_str::<RequestPurpose>(r#"{"Final":{"backstops":[]}}"#).is_err());
    }
}
