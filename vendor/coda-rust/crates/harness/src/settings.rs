//! Required job settings. Products choose the prefix and roles; the harness
//! validates each binding before accepted work can freeze it.
use crate::engine;

/// A model selection without a transport or product identity.
#[derive(Clone, Debug)]
pub struct Job {
    /// Provider model name.
    pub model: String,
    /// Reasoning effort supplied by the product.
    pub effort: String,
    /// Product-selected working window.
    pub context_window_tokens: u64,
    /// Provider input ceiling.
    pub max_input_tokens: u64,
}

/// An absent or invalid configuration retains its original cause.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Required value is absent.
    #[error("{0} is required")]
    Missing(String),
    /// Model and effort must each be nonblank.
    #[error("{0} must not be blank")]
    Blank(String),
    /// A positive integer could not be read.
    #[error("{key} must be a positive integer: {source}")]
    Count {
        /// Setting key.
        key: String,
        /// Original parse error.
        source: std::num::ParseIntError,
    },
    /// The working window cannot fit input and reply allowance.
    #[error("{key}: {source}")]
    Window {
        /// Window setting key.
        key: String,
        /// Original capacity error.
        source: engine::Failure,
    },
}
/// Read required settings from the product's configuration source.
pub struct Reader<F>(pub F);
impl<F: Fn(&str) -> Option<String>> Reader<F> {
    fn required(&self, key: &str) -> Result<String, Error> {
        (self.0)(key).ok_or_else(|| Error::Missing(key.into()))
    }
    fn nonblank(&self, key: &str) -> Result<String, Error> {
        let value = self.required(key)?;
        if value.trim().is_empty() {
            return Err(Error::Blank(key.into()));
        }
        Ok(value)
    }
    /// Read a positive count without a fallback.
    fn count(&self, key: &str) -> Result<u64, Error> {
        self.required(key)?
            .parse::<std::num::NonZeroU64>()
            .map(std::num::NonZeroU64::get)
            .map_err(|source| Error::Count {
                key: key.into(),
                source,
            })
    }
    /// Read one job's explicit model, effort and capacities.
    pub fn job(&self, prefix: &str) -> Result<Job, Error> {
        let model = self.nonblank(&format!("{prefix}_MODEL"))?;
        let effort = self.nonblank(&format!("{prefix}_EFFORT"))?;
        let key = format!("{prefix}_CONTEXT_WINDOW_TOKENS");
        let context_window_tokens = self.count(&key)?;
        let max_input_tokens = self.count(&format!("{prefix}_INPUT_LIMIT_TOKENS"))?;
        engine::context::input_capacity(context_window_tokens, max_input_tokens)
            .map_err(|source| Error::Window { key, source })?;
        Ok(Job {
            model,
            effort,
            context_window_tokens,
            max_input_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn required_job_settings_validate_presence_counts_and_capacity() {
        let values = [
            ("JOB_MODEL", "configured-model"),
            ("JOB_EFFORT", "high"),
            ("JOB_CONTEXT_WINDOW_TOKENS", "272000"),
            ("JOB_INPUT_LIMIT_TOKENS", "922000"),
            ("SAMPLE_COUNT", "660"),
        ];
        let get = |key: &str| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, v)| (*v).to_owned())
        };
        let reader = Reader(get);
        let job = reader.job("JOB").unwrap();
        assert_eq!(job.model, "configured-model");
        assert_eq!(job.effort, "high");
        assert_eq!(
            (job.context_window_tokens, job.max_input_tokens),
            (272000, 922000)
        );
        assert_eq!(reader.count("SAMPLE_COUNT").unwrap(), 660);
        for (key, _) in values {
            let missing = Reader(|name: &str| if name == key { None } else { get(name) });
            let result = if key == "SAMPLE_COUNT" {
                missing.count(key).map(|_| ())
            } else {
                missing.job("JOB").map(|_| ())
            };
            assert!(matches!(result, Err(Error::Missing(name)) if name==key));
        }
        for key in ["JOB_MODEL", "JOB_EFFORT"] {
            for value in ["", "  "] {
                let reader = Reader(|name: &str| {
                    if name == key {
                        Some(value.into())
                    } else {
                        get(name)
                    }
                });
                assert!(matches!(reader.job("JOB"), Err(Error::Blank(name)) if name==key));
            }
        }
        for key in [
            "JOB_CONTEXT_WINDOW_TOKENS",
            "JOB_INPUT_LIMIT_TOKENS",
            "SAMPLE_COUNT",
        ] {
            for value in ["0", "-1", "invalid"] {
                let reader = Reader(|name: &str| {
                    if name == key {
                        Some(value.into())
                    } else {
                        get(name)
                    }
                });
                assert!(
                    matches!(reader.count(key), Err(Error::Count {key: name, ..}) if name==key)
                );
            }
        }
        for value in ["1", "16383", "16384", "16385"] {
            let reader = Reader(|name: &str| {
                if name == "JOB_CONTEXT_WINDOW_TOKENS" {
                    Some(value.into())
                } else {
                    get(name)
                }
            });
            assert_eq!(reader.job("JOB").is_ok(), value == "16385");
        }
    }
}
