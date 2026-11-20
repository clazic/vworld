//! PES 중계 모드 시험 — 중계 흉내 서버(127.0.0.1, 평문 HTTP)를 띄우고 실제 `vworld` 바이너리를 돌린다.
//!
//! 외부(VWorld · GitHub) 호출 없음. 홈 디렉터리는 시험마다 임시 폴더로 바꾼다(`~/.vworld` 를 건드리지 않음).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const TOKEN: &str = "tok-123";
/// 설정파일에 넣어 두는 가짜 "실제 키" — 중계 모드에서 절대 나가면 안 된다.
const CONFIG_KEY: &str = "CONFIGKEY-MUST-NOT-LEAK";
const CONFIG_DOMAIN: &str = "config-domain.example";
const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
const GEOCODE_OK: &str =
    r#"{"response":{"status":"OK","result":{"crs":"EPSG:4326","point":{"x":"126.978","y":"37.566"}}}}"#;

/// 받은 요청 1건.
#[derive(Debug, Clone)]
struct Req {
    /// 요청 줄의 경로+질의(퍼센트 인코딩 그대로).
    target: String,
    /// 머리글(이름은 소문자).
    headers: Vec<(String, String)>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }
    /// 질의 값(퍼센트 디코딩).
    fn query(&self, name: &str) -> Option<String> {
        let q = self.target.split_once('?')?.1;
        q.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == name).then(|| percent_decode(v))
        })
    }
    /// 요청 전체(경로 · 질의 · 머리글)에 글이 들어 있는지.
    fn mentions(&self, needle: &str) -> bool {
        percent_decode(&self.target).contains(needle)
            || self.headers.iter().any(|(_, v)| v.contains(needle))
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = std::str::from_utf8(&b[i + 1..i + 3]).map_err(|_| ()).and_then(|h| u8::from_str_radix(h, 16).map_err(|_| ())) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 흉내 서버가 돌려줄 응답.
struct Resp {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Resp {
    fn ok(body: impl Into<Vec<u8>>) -> Resp {
        Resp { status: 200, headers: vec![], body: body.into() }
    }
    fn with(mut self, k: &'static str, v: &str) -> Resp {
        self.headers.push((k, v.to_string()));
        self
    }
}

/// 중계 흉내 서버 — 요청을 적어 두고, `handler(몇 번째, 요청)` 이 준 응답을 돌려준다.
struct Mock {
    url: String,
    reqs: Arc<Mutex<Vec<Req>>>,
}

impl Mock {
    fn start(handler: impl Fn(usize, &Req) -> Resp + Send + Sync + 'static) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let reqs: Arc<Mutex<Vec<Req>>> = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(handler);
        let counter = Arc::new(AtomicUsize::new(0));
        let reqs2 = Arc::clone(&reqs);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let handler = Arc::clone(&handler);
                let reqs = Arc::clone(&reqs2);
                let counter = Arc::clone(&counter);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    let mut headers = Vec::new();
                    loop {
                        let mut h = String::new();
                        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                            break;
                        }
                        if let Some((k, v)) = h.split_once(':') {
                            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
                        }
                    }
                    let req = Req { target, headers };
                    let n = counter.fetch_add(1, Ordering::SeqCst);
                    reqs.lock().unwrap().push(req.clone());
                    let resp = handler(n, &req);
                    let mut head = format!(
                        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                        resp.status,
                        resp.body.len()
                    );
                    for (k, v) in &resp.headers {
                        head.push_str(&format!("{k}: {v}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&resp.body);
                    let _ = stream.flush();
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    let _ = reader.read(&mut [0u8; 16]);
                });
            }
        });
        Mock { url, reqs }
    }

    fn requests(&self) -> Vec<Req> {
        self.reqs.lock().unwrap().clone()
    }
}

/// 시험별 임시 홈. `with_config` 면 가짜 키 · 도메인이 든 `~/.vworld/config.toml` 을 만든다.
fn temp_home(name: &str, with_config: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vworld_relay_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".vworld")).unwrap();
    if with_config {
        std::fs::write(dir.join(".vworld").join("config.toml"), config_text()).unwrap();
    }
    dir
}

fn config_text() -> String {
    format!("referer = \"https://{CONFIG_DOMAIN}\"\n\n[[keys]]\nkey = \"{CONFIG_KEY}\"\nalias = \"local\"\n")
}

/// `vworld` 실행 — 환경은 시험이 정한 것만(PES_* · CI 는 지우고 시작).
fn vworld(home: &Path, envs: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vworld"));
    cmd.args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("VWORLD_NO_UPDATE_CHECK", "1")
        .env_remove("CI")
        .env_remove("PES_RELAY_URL")
        .env_remove("PES_RELAY_TOKEN")
        .env_remove("PES_RELAY_VWORLD_KEYS")
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy");
    for (k, v) in envs {
        if v.is_empty() {
            cmd.env_remove(k);
        } else {
            cmd.env(k, v);
        }
    }
    cmd.output().unwrap()
}

fn relay_env<'a>(mock: &'a Mock, keys: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("PES_RELAY_URL", mock.url.as_str()),
        ("PES_RELAY_TOKEN", TOKEN),
        ("PES_RELAY_VWORLD_KEYS", keys),
    ]
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// 모든 요청이 규약대로인지: 중계 경로 · 토큰 머리글 · 설정파일 키/도메인 없음.
fn assert_relay_shape(reqs: &[Req], prefix: &str) {
    assert!(!reqs.is_empty(), "중계로 온 요청이 없음");
    for r in reqs {
        assert!(r.path().starts_with(prefix), "경로가 중계 꼴이 아님: {}", r.target);
        assert_eq!(r.header("x-pes-relay-token"), Some(TOKEN), "토큰 머리글 없음: {r:?}");
        assert!(!r.mentions(CONFIG_KEY), "설정파일 키가 나감: {r:?}");
        assert!(!r.mentions(CONFIG_DOMAIN), "설정파일 도메인이 나감: {r:?}");
    }
}

#[test]
fn geocode_goes_through_relay_with_placeholders() {
    let mock = Mock::start(|_, _| Resp::ok(GEOCODE_OK));
    let home = temp_home("geocode", true);
    let out = vworld(&home, &relay_env(&mock, "2"), &["geocode", "세종대로 110", "--type", "road"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("126.978"));

    let reqs = mock.requests();
    assert_relay_shape(&reqs, "/vworld/https/api.vworld.kr/req/address");
    let r = &reqs[0];
    assert_eq!(r.query("key").as_deref(), Some("__PES_KEY_0__"));
    assert_eq!(r.query("domain").as_deref(), Some("__PES_DOMAIN__"));
    assert_eq!(r.header("referer"), Some("__PES_DOMAIN__"));
    // 방식 · 질의는 그대로.
    assert_eq!(r.query("service").as_deref(), Some("address"));
    assert_eq!(r.query("address").as_deref(), Some("세종대로 110"));
}

#[test]
fn config_flag_keys_are_ignored_and_explicit_referer_wins() {
    let mock = Mock::start(|_, _| Resp::ok(GEOCODE_OK));
    let home = temp_home("cfgflag", false);
    let cfg = home.join("other.toml");
    std::fs::write(&cfg, config_text()).unwrap();
    let out = vworld(
        &home,
        &relay_env(&mock, "1"),
        &["--config", cfg.to_str().unwrap(), "--referer", "https://mine.example", "geocode", "세종대로 110", "--type", "road"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let reqs = mock.requests();
    assert_relay_shape(&reqs, "/vworld/https/api.vworld.kr/req/address");
    assert_eq!(reqs[0].query("key").as_deref(), Some("__PES_KEY_0__"));
    // 명령줄에 직접 준 --referer 는 자리표시보다 우선.
    assert_eq!(reqs[0].query("domain").as_deref(), Some("https://mine.example"));
    assert_eq!(reqs[0].header("referer"), Some("https://mine.example"));
}

#[test]
fn geocoder_host_uses_apikey_placeholder() {
    let mock = Mock::start(|_, _| {
        Resp::ok(r#"{"EPSG_4326_X":"127.0","EPSG_4326_Y":"37.5","ADDR":"지번 주소","NEW_JUSO":"도로명 주소"}"#)
    });
    let home = temp_home("geocoder", true);
    let out = vworld(&home, &relay_env(&mock, "1"), &["geocoder", "서울 중구 태평로1가 31"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let reqs = mock.requests();
    assert_relay_shape(&reqs, "/vworld/https/apis.vworld.kr/");
    assert_eq!(reqs.len(), 3);
    assert_eq!(reqs[0].path(), "/vworld/https/apis.vworld.kr/jibun2coord.do");
    for r in &reqs {
        assert_eq!(r.query("apiKey").as_deref(), Some("__PES_KEY_0__"));
    }
}

#[test]
fn tile_path_carries_placeholder_and_binary_body_is_kept() {
    let mock = Mock::start(|_, _| Resp::ok(PNG));
    let home = temp_home("tile", true);
    let out_file = home.join("t.png");
    let out = vworld(
        &home,
        &relay_env(&mock, "1"),
        &["tile", "wmts", "--z", "14", "--row", "6449", "--col", "13969", "-o", out_file.to_str().unwrap()],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let reqs = mock.requests();
    assert_relay_shape(&reqs, "/vworld/https/api.vworld.kr/req/wmts/");
    assert_eq!(
        reqs[0].target,
        "/vworld/https/api.vworld.kr/req/wmts/1.0.0/__PES_KEY_0__/Base/14/6449/13969.png"
    );
    assert_eq!(std::fs::read(&out_file).unwrap(), PNG);
}

#[test]
fn redirect_location_is_rewrapped_not_followed_directly() {
    // 첫 응답: 기관 주소로 넘김. CLI 는 그 주소를 다시 중계 주소로 바꿔 요청해야 한다.
    let mock = Mock::start(|n, _| {
        if n == 0 {
            Resp { status: 302, headers: vec![], body: vec![] }
                .with("Location", "https://map.vworld.kr/moved/a.png?key=__PES_KEY_0__&v=2")
        } else {
            Resp::ok(PNG)
        }
    });
    let home = temp_home("redirect", true);
    let out_file = home.join("s.png");
    let out = vworld(
        &home,
        &relay_env(&mock, "1"),
        &["staticmap", "127.0,37.5", "--zoom", "14", "-o", out_file.to_str().unwrap()],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 2);
    assert_relay_shape(&reqs, "/vworld/https/");
    assert_eq!(reqs[0].path(), "/vworld/https/api.vworld.kr/req/image");
    assert_eq!(reqs[1].target, "/vworld/https/map.vworld.kr/moved/a.png?key=__PES_KEY_0__&v=2");
    assert_eq!(std::fs::read(&out_file).unwrap(), PNG);
}

#[test]
fn zero_or_missing_key_count_stops_before_any_request() {
    let mock = Mock::start(|_, _| Resp::ok(GEOCODE_OK));
    let home = temp_home("nokeys", true);
    let msg = "PES 에 VWorld 키가 없어요. PES 「조사 도구 · 그림 키」에서 키를 넣으세요.";
    for keys in ["0", ""] {
        let out = vworld(&home, &relay_env(&mock, keys), &["geocode", "세종대로 110"]);
        assert!(!out.status.success());
        assert!(stderr(&out).contains(msg), "stderr: {}", stderr(&out));
    }
    assert!(mock.requests().is_empty(), "키가 없는데 중계를 불렀음");
}

#[test]
fn relay_error_is_shown_and_not_retried() {
    let mock = Mock::start(|_, _| {
        Resp { status: 409, headers: vec![], body: r#"{"error":"중계가 알려 준 오류 글"}"#.into() }
            .with("X-PES-Relay-Error", "1")
    });
    let home = temp_home("relayerr", true);
    // 키가 셋이어도, 유형 자동 폴백(road→parcel)이 있어도 한 번만 부른다.
    let out = vworld(&home, &relay_env(&mock, "3"), &["geocode", "세종대로 110"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("중계가 알려 준 오류 글"), "stderr: {}", stderr(&out));
    assert_eq!(mock.requests().len(), 1);
}

#[test]
fn relay_error_fails_a_batch_instead_of_per_item_errors() {
    let mock = Mock::start(|_, _| {
        Resp { status: 401, headers: vec![], body: r#"{"error":"토큰이 틀려요"}"#.into() }
            .with("X-PES-Relay-Error", "1")
    });
    let home = temp_home("relayerr_batch", true);
    let list = home.join("addrs.txt");
    std::fs::write(&list, "주소1\n주소2\n주소3\n주소4\n주소5\n주소6\n주소7\n주소8\n").unwrap();
    let out = vworld(
        &home,
        &relay_env(&mock, "2"),
        &["batch", "geocode", "--from", list.to_str().unwrap(), "--concurrency", "2"],
    );
    assert!(!out.status.success(), "stdout: {}", stdout(&out));
    assert!(stderr(&out).contains("토큰이 틀려요"), "stderr: {}", stderr(&out));
    // 이미 떠난 요청(워커 수)까지만 — 남은 항목은 중계로 보내지 않는다.
    assert!(mock.requests().len() <= 2, "요청 {}건", mock.requests().len());
}

#[test]
fn key_embedding_commands_fail_and_offline_commands_work() {
    let mock = Mock::start(|_, _| Resp::ok(GEOCODE_OK));
    let home = temp_home("artifacts", true);
    let env = relay_env(&mock, "1");
    let html = home.join("m.html");
    let html = html.to_str().unwrap();

    // 실제 키가 박히는 산출물 — 중계 모드에서는 만들지 않는다.
    for args in [
        vec!["map", "2d", "-o", html],
        vec!["map", "2d"],
        vec!["wfs", "--request", "GetFeature", "--typename", "lp_pa_cbnd_bubun", "--bbox", "1,2,3,4", "-o", html],
    ] {
        let out = vworld(&home, &env, &args);
        assert!(!out.status.success(), "{args:?} 가 성공함");
        assert!(stderr(&out).contains("PES 중계 모드"), "{args:?}: {}", stderr(&out));
        assert!(!stdout(&out).contains("__PES_KEY_"), "{args:?}: 자리표시가 산출물에 나감");
    }
    assert!(!Path::new(html).exists());

    // 오프라인 명령은 그대로.
    for args in [
        vec!["config", "path"],
        vec!["config", "list-keys"],
        vec!["data", "layers", "--search", "지적"],
        vec!["ned", "--list"],
        vec!["map", "--analysis", "list"],
    ] {
        let out = vworld(&home, &env, &args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
    }
    assert!(mock.requests().is_empty());
}

#[test]
fn update_notice_is_off_in_relay_mode_only() {
    let mock = Mock::start(|_, _| Resp::ok(GEOCODE_OK));
    let home = temp_home("notice", false);
    // 방금 확인한 것처럼 캐시를 써 둔다(네트워크 없이 알림만 판단).
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    std::fs::write(home.join(".vworld").join(".update-check"), format!("{now}\nv999.0.0\n")).unwrap();

    let plain = vworld(&home, &[("VWORLD_NO_UPDATE_CHECK", "")], &["config", "path"]);
    assert!(stderr(&plain).contains("새 버전"), "평소에는 알림이 떠야 함: {}", stderr(&plain));

    let mut env = relay_env(&mock, "1");
    env.push(("VWORLD_NO_UPDATE_CHECK", ""));
    let relayed = vworld(&home, &env, &["config", "path"]);
    assert!(relayed.status.success());
    assert!(!stderr(&relayed).contains("새 버전"), "중계 모드에서 알림이 뜸: {}", stderr(&relayed));
    assert!(mock.requests().is_empty());
}

#[test]
fn without_relay_env_the_cli_reads_config_as_before() {
    // 중계 변수가 없으면 설정파일 키를 찾는다 — 키가 없으면 예전 그대로의 안내.
    let home = temp_home("plain", false);
    let out = vworld(&home, &[], &["geocode", "세종대로 110"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("등록된 키가 없습니다"), "stderr: {}", stderr(&out));

    // 키 수 변수만 있고 PES_RELAY_URL 이 없으면 중계 모드가 아니다.
    let out = vworld(&home, &[("PES_RELAY_VWORLD_KEYS", "3"), ("PES_RELAY_TOKEN", TOKEN)], &["geocode", "세종대로 110"]);
    assert!(stderr(&out).contains("등록된 키가 없습니다"), "stderr: {}", stderr(&out));
}

#[tokio::test]
async fn plain_client_sends_urls_untouched() {
    // 중계가 아닌 클라이언트(`Client::new` — vworld-mcp 도 이것을 쓴다): 주소 그대로, 토큰 머리글 없음,
    // 넘김은 예전처럼 자동으로 따라간다.
    let mock = Mock::start(|n, _| {
        if n == 0 {
            Resp { status: 302, headers: vec![], body: vec![] }.with("Location", "/req/moved?x=1")
        } else {
            Resp::ok("{}")
        }
    });
    let client = vworld::api::Client::new().unwrap();
    let auth = vworld::api::Auth { key: "REALKEY".into(), domain: Some("d.example".into()) };
    let body = client
        .get_text(&format!("{}/req/address", mock.url), vec![("a".into(), "1".into())], &auth)
        .await
        .unwrap();
    assert_eq!(body, "{}");
    let reqs = mock.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].path(), "/req/address");
    assert_eq!(reqs[0].query("key").as_deref(), Some("REALKEY"));
    assert_eq!(reqs[0].query("domain").as_deref(), Some("d.example"));
    assert_eq!(reqs[0].header("referer"), Some("d.example"));
    assert!(reqs.iter().all(|r| r.header("x-pes-relay-token").is_none()));
    assert_eq!(reqs[1].target, "/req/moved?x=1");
}
