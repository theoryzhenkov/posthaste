// Tokenizer
// ---------------------------------------------------------------------------

pub(super) struct Token {
    pub(super) negated: bool,
    pub(super) prefix: Option<String>,
    pub(super) value: String,
}

/// Splits input on whitespace, respecting `"quoted strings"` and `-` negation.
pub(super) fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // skip whitespace
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }

        let negated = chars[i] == '-' && i + 1 < len && !chars[i + 1].is_whitespace();
        if negated {
            i += 1;
        }

        // scan for prefix (everything before the first ':')
        let start = i;
        let mut colon_pos = None;
        while i < len && !chars[i].is_whitespace() {
            if chars[i] == ':' && colon_pos.is_none() {
                colon_pos = Some(i);
                break;
            }
            i += 1;
        }

        if let Some(cp) = colon_pos {
            let prefix: String = chars[start..cp].iter().collect();
            i = cp + 1; // skip ':'

            let value = scan_prefixed_value(&chars, &mut i, &prefix);
            tokens.push(Token {
                negated,
                prefix: Some(prefix),
                value,
            });
        } else {
            // no colon -- this is free text; rescan from `start`
            i = start;
            let value = scan_value(&chars, &mut i);
            tokens.push(Token {
                negated,
                prefix: None,
                value,
            });
        }
    }

    tokens
}

fn scan_prefixed_value(chars: &[char], pos: &mut usize, prefix: &str) -> String {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }

    let prefix = prefix.to_ascii_lowercase();
    if !prefix_accepts_spaced_value(&prefix) {
        return scan_value(chars, pos);
    }

    if *pos < chars.len() && chars[*pos] == '"' {
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
            let prefix: String = chars[start..i]
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
    if i < chars.len() && chars[i] == '-' {
        i += 1;
    }

    let start = i;
    while i < chars.len() && !chars[i].is_whitespace() {
        if chars[i] == ':' {
            let prefix: String = chars[start..i]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            return is_known_prefix(&prefix);
        }
        i += 1;
    }

    false
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

fn prefix_accepts_spaced_value(prefix: &str) -> bool {
    matches!(
        prefix,
        "f" | "from"
            | "sender"
            | "subject"
            | "s"
            | "body"
            | "preview"
            | "tag"
            | "keyword"
            | "in"
            | "mailbox"
            | "source"
            | "account"
    )
}

/// Reads a value starting at `chars[*pos]`. Handles quoted strings.
fn scan_value(chars: &[char], pos: &mut usize) -> String {
    let len = chars.len();
    if *pos < len && chars[*pos] == '"' {
        // quoted value
        *pos += 1; // skip opening quote
        let start = *pos;
        while *pos < len && chars[*pos] != '"' {
            *pos += 1;
        }
        let value: String = chars[start..*pos].iter().collect();
        if *pos < len {
            *pos += 1; // skip closing quote
        }
        value
    } else {
        let start = *pos;
        while *pos < len && !chars[*pos].is_whitespace() {
            *pos += 1;
        }
        chars[start..*pos].iter().collect()
    }
}

// ---------------------------------------------------------------------------
// Token -> MailQueryRuleNode mapping
// ---------------------------------------------------------------------------
