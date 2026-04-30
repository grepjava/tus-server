use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::HashMap;

pub fn parse(header: &str) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();

    for part in header.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let mut iter = part.splitn(2, ' ');
        let key = iter
            .next()
            .ok_or_else(|| format!("invalid metadata part: {part}"))?;

        let value = match iter.next() {
            Some(b64) => {
                let bytes = STANDARD
                    .decode(b64.trim())
                    .map_err(|e| format!("base64 decode error for '{key}': {e}"))?;
                String::from_utf8(bytes)
                    .map_err(|e| format!("utf-8 error for '{key}': {e}"))?
            }
            None => String::new(),
        };

        if map.contains_key(key) {
            return Err(format!("duplicate metadata key: {key}"));
        }
        map.insert(key.to_string(), value);
    }

    Ok(map)
}

pub fn get_filename(metadata: &HashMap<String, String>) -> Option<String> {
    metadata
        .get("filename")
        .cloned()
        .or_else(|| metadata.get("name").cloned())
}
