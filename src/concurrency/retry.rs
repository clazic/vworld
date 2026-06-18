//! 재시도 단일화 — 429/5xx/타임아웃 backoff(설계 §2.1.1-c). 곱연산 backoff 금지.

use std::time::Duration;

/// 재시도 정책 파라미터.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retry: u32,
    /// 429 작업 재투입 상한(워커 통합 예정·§3.4).
    #[allow(dead_code)]
    pub max_requeue: u32,
    pub base_backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_retry: 3,
            max_requeue: 3,
            base_backoff_ms: 200,
        }
    }
}

impl RetryPolicy {
    /// attempt(0-기반)에 대한 지수 backoff 지연.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let mult = 1u64 << attempt.min(6); // 상한으로 폭주 방지.
        Duration::from_millis(self.base_backoff_ms.saturating_mul(mult))
    }
}

/// 분류된 요청 실패 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    /// 429 — 키 쿨다운 반납 + 재투입.
    RateLimited,
    /// 5xx/타임아웃/연결오류 — backoff 재시도.
    Transient,
    /// 4xx(429 제외)/본문 에러 — 재시도 무의미.
    Permanent,
}

/// reqwest 에러를 FailKind로 분류.
pub fn classify_reqwest(err: &reqwest::Error) -> FailKind {
    if err.is_timeout() || err.is_connect() {
        return FailKind::Transient;
    }
    if let Some(status) = err.status() {
        let code = status.as_u16();
        if code == 429 {
            return FailKind::RateLimited;
        }
        if (500..600).contains(&code) {
            return FailKind::Transient;
        }
        return FailKind::Permanent;
    }
    FailKind::Transient
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_exponential_not_multiplicative() {
        let p = RetryPolicy::default();
        assert_eq!(p.backoff(0), Duration::from_millis(200));
        assert_eq!(p.backoff(1), Duration::from_millis(400));
        assert_eq!(p.backoff(2), Duration::from_millis(800));
    }

    #[test]
    fn backoff_caps_shift() {
        let p = RetryPolicy::default();
        // attempt가 커도 shift 상한(6)으로 폭주하지 않음.
        assert_eq!(p.backoff(100), p.backoff(6));
    }
}
