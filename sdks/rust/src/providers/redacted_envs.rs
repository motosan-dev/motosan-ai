//! A secret-bearing env-var collection whose `Debug` never prints values.

/// Ordered environment variables injected into a spawned CLI subprocess.
///
/// Values are **secrets** (e.g. API keys). `Debug` renders only the key count
/// (`<N redacted>`), so providers can keep `#[derive(Debug)]` without leaking.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct RedactedEnvs(Vec<(String, String)>);

impl RedactedEnvs {
    /// Append one variable (repeatable; OS env semantics decide duplicate keys).
    pub fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.push((key.into(), value.into()));
    }

    /// Replace all variables from an iterator of `(key, value)` pairs.
    pub fn replace_from<I, K, V>(&mut self, vars: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.0 = vars
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
    }

    /// Owned clone of the pairs (for threading into `SpawnConfig` / `Command::envs`).
    pub fn to_vec(&self) -> Vec<(String, String)> {
        self.0.clone()
    }
}
// NOTE: only `push`/`replace_from`/`to_vec` are added — they are the only methods
// the wiring calls. Do NOT add `as_slice`/`len`/`is_empty`: nothing calls them and
// `check-rust`'s `clippy --all-features -- -D warnings` fails on the dead_code lint.
// `Debug` reads `self.0.len()` via field access, so no `len()` method is needed.

impl std::fmt::Debug for RedactedEnvs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} redacted>", self.0.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_values_keeps_count() {
        let mut e = RedactedEnvs::default();
        e.push("API_KEY", "sk-super-secret");
        e.push("FOO", "bar");
        let dbg = format!("{e:?}");
        assert!(
            !dbg.contains("sk-super-secret"),
            "must not leak value: {dbg}"
        );
        assert!(dbg.contains("<2 redacted>"), "got: {dbg}");
        assert_eq!(
            e.to_vec(),
            vec![
                ("API_KEY".to_string(), "sk-super-secret".to_string()),
                ("FOO".to_string(), "bar".to_string()),
            ]
        );
    }
}
