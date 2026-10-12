use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

pub(super) const MAX_CALLS: usize = 16;
pub(super) enum Decision {
    Execute,
    Cached(Value),
    Reject(&'static str),
}
type Key = [u8; 32];
pub(super) struct Budget {
    calls: usize,
    ceiling: usize,
    stopped: bool,
    cache: HashMap<Key, (Instant, Value)>,
    repeated: HashMap<Key, (Key, usize)>,
    uncertain: HashSet<Key>,
}
impl Default for Budget {
    fn default() -> Self {
        Self::new(MAX_CALLS)
    }
}
impl Budget {
    pub(super) fn new(ceiling: usize) -> Self {
        Self {
            calls: 0,
            ceiling,
            stopped: false,
            cache: HashMap::new(),
            repeated: HashMap::new(),
            uncertain: HashSet::new(),
        }
    }
    pub(super) fn calls(&self) -> usize {
        self.calls
    }
    pub(super) fn exhausted(&self) -> bool {
        self.calls >= self.ceiling
    }
    pub(super) fn safety_halted(&self) -> bool {
        self.stopped
    }
    pub(super) fn extend(&mut self, allowance: usize) {
        // Extending a budget must preserve repeated-result and uncertain-outcome guards.
        self.ceiling = self.ceiling.saturating_add(allowance);
    }
    pub(super) fn begin(&mut self, name: &str, args: &Value) -> Decision {
        let key = fingerprint(name, args);
        if self.uncertain.contains(&key) {
            return Decision::Reject("AI_OUTCOME_UNKNOWN");
        }
        if self.halted() {
            return Decision::Reject("AI_TOOL_BUDGET");
        }
        self.calls += 1;
        if read(name) {
            if let Some((at, value)) = self.cache.get(&key)
                && at.elapsed() < Duration::from_secs(1)
            {
                return Decision::Cached(value.clone());
            }
        } else {
            // Invalidate before attempting a mutation: even a failed or uncertain command
            // may have changed remote state. Mutation results are never reused.
            self.cache.clear();
        }
        Decision::Execute
    }
    pub(super) fn record(&mut self, name: &str, args: &Value, result: &Value) {
        let key = fingerprint(name, args);
        if matches!(
            result["state"].as_str(),
            Some("outcome_unknown" | "timed_out" | "interrupted" | "cancel_requested")
        ) || matches!(
            result["error"]["code"]
                .as_str()
                .or(result["error"].as_str()),
            Some("OUTCOME_UNKNOWN" | "TIMEOUT")
        ) {
            self.uncertain.insert(key);
            self.stopped = true;
        }
        let outcome = fingerprint("result", result);
        let repeat = self.repeated.entry(key).or_insert((outcome, 0));
        if repeat.0 != outcome {
            *repeat = (outcome, 0);
        }
        repeat.1 += 1;
        if repeat.1 >= 3 {
            self.stopped = true;
        }
        if read(name) && result.get("error").is_none_or(Value::is_null) {
            if self.cache.len() >= 4
                && !self.cache.contains_key(&key)
                && let Some(oldest) = self
                    .cache
                    .iter()
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(key, _)| *key)
            {
                self.cache.remove(&oldest);
            }
            // Reusing a cached result must not extend its freshness window.
            self.cache
                .entry(key)
                .and_modify(|(at, previous)| {
                    if at.elapsed() >= Duration::from_secs(1) || previous != result {
                        *at = Instant::now();
                        *previous = result.clone();
                    }
                })
                .or_insert_with(|| (Instant::now(), result.clone()));
        }
    }
    pub(super) fn halted(&self) -> bool {
        self.stopped || self.exhausted()
    }
}
fn read(name: &str) -> bool {
    matches!(name, "read_file" | "list_directory")
}
fn fingerprint(name: &str, value: &Value) -> Key {
    let mut digest = Sha256::new();
    digest.update(name.as_bytes());
    digest.update([0]);
    digest.update(value.to_string().as_bytes());
    digest.finalize().into()
}

/// Preserve metadata and valid JSON when content needs a smaller model-context preview.
pub(super) fn bounded_result(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("id");
    }
    let mut maximum = 32 * 1024;
    while value.to_string().len() > 64 * 1024 && maximum > 0 {
        trim_content(&mut value, maximum);
        value["model_result_truncated"] = true.into();
        maximum /= 2;
    }
    if value.to_string().len() > 64 * 1024 {
        return json!({"error":"AI_RESULT_LIMIT","model_result_truncated":true});
    }
    value
}
fn trim_content(value: &mut Value, maximum: usize) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "content" | "text" | "stdout" | "stderr") {
                    if let Some(text) = value.as_str()
                        && text.len() > maximum
                    {
                        *value = text[..text.floor_char_boundary(maximum)].to_owned().into();
                    }
                } else {
                    trim_content(value, maximum);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                trim_content(value, maximum);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extension_keeps_the_completed_count_and_safety_guards() {
        let mut budget = Budget::new(1);
        let args = json!({"command":"fixture"});
        assert!(matches!(
            budget.begin("run_command", &args),
            Decision::Execute
        ));
        assert!(budget.exhausted());
        budget.extend(1);
        assert_eq!(budget.calls(), 1);
        assert!(!budget.exhausted());
        budget.record("run_command", &args, &json!({"state":"outcome_unknown"}));
        budget.extend(128);
        assert!(budget.safety_halted());
        assert!(matches!(
            budget.begin("run_command", &args),
            Decision::Reject("AI_OUTCOME_UNKNOWN")
        ));
    }
    use serde_json::json;
    #[test]
    fn total_budget_bounds_a_large_model_batch() {
        let mut budget = Budget::default();
        for i in 0..MAX_CALLS {
            assert!(matches!(
                budget.begin("read_file", &json!({"path":format!("/{i}")})),
                Decision::Execute
            ));
        }
        assert!(matches!(
            budget.begin("read_file", &json!({"path":"/next"})),
            Decision::Reject("AI_TOOL_BUDGET")
        ));
    }
    #[test]
    fn repeated_read_reuses_result_and_mutations_invalidate_it() {
        let mut budget = Budget::default();
        let args = json!({"path":"/file"});
        let result = json!({"content":"original","version":"fixture"});
        budget.begin("read_file", &args);
        budget.record("read_file", &args, &result);
        assert!(matches!(budget.begin("read_file",&args),Decision::Cached(v) if v==result));
        assert!(matches!(
            budget.begin("run_command", &json!({"command":"fixture"})),
            Decision::Execute
        ));
        assert!(matches!(
            budget.begin("read_file", &args),
            Decision::Execute
        ));
    }
    #[test]
    fn three_identical_failures_stop_further_actions() {
        let mut budget = Budget::default();
        let args = json!({"path":"/missing"});
        for _ in 0..3 {
            budget.begin("read_file", &args);
            budget.record("read_file", &args, &json!({"error":"FILE_ERROR"}));
        }
        assert!(budget.halted());
    }
    #[test]
    fn uncertain_mutation_is_not_resubmitted_with_a_new_request_identifier() {
        let mut budget = Budget::default();
        let args = json!({"command":"fixture"});
        budget.begin("run_command", &args);
        budget.record(
            "run_command",
            &args,
            &json!({"state":"outcome_unknown","error":{"code":"OUTCOME_UNKNOWN"}}),
        );
        assert!(matches!(
            budget.begin("run_command", &args),
            Decision::Reject("AI_OUTCOME_UNKNOWN")
        ));
    }
    #[test]
    fn large_results_keep_versions_and_remain_valid_json() {
        let value = bounded_result(
            json!({"content":"\"你好\n".repeat(50000),"version":"v1","size":500000,"encoding":"UTF-8"}),
        );
        assert!(value.to_string().len() <= 64 * 1024);
        assert_eq!(value["version"], "v1");
        assert_eq!(value["model_result_truncated"], true);
    }
    #[test]
    fn successful_mutations_are_not_cached() {
        let mut budget = Budget::default();
        let args = json!({"command":"fixture"});
        budget.begin("run_command", &args);
        budget.record("run_command", &args, &json!({"state":"succeeded"}));
        assert!(matches!(
            budget.begin("run_command", &args),
            Decision::Execute
        ));
    }
    #[test]
    fn cached_reads_expire_and_the_cache_has_a_fixed_entry_limit() {
        let mut budget = Budget::default();
        for i in 0..5 {
            let args = json!({"path":format!("/{i}")});
            budget.begin("read_file", &args);
            budget.record("read_file", &args, &json!({"content":i.to_string()}));
        }
        assert_eq!(budget.cache.len(), 4);
        let args = json!({"path":"/4"});
        let key = fingerprint("read_file", &args);
        budget.cache.get_mut(&key).unwrap().0 = Instant::now() - Duration::from_secs(2);
        assert!(matches!(
            budget.begin("read_file", &args),
            Decision::Execute
        ));
    }
    #[test]
    fn a_cache_hit_does_not_extend_result_freshness() {
        let mut budget = Budget::default();
        let args = json!({"path":"/file"});
        let result = json!({"content":"fixture"});
        budget.record("read_file", &args, &result);
        let original = Instant::now() - Duration::from_millis(500);
        budget
            .cache
            .get_mut(&fingerprint("read_file", &args))
            .unwrap()
            .0 = original;
        assert!(matches!(
            budget.begin("read_file", &args),
            Decision::Cached(_)
        ));
        budget.record("read_file", &args, &result);
        assert_eq!(budget.cache[&fingerprint("read_file", &args)].0, original);
    }
}
