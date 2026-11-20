//! 요청별 키 사용량 — 키 목록의 자리(0부터)마다 VWorld 로 나간 HTTP 요청 수를 센다.
//!
//! `vworld-mcp` 가 요청별 키(`_meta["kr.pes/keys"]`)로 도구를 실행할 때만 만든다. `vworld` CLI 는
//! 만들지 않으므로 아무것도 세지 않는다. 키 원문은 담지 않는다(해시만) — 보고에는 자리 번호와 횟수뿐이다.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

/// 키 한 개의 횟수.
#[derive(Default)]
struct Counts {
    /// 이 키로 보낸 HTTP 요청(재시도도 한 번씩 센다).
    calls: AtomicU64,
    /// 그중 실패(전송 오류 · HTTP 400 이상 · VWorld 오류 본문).
    errors: AtomicU64,
    /// 그중 한도 초과(HTTP 429 · VWorld 사용량 초과 오류). `errors` 에도 들어 있다.
    limited: AtomicU64,
}

/// 도구 호출 한 번의 키별 사용량(여러 작업이 함께 써도 안전).
pub struct KeyUsage {
    /// (키 해시, 받은 목록에서 처음 나온 자리).
    index: Vec<(u64, usize)>,
    /// 받은 목록과 같은 길이(겹친 키의 뒷자리는 늘 0).
    counts: Vec<Counts>,
}

fn key_hash(k: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    k.hash(&mut h);
    h.finish()
}

impl KeyUsage {
    /// 받은 순서 그대로의 키 목록으로 만든다. 같은 키가 또 나오면 처음 나온 자리로 센다.
    pub fn new<S: AsRef<str>>(keys: &[S]) -> Self {
        let mut index: Vec<(u64, usize)> = Vec::new();
        for (i, k) in keys.iter().enumerate() {
            let h = key_hash(k.as_ref());
            if !k.as_ref().is_empty() && !index.iter().any(|(x, _)| *x == h) {
                index.push((h, i));
            }
        }
        KeyUsage {
            index,
            counts: keys.iter().map(|_| Counts::default()).collect(),
        }
    }

    fn slot(&self, key: &str) -> Option<&Counts> {
        let h = key_hash(key);
        let i = self.index.iter().find(|(x, _)| *x == h)?.1;
        self.counts.get(i)
    }

    /// HTTP 요청 한 번을 보냈다.
    pub fn record_call(&self, key: &str) {
        if let Some(c) = self.slot(key) {
            c.calls.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 보낸 요청 하나가 실패했다. `limited` 면 한도 초과로도 센다.
    pub fn record_error(&self, key: &str, limited: bool) {
        if let Some(c) = self.slot(key) {
            c.errors.fetch_add(1, Ordering::Relaxed);
            if limited {
                c.limited.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// 보고: `[{"i":0,"calls":3,"errors":1,"limited":1}, ...]`. 한 번도 안 쓴 키는 빼고,
    /// 요청을 하나도 안 보냈으면 `None`.
    pub fn report(&self) -> Option<Value> {
        let rows: Vec<Value> = self
            .counts
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let calls = c.calls.load(Ordering::Relaxed);
                (calls > 0).then(|| {
                    json!({
                        "i": i,
                        "calls": calls,
                        "errors": c.errors.load(Ordering::Relaxed),
                        "limited": c.limited.load(Ordering::Relaxed),
                    })
                })
            })
            .collect();
        (!rows.is_empty()).then_some(Value::Array(rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_by_first_index_and_skips_unused() {
        let u = KeyUsage::new(&["KEY-A", "KEY-A", "KEY-B", "KEY-C"]);
        assert_eq!(u.report(), None, "요청이 없으면 보고도 없다");
        u.record_call("KEY-A");
        u.record_call("KEY-A");
        u.record_error("KEY-A", true);
        u.record_call("KEY-B");
        u.record_error("KEY-B", false);
        u.record_call("NOT-IN-LIST"); // 목록 밖의 키는 세지 않는다.
        let r = u.report().unwrap();
        assert_eq!(
            r,
            json!([
                {"i": 0, "calls": 2, "errors": 1, "limited": 1},
                {"i": 2, "calls": 1, "errors": 1, "limited": 0},
            ])
        );
        let text = r.to_string();
        assert!(!text.contains("KEY-"), "키 원문이 없어야 한다: {text}");
    }

    #[test]
    fn concurrent_updates_are_not_lost() {
        let u = std::sync::Arc::new(KeyUsage::new(&["K0", "K1"]));
        let hs: Vec<_> = (0..8)
            .map(|t| {
                let u = u.clone();
                std::thread::spawn(move || {
                    for _ in 0..1000 {
                        u.record_call(if t % 2 == 0 { "K0" } else { "K1" });
                    }
                })
            })
            .collect();
        hs.into_iter().for_each(|h| h.join().unwrap());
        let r = u.report().unwrap();
        assert_eq!((r[0]["calls"].as_u64(), r[1]["calls"].as_u64()), (Some(4000), Some(4000)));
    }
}
