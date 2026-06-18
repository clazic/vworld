//! 작업 큐 + 워커 — 인덱스 슬롯 결과 수집(순서 보존), 키 풀 borrow/return(설계 §3·§5.1).

use super::key_pool::KeyPool;
use crate::api::Auth;
use std::future::Future;
use std::sync::{Arc, Mutex};

/// 다건 작업을 병렬 실행하고 **입력 순서로** 결과를 수집한다.
///
/// - 워커 수 = `min(concurrency, jobs.len())` = 동시 in-flight 상한.
/// - 키 풀 용량 = 워커 수, 토큰 = 키 인덱스 라운드로빈(키1개여도 병렬).
/// - 작업 1건마다 새 KeyLease 획득·반납(작업-키 비고정).
pub async fn run_jobs<J, F, Fut, T>(
    jobs: Vec<J>,
    keys: Vec<Auth>,
    concurrency: usize,
    fetch: F,
) -> Vec<anyhow::Result<T>>
where
    J: Send + 'static,
    T: Send + 'static,
    F: Fn(J, Auth) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<T>> + Send,
{
    let n = jobs.len();
    if n == 0 {
        return Vec::new();
    }
    let keys = if keys.is_empty() {
        vec![Auth::default()]
    } else {
        keys
    };
    let workers = concurrency.clamp(1, n);
    let pool = KeyPool::new(keys.len(), workers);

    // 작업 큐(인덱스 부착).
    let (job_tx, job_rx) = async_channel::unbounded::<(usize, J)>();
    for (i, j) in jobs.into_iter().enumerate() {
        let _ = job_tx.send((i, j)).await;
    }
    job_tx.close();

    // 결과 슬롯.
    let slots: Arc<Mutex<Vec<Option<anyhow::Result<T>>>>> =
        Arc::new(Mutex::new((0..n).map(|_| None).collect()));
    let fetch = Arc::new(fetch);
    let keys = Arc::new(keys);

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let job_rx = job_rx.clone();
        let pool = pool.clone();
        let slots = Arc::clone(&slots);
        let fetch = Arc::clone(&fetch);
        let keys = Arc::clone(&keys);
        handles.push(tokio::spawn(async move {
            while let Ok((idx, job)) = job_rx.recv().await {
                // 작업 1건마다 새 lease 획득(루프 밖 1회 획득 금지).
                let lease = match pool.acquire().await {
                    Some(l) => l,
                    None => break,
                };
                let auth = keys[lease.idx % keys.len()].clone();
                let result = fetch(job, auth).await;
                // lease는 여기서 drop → 토큰 반납.
                drop(lease);
                slots.lock().unwrap()[idx] = Some(result);
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    // 슬롯 → 순서 보존 Vec.
    let mut out = slots.lock().unwrap();
    out.iter_mut()
        .map(|slot| slot.take().unwrap_or_else(|| Err(anyhow::anyhow!("미수집 작업"))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn preserves_order_and_runs_all() {
        let jobs: Vec<usize> = (0..50).collect();
        let keys = vec![Auth::default()];
        let results = run_jobs(jobs, keys, 8, |j, _auth| async move {
            // 입력값을 두 배로.
            Ok::<usize, anyhow::Error>(j * 2)
        })
        .await;
        assert_eq!(results.len(), 50);
        for (i, r) in results.iter().enumerate() {
            assert_eq!(*r.as_ref().unwrap(), i * 2);
        }
    }

    #[tokio::test]
    async fn concurrency_bounded_by_workers() {
        let counter = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let jobs: Vec<usize> = (0..20).collect();
        let c = Arc::clone(&counter);
        let p = Arc::clone(&peak);
        let results = run_jobs(jobs, vec![Auth::default()], 4, move |_j, _a| {
            let c = Arc::clone(&c);
            let p = Arc::clone(&p);
            async move {
                let cur = c.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(cur, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                c.fetch_sub(1, Ordering::SeqCst);
                Ok::<usize, anyhow::Error>(0)
            }
        })
        .await;
        assert_eq!(results.len(), 20);
        // 동시 in-flight는 워커 수(4)를 넘지 않음.
        assert!(peak.load(Ordering::SeqCst) <= 4);
    }
}
