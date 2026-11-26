//! 화면에 보낼 작은 JSON 을 만든다.

pub fn text(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // 스크립트 안에 넣으므로 태그가 닫히지 않게, 줄 바꿈 문자도 막는다.
            '<' => out.push_str("\\u003c"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn optional(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), text)
}

pub fn object(fields: &[(&str, String)]) -> String {
    let body: Vec<String> = fields
        .iter()
        .map(|(key, value)| format!("{}:{value}", text(key)))
        .collect();
    format!("{{{}}}", body.join(","))
}

pub fn array(items: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", items.into_iter().collect::<Vec<_>>().join(","))
}

/// 화면에서 온 명령을 읽는다. 명령은 `{"이름": 값}` 하나뿐이라 그만큼만 읽는다.
pub fn command(body: &str) -> Option<(String, String)> {
    let inner = body.trim().strip_prefix('{')?.strip_suffix('}')?.trim();
    let (key, value) = inner.split_once(':')?;
    let key = key.trim().strip_prefix('"')?.strip_suffix('"')?;
    let value = value.trim();
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value);
    Some((key.to_owned(), value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_what_could_break_out() {
        assert_eq!(text("a\"b\\c\n</script>"), r#""a\"b\\c\n\u003c/script>""#);
    }

    #[test]
    fn reads_commands() {
        assert_eq!(
            command(r#"{"pair":"R3CN704E5EX"}"#),
            Some(("pair".into(), "R3CN704E5EX".into()))
        );
        assert_eq!(
            command(r#"{"answer":true}"#),
            Some(("answer".into(), "true".into()))
        );
        assert_eq!(command("nonsense"), None);
    }
}
