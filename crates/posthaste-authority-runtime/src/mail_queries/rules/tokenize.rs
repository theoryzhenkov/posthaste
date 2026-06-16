pub(super) struct QueryToken {
    pub(super) raw: String,
    pub(super) negated: bool,
    pub(super) prefix: Option<String>,
    pub(super) value: String,
}

pub(super) fn tokenize(input: &str) -> Vec<QueryToken> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let token_start = i;
        let negated = chars[i] == '-' && i + 1 < chars.len() && !chars[i + 1].is_whitespace();
        if negated {
            i += 1;
        }
        let prefix_start = i;
        let mut colon = None;
        while i < chars.len() && !chars[i].is_whitespace() {
            if chars[i] == ':' {
                colon = Some(i);
                break;
            }
            i += 1;
        }
        let (prefix, value) = if let Some(colon) = colon {
            let prefix = chars[prefix_start..colon].iter().collect::<String>();
            i = colon + 1;
            let value = scan_prefixed_value(&chars, &mut i, &prefix);
            (Some(prefix), value)
        } else {
            i = prefix_start;
            (None, scan_value(&chars, &mut i))
        };
        let raw = chars[token_start..i].iter().collect::<String>();
        tokens.push(QueryToken {
            raw,
            negated,
            prefix,
            value,
        });
    }
    tokens
}

fn scan_prefixed_value(chars: &[char], pos: &mut usize, prefix: &str) -> String {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
    if !prefix.eq_ignore_ascii_case("in") || *pos < chars.len() && chars[*pos] == '"' {
        return scan_value(chars, pos);
    }

    let start = *pos;
    if starts_prefix_token(chars, start) {
        return String::new();
    }
    while *pos < chars.len() {
        if starts_next_prefix(chars, *pos) {
            break;
        }
        *pos += 1;
    }
    chars[start..*pos]
        .iter()
        .collect::<String>()
        .trim()
        .to_string()
}

fn starts_prefix_token(chars: &[char], pos: usize) -> bool {
    if pos >= chars.len() {
        return false;
    }
    let mut i = pos;
    if chars[i] == '-' {
        i += 1;
    }
    let start = i;
    while i < chars.len() && !chars[i].is_whitespace() {
        if chars[i] == ':' {
            let prefix = chars[start..i]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            return is_known_prefix(&prefix);
        }
        i += 1;
    }
    false
}

fn starts_next_prefix(chars: &[char], pos: usize) -> bool {
    if pos >= chars.len() || !chars[pos].is_whitespace() {
        return false;
    }
    let mut i = pos;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    starts_prefix_token(chars, i)
}

fn is_known_prefix(prefix: &str) -> bool {
    matches!(
        prefix,
        "f" | "from"
            | "sender"
            | "subject"
            | "s"
            | "body"
            | "preview"
            | "is"
            | "has"
            | "tag"
            | "keyword"
            | "in"
            | "mailbox"
            | "source"
            | "account"
            | "id"
            | "thread"
            | "threadid"
            | "conversation"
            | "conversationid"
            | "conv"
            | "before"
            | "after"
            | "date"
            | "newer"
            | "older"
    )
}

fn scan_value(chars: &[char], pos: &mut usize) -> String {
    if *pos < chars.len() && chars[*pos] == '"' {
        *pos += 1;
        let start = *pos;
        while *pos < chars.len() && chars[*pos] != '"' {
            *pos += 1;
        }
        let value = chars[start..*pos].iter().collect();
        if *pos < chars.len() {
            *pos += 1;
        }
        return value;
    }
    let start = *pos;
    while *pos < chars.len() && !chars[*pos].is_whitespace() {
        *pos += 1;
    }
    chars[start..*pos].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_preserves_query_after_quoted_in_selector() {
        let tokens = tokenize("in:\"acct-a/inbox\" from:Alex");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].prefix.as_deref(), Some("in"));
        assert_eq!(tokens[0].value, "acct-a/inbox");
        assert_eq!(tokens[1].raw, "from:Alex");
    }

    #[test]
    fn tokenizer_preserves_spaced_in_selector_until_next_prefix() {
        let tokens = tokenize("in:All Mail from:Alex");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].value, "All Mail");
        assert_eq!(tokens[1].raw, "from:Alex");
    }

    #[test]
    fn tokenizer_marks_negated_in_for_resolution() {
        let tokens = tokenize("-in:Inbox subject:hello");
        assert_eq!(tokens[0].prefix.as_deref(), Some("in"));
        assert!(tokens[0].negated);
        assert_eq!(tokens[1].raw, "subject:hello");
    }
}
