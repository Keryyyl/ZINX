use serde_json::Value;

const UNSUPPORTED: [&str; 4] = ["maxItems", "minItems", "maxLength", "minLength"];

/// Schemat odpowiedzi w wersji akceptowanej przez tryby „structured output” dostawców chmurowych:
/// obiekty zamknięte (additionalProperties: false), bez limitów długości. Limity i tak egzekwują
/// parsery odpowiedzi (`parse_tag_ids`, `parse_summary`), więc nic nie tracimy.
pub fn strict_schema(schema: &Value) -> Value {
    match schema {
        Value::Array(items) => Value::Array(items.iter().map(strict_schema).collect()),
        Value::Object(map) => {
            let mut out: serde_json::Map<String, Value> =
                map.iter().filter(|(k, _)| !UNSUPPORTED.contains(&k.as_str())).map(|(k, v)| (k.clone(), strict_schema(v))).collect();
            if out.get("type").and_then(Value::as_str) == Some("object") {
                out.insert("additionalProperties".into(), Value::Bool(false));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn closes_objects_and_drops_length_limits() {
        let s = json!({"type":"object","properties":{"tags":{"type":"array","maxItems":1,"items":{"type":"string","maxLength":5}}},"required":["tags"]});
        let out = strict_schema(&s);
        assert_eq!(out["additionalProperties"], false);
        assert!(out["properties"]["tags"].get("maxItems").is_none());
        assert!(out["properties"]["tags"]["items"].get("maxLength").is_none());
        assert_eq!(out["required"], json!(["tags"]));
        assert!(s["properties"]["tags"].get("maxItems").is_some(), "oryginał bez zmian");
    }
}
