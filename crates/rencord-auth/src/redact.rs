//! Strip session-shaped strings before anything is shown or logged.

/// Replace Discord-shaped tokens and long secrets with `[redacted]`.
pub fn redact(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if is_secret_char(chars[index]) {
            let start = index;
            while index < chars.len() && is_secret_char(chars[index]) {
                index += 1;
            }
            let slice: String = chars[start..index].iter().collect();
            if looks_secret(&slice) {
                out.push_str("[redacted]");
            } else {
                out.push_str(&slice);
            }
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn is_secret_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+' | '/' | '=')
}

fn looks_secret(value: &str) -> bool {
    if value.len() >= 20 && value.starts_with("mfa.") {
        return true;
    }
    let dots = value.bytes().filter(|byte| *byte == b'.').count();
    if dots >= 2 && value.len() >= 24 {
        return true;
    }
    value.len() >= 48
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_hides_tokens_and_keeps_prose() {
        let sample = "session ODUyODky.GX5Xdp.22jsdSqEiHLUYEJSsjeq_vJKLp for Rencord";
        let cleaned = redact(sample);
        assert!(!cleaned.contains("GX5Xdp"));
        assert!(!cleaned.contains("22jsdSqEiHLU"));
        assert!(cleaned.contains("Rencord"));
        assert!(cleaned.contains("[redacted]"));
        assert_eq!(redact("short name"), "short name");
    }
}
