//! 응답 정규화 — 200 본문 에러 검사(§2.1.1-b), XML/GML→JSON 트리(§1.1.1).

use super::ApiError;
use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

/// 텍스트 응답을 JSON Value로 파싱(JSON 우선, 실패 시 XML 정규화).
///
/// VWorld는 에러 응답 본문에 **이스케이프 안 된 따옴표**(예: `단일검색="Y"`)를 넣어
/// 표준 JSON 파싱을 깨뜨리는 경우가 있다. serde 실패 시 정규식으로 에러 상태를
/// 복원해 `check_body_error`가 정상 동작하도록 한다(크래시 방지).
pub fn parse_to_json(body: &str) -> Result<Value> {
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        match serde_json::from_str::<Value>(body) {
            Ok(v) => Ok(v),
            Err(e) => salvage_error_json(body)
                .ok_or_else(|| anyhow!("JSON 파싱 실패: {e}")),
        }
    } else if trimmed.starts_with('<') {
        xml_to_json(body)
    } else {
        Err(anyhow!("알 수 없는 응답 포맷: {}", &body.chars().take(80).collect::<String>()))
    }
}

/// 깨진 JSON에서 VWorld 에러(status/error.code/error.text)를 정규식으로 복원.
///
/// 성공/정상 데이터(features 등)는 복원 대상이 아니다 — 에러 응답만 구제한다.
fn salvage_error_json(body: &str) -> Option<Value> {
    // status 추출.
    let status = field_after(body, "status")?;
    if status == "OK" {
        // 정상인데 파싱 실패면 데이터 손상 — 구제하지 않음(상위에서 에러로).
        return None;
    }
    let code = field_after(body, "code").unwrap_or_else(|| "ERROR".to_string());
    // text는 따옴표가 깨졌을 수 있으니 라벨 이후 ~200자를 best-effort로.
    let text = field_after(body, "text")
        .or_else(|| field_after(body, "message"))
        .unwrap_or_else(|| "응답 본문 파싱 실패(에러 응답)".to_string());
    Some(serde_json::json!({
        "response": { "status": status, "error": { "code": code, "text": text } }
    }))
}

/// `"<key>" : "<value>"` 패턴에서 value를 추출(value 내부 따옴표 손상에 관대).
fn field_after(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let start = body.find(&pat)? + pat.len();
    let rest = &body[start..];
    // 콜론 이후 첫 따옴표 열기.
    let q1 = rest.find('"')?;
    let after = &rest[q1 + 1..];
    // 닫는 따옴표 — 다음 `",` 또는 `"}` 경계를 우선 사용(내부 따옴표 관대).
    let end = after
        .find("\",")
        .or_else(|| after.find("\"}"))
        .or_else(|| after.find('"'))?;
    Some(after[..end].to_string())
}

/// VWorld 응답의 `response.status`/`error`를 검사 → 에러면 ApiError.
///
/// "데이터 없음"(NOT_FOUND 등)은 `empty_ok=true`로 표시(배치에서 빈 결과 정상 처리).
pub fn check_body_error(value: &Value) -> std::result::Result<(), ApiError> {
    // VWorld REST: { "response": { "status": "OK|NOT_FOUND|ERROR", "error": {...} } }
    let resp = value.get("response").unwrap_or(value);
    let status = resp
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("OK");

    match status {
        "OK" => Ok(()),
        "NOT_FOUND" => Err(ApiError {
            code: "NOT_FOUND".into(),
            text: "결과 없음".into(),
            empty_ok: true,
        }),
        _ => {
            let (code, text) = extract_error(resp);
            Err(ApiError {
                code,
                text,
                empty_ok: false,
            })
        }
    }
}

fn extract_error(resp: &Value) -> (String, String) {
    if let Some(err) = resp.get("error") {
        let code = err
            .get("code")
            .or_else(|| err.get("level"))
            .and_then(|c| c.as_str())
            .unwrap_or("ERROR")
            .to_string();
        let text = err
            .get("text")
            .or_else(|| err.get("message"))
            .and_then(|t| t.as_str())
            .unwrap_or("알 수 없는 오류")
            .to_string();
        (code, text)
    } else {
        ("ERROR".into(), "상태=ERROR".into())
    }
}

/// 스택 기반 XML 노드 빌더 항목.
struct Node {
    name: String,
    map: Map<String, Value>,
    text: String,
}

impl Node {
    fn new(name: String) -> Self {
        Node {
            name,
            map: Map::new(),
            text: String::new(),
        }
    }

    fn into_value(self) -> Value {
        if self.map.is_empty() {
            if self.text.is_empty() {
                Value::Null
            } else {
                Value::String(self.text)
            }
        } else {
            let mut m = self.map;
            if !self.text.is_empty() {
                m.insert("#text".into(), Value::String(self.text));
            }
            Value::Object(m)
        }
    }

    /// 자식 (key→value)을 부착. 중복 키는 배열로 승격.
    fn attach(&mut self, key: String, value: Value) {
        match self.map.get_mut(&key) {
            Some(Value::Array(arr)) => arr.push(value),
            Some(existing) => {
                let prev = existing.take();
                *existing = Value::Array(vec![prev, value]);
            }
            None => {
                self.map.insert(key, value);
            }
        }
    }
}

/// XML/GML → JSON 트리 정규화(§1.1.1 규칙: 속성=`@attr`, 텍스트=`#text`, 반복=배열).
pub fn xml_to_json(xml: &str) -> Result<Value> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut stack: Vec<Node> = vec![Node::new("#root".to_string())];

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let mut node = Node::new(local_name(e.name().as_ref()));
                for attr in e.attributes().flatten() {
                    let key = format!("@{}", local_name(attr.key.as_ref()));
                    let val = attr.unescape_value().map(|c| c.into_owned()).unwrap_or_default();
                    node.map.insert(key, Value::String(val));
                }
                stack.push(node);
            }
            Ok(Event::Empty(e)) => {
                let mut node = Node::new(local_name(e.name().as_ref()));
                for attr in e.attributes().flatten() {
                    let key = format!("@{}", local_name(attr.key.as_ref()));
                    let val = attr.unescape_value().map(|c| c.into_owned()).unwrap_or_default();
                    node.map.insert(key, Value::String(val));
                }
                let name = node.name.clone();
                let value = node.into_value();
                stack.last_mut().unwrap().attach(name, value);
            }
            Ok(Event::Text(e)) => {
                if let Ok(t) = e.unescape() {
                    stack.last_mut().unwrap().text.push_str(t.trim());
                }
            }
            Ok(Event::End(_)) => {
                let node = stack.pop().unwrap();
                let name = node.name.clone();
                let value = node.into_value();
                stack.last_mut().unwrap().attach(name, value);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(anyhow!("XML 파싱 오류: {e}")),
        }
    }

    Ok(stack.pop().unwrap().into_value())
}

fn local_name(qname: &[u8]) -> String {
    let s = String::from_utf8_lossy(qname);
    match s.rsplit(':').next() {
        Some(local) => local.to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_not_found_as_empty_ok() {
        let v = serde_json::json!({"response": {"status": "NOT_FOUND"}});
        let err = check_body_error(&v).unwrap_err();
        assert!(err.empty_ok);
        assert_eq!(err.code, "NOT_FOUND");
    }

    #[test]
    fn detects_error_status() {
        let v = serde_json::json!({
            "response": {"status": "ERROR", "error": {"code": "INVALID_KEY", "text": "키 오류"}}
        });
        let err = check_body_error(&v).unwrap_err();
        assert!(!err.empty_ok);
        assert_eq!(err.code, "INVALID_KEY");
    }

    #[test]
    fn ok_status_passes() {
        let v = serde_json::json!({"response": {"status": "OK", "result": {}}});
        assert!(check_body_error(&v).is_ok());
    }

    #[test]
    fn salvages_malformed_error_json() {
        // VWorld 실제 응답: text 내부에 이스케이프 안 된 따옴표.
        let body = r#"{"response" : {"status" : "ERROR", "error" : {"level" : "1", "code" : "INVALID_RANGE", "text" : "유효범위 초과 단일검색="Y" 포함 필요"}}}"#;
        // 표준 파싱은 실패하지만 parse_to_json은 에러를 복원해야 한다.
        let v = parse_to_json(body).expect("복원 성공");
        let err = check_body_error(&v).unwrap_err();
        assert_eq!(err.code, "INVALID_RANGE");
        assert!(!err.empty_ok);
    }

    #[test]
    fn xml_basic_normalize() {
        let xml = r#"<root><item id="1">A</item><item id="2">B</item></root>"#;
        let v = xml_to_json(xml).unwrap();
        let root = v.get("root").unwrap();
        let items = root.get("item").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("@id").unwrap(), "1");
        assert_eq!(items[0].get("#text").unwrap(), "A");
    }
}
