use sha2::{Digest, Sha256};

pub(crate) fn extract_json_object(content: &str) -> anyhow::Result<String> {
    let trimmed = content.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed.to_owned());
    }
    let Some(start) = trimmed.find('{') else {
        anyhow::bail!("AI返回内容不是JSON");
    };
    let Some(end) = trimmed.rfind('}') else {
        anyhow::bail!("AI返回内容不是完整JSON");
    };
    Ok(trimmed[start..=end].to_owned())
}

pub(crate) fn truncate_text(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub(crate) fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
