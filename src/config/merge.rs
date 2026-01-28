//! Deep merge functionality for YAML configurations.
//!
//! Implements field-by-field merging where higher tier values override lower tier values.
//! Arrays are replaced entirely, not concatenated.

use serde_json::Value;

/// Deep merge two JSON values, with `overlay` taking precedence over `base`.
///
/// - Objects are merged recursively: keys in overlay override keys in base
/// - Arrays, strings, numbers, booleans, nulls are replaced entirely
/// - If overlay is null, the base value is preserved (null means "not specified")
///
/// # Example
/// ```
/// use serde_json::json;
/// use task_graph_mcp::config::deep_merge;
///
/// let base = json!({
///     "server": { "port": 8080, "host": "localhost" },
///     "features": ["a", "b"]
/// });
/// let overlay = json!({
///     "server": { "port": 9000 },
///     "features": ["c"]
/// });
/// let result = deep_merge(base, overlay);
/// // Result: { "server": { "port": 9000, "host": "localhost" }, "features": ["c"] }
/// ```
pub fn deep_merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        // Both are objects: merge recursively
        (Value::Object(mut base_map), Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                let merged_value = if let Some(base_value) = base_map.remove(&key) {
                    deep_merge(base_value, overlay_value)
                } else {
                    overlay_value
                };
                base_map.insert(key, merged_value);
            }
            Value::Object(base_map)
        }
        // Overlay is null: preserve base (null means "not specified")
        (base, Value::Null) => base,
        // Any other case: overlay replaces base entirely
        (_, overlay) => overlay,
    }
}

/// Merge multiple values in order, with later values taking precedence.
///
/// Equivalent to folding `deep_merge` over the list.
pub fn deep_merge_all(values: impl IntoIterator<Item = Value>) -> Value {
    values.into_iter().fold(Value::Null, deep_merge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_merge_simple_objects() {
        let base = json!({"a": 1, "b": 2});
        let overlay = json!({"b": 3, "c": 4});
        let result = deep_merge(base, overlay);
        assert_eq!(result, json!({"a": 1, "b": 3, "c": 4}));
    }

    #[test]
    fn test_merge_nested_objects() {
        let base = json!({
            "server": {"host": "localhost", "port": 8080},
            "debug": true
        });
        let overlay = json!({
            "server": {"port": 9000}
        });
        let result = deep_merge(base, overlay);
        assert_eq!(
            result,
            json!({
                "server": {"host": "localhost", "port": 9000},
                "debug": true
            })
        );
    }

    #[test]
    fn test_arrays_replaced_not_merged() {
        let base = json!({"items": [1, 2, 3]});
        let overlay = json!({"items": [4, 5]});
        let result = deep_merge(base, overlay);
        assert_eq!(result, json!({"items": [4, 5]}));
    }

    #[test]
    fn test_null_preserves_base() {
        let base = json!({"a": 1, "b": {"c": 2}});
        let overlay = json!({"a": null, "b": {"c": null}});
        let result = deep_merge(base, overlay);
        assert_eq!(result, json!({"a": 1, "b": {"c": 2}}));
    }

    #[test]
    fn test_deep_nested_merge() {
        let base = json!({
            "level1": {
                "level2": {
                    "level3": {"a": 1, "b": 2}
                }
            }
        });
        let overlay = json!({
            "level1": {
                "level2": {
                    "level3": {"b": 3, "c": 4}
                }
            }
        });
        let result = deep_merge(base, overlay);
        assert_eq!(
            result,
            json!({
                "level1": {
                    "level2": {
                        "level3": {"a": 1, "b": 3, "c": 4}
                    }
                }
            })
        );
    }

    #[test]
    fn test_merge_all() {
        let values = vec![json!({"a": 1}), json!({"b": 2}), json!({"a": 3, "c": 4})];
        let result = deep_merge_all(values);
        assert_eq!(result, json!({"a": 3, "b": 2, "c": 4}));
    }

    #[test]
    fn test_overlay_replaces_primitive_with_object() {
        let base = json!({"value": 42});
        let overlay = json!({"value": {"nested": true}});
        let result = deep_merge(base, overlay);
        assert_eq!(result, json!({"value": {"nested": true}}));
    }

    #[test]
    fn test_overlay_replaces_object_with_primitive() {
        let base = json!({"value": {"nested": true}});
        let overlay = json!({"value": 42});
        let result = deep_merge(base, overlay);
        assert_eq!(result, json!({"value": 42}));
    }
}
