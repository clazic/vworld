//! 키 풀 — borrow/return(MPMC 채널), 라운드로빈 다중 발행, KeyLease RAII(설계 §3.1·§3.2).

use async_channel::{Receiver, Sender};
use std::time::Duration;

/// 키 풀 — 채널 용량 = 워커 수, 토큰 = 키 인덱스 `i%nKeys` 라운드로빈.
#[derive(Clone)]
pub struct KeyPool {
    tx: Sender<usize>,
    rx: Receiver<usize>,
}

impl KeyPool {
    /// 워커 수만큼 토큰을 키 인덱스 라운드로빈으로 채운다(키1개여도 워커N개 병렬).
    pub fn new(n_keys: usize, workers: usize) -> Self {
        let n_keys = n_keys.max(1);
        let cap = workers.max(1);
        let (tx, rx) = async_channel::bounded(cap);
        for i in 0..cap {
            // 용량=cap이므로 try_send는 항상 성공.
            let _ = tx.try_send(i % n_keys);
        }
        KeyPool { tx, rx }
    }

    /// 작업 1건마다 새 lease 획득(borrow = recv).
    pub async fn acquire(&self) -> Option<KeyLease> {
        let idx = self.rx.recv().await.ok()?;
        Some(KeyLease {
            idx,
            tx: self.tx.clone(),
            cooldown: None,
            returned: false,
        })
    }
}

/// RAII 키 토큰 — Drop에서 자동 반납(panic/abort 안전). 429는 쿨다운 지연 반납.
pub struct KeyLease {
    pub idx: usize,
    tx: Sender<usize>,
    cooldown: Option<Duration>,
    returned: bool,
}

impl KeyLease {
    /// 429 쿨다운 설정 — Drop 시 sleep 후 지연 send(동일 키 연타 방지).
    #[allow(dead_code)] // 429 쿨다운 반납 경로용 공개 API(워커 통합 예정·§3.4).
    pub fn set_cooldown(&mut self, d: Duration) {
        self.cooldown = Some(d);
    }
}

impl Drop for KeyLease {
    fn drop(&mut self) {
        if self.returned {
            return;
        }
        self.returned = true;
        let idx = self.idx;
        let tx = self.tx.clone();
        match self.cooldown {
            None => {
                // 즉시 반납 — 채널 용량=워커수, 빌린 lease ≤ 워커수 불변식 → try_send 항상 성공.
                let _ = tx.try_send(idx);
            }
            Some(d) => {
                tokio::spawn(async move {
                    tokio::time::sleep(d).await;
                    let _ = tx.send(idx).await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn single_key_multiple_workers_parallel() {
        // 키 1개 + 워커 4 → 토큰 4개 모두 인덱스 0, 동시 4 borrow 가능.
        let pool = KeyPool::new(1, 4);
        let l1 = pool.acquire().await.unwrap();
        let l2 = pool.acquire().await.unwrap();
        let l3 = pool.acquire().await.unwrap();
        let l4 = pool.acquire().await.unwrap();
        assert_eq!((l1.idx, l2.idx, l3.idx, l4.idx), (0, 0, 0, 0));
        drop(l1);
        // 반납 후 다시 획득 가능.
        let l5 = pool.acquire().await.unwrap();
        assert_eq!(l5.idx, 0);
        drop((l2, l3, l4, l5));
    }

    #[tokio::test]
    async fn round_robin_across_keys() {
        // 키 3개 + 워커 4 → 토큰 [0,1,2,0].
        let pool = KeyPool::new(3, 4);
        let mut got = Vec::new();
        for _ in 0..4 {
            got.push(pool.acquire().await.unwrap().idx);
        }
        got.sort();
        assert_eq!(got, vec![0, 0, 1, 2]);
    }

    #[tokio::test]
    async fn lease_returns_on_drop() {
        let pool = KeyPool::new(1, 1);
        {
            let _l = pool.acquire().await.unwrap();
            // 토큰 1개 전부 빌림.
        } // drop → 즉시 반납.
        // 반납됐으므로 즉시 다시 획득 가능.
        let l = pool.acquire().await.unwrap();
        assert_eq!(l.idx, 0);
    }
}
