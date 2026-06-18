//! 데이터 반환형 명령 핸들러 + 배치 진입점(설계 §1.3·§3·§5.1).

use super::GlobalArgs;
use crate::api::normalize;
use crate::api::{parse_passthrough, Auth, Client, NedBuilder, NedKind, QueryBuilder};
use crate::config::{resolve_config_path, Config};
use crate::ned_registry;
use crate::output;
use anyhow::{anyhow, Result};
use clap::Args;
use serde_json::Value;

// ───────────────────────── 공통 헬퍼 ─────────────────────────

/// config에서 키 풀(Auth 목록)을 로드. referer는 --referer > 키별 > 전역.
pub fn load_auths(g: &GlobalArgs) -> Result<Vec<Auth>> {
    let path = resolve_config_path(g.config.as_deref())?;
    let cfg = Config::load(&path)?;
    if cfg.keys.is_empty() {
        return Err(anyhow!(
            "등록된 키가 없습니다. `vworld config add-key <KEY>`로 추가하세요."
        ));
    }
    Ok(cfg
        .keys
        .iter()
        .map(|e| Auth {
            key: e.key.clone(),
            domain: g.referer.clone().or_else(|| cfg.referer_for(e)),
        })
        .collect())
}

/// 단건 데이터 요청: 호출 → (raw면 원문, 아니면 정규화 JSON) 출력.
async fn fetch_one(
    g: &GlobalArgs,
    client: &Client,
    url: String,
    params: Vec<(String, String)>,
    auth: &Auth,
) -> Result<()> {
    let body = client.get_text(&url, params, auth).await?;
    if g.raw {
        return output::print_raw_text(&body);
    }
    let value = normalize::parse_to_json(&body)?;
    if let Err(e) = normalize::check_body_error(&value) {
        if e.empty_ok {
            return output::print_json(g, &serde_json::json!({"ok": true, "result": Value::Null, "note": "결과 없음"}));
        }
        return Err(anyhow!("{e}"));
    }
    output::print_json(g, &serde_json::json!({"ok": true, "result": value}))
}

// ───────────────────────── geocode ─────────────────────────

#[derive(Args, Debug)]
pub struct GeocodeArgs {
    /// 주소(지오) 또는 좌표 "x,y"(역지오, --reverse 시).
    pub query: Option<String>,
    /// 역지오코딩(좌표→주소). query는 "x,y".
    #[arg(long)]
    pub reverse: bool,
    /// 주소 유형: auto | ROAD | PARCEL. auto는 도로명→지번 자동 판별·폴백.
    #[arg(long, default_value = "auto")]
    pub r#type: String,
    /// 응답 좌표계.
    #[arg(long, default_value = "EPSG:4326")]
    pub crs: String,
    /// 다건 입력 파일(줄당 1건) — 배치.
    #[arg(long)]
    pub input: Option<std::path::PathBuf>,
}

pub async fn run_geocode(g: &GlobalArgs, a: GeocodeArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let crs = a.crs.clone();
    let auto = a.r#type.eq_ignore_ascii_case("auto");

    // 배치(--input): 줄 단위 처리. auto는 ROAD 기준(역지오는 --reverse 명시).
    if let Some(path) = &a.input {
        let ty = if auto { "ROAD".to_string() } else { a.r#type.clone() };
        let reverse = a.reverse;
        let request = if reverse { "GetAddress" } else { "GetCoord" };
        let crs2 = crs.clone();
        let build = move |q: &str| -> (String, Vec<(String, String)>) {
            let mut b = QueryBuilder::new("address", request)
                .format("json")
                .set("type", &ty)
                .set("crs", &crs2);
            b = if reverse { b.set("point", q) } else { b.set("address", q) };
            b.build()
        };
        let lines = read_lines(path)?;
        return run_batch_lines(g, &client, &auths, lines, build).await;
    }

    let q = a.query.ok_or_else(|| anyhow!("주소 또는 좌표(\"x,y\")가 필요합니다"))?;
    // ③ 입력 자동 감지: "x,y"(숫자 두 개) 형태면 역지오코딩.
    let reverse = a.reverse || looks_like_coord(&q);
    let request = if reverse { "GetAddress" } else { "GetCoord" };
    // ①② 유형 자동 폴백: auto면 도로명→지번 순으로 시도.
    let types: Vec<&str> = if auto {
        vec!["ROAD", "PARCEL"]
    } else {
        vec![a.r#type.as_str()]
    };

    let auth = pick_auth(&auths);
    let mut last_body = String::new();
    for ty in &types {
        let mut b = QueryBuilder::new("address", request)
            .format("json")
            .set("type", ty)
            .set("crs", &crs);
        b = if reverse { b.set("point", &q) } else { b.set("address", &q) };
        let (url, params) = b.build();
        let body = client.get_text(&url, params, auth).await?;
        let v = normalize::parse_to_json(&body)?;
        if geocode_has_result(&v, reverse) {
            if g.raw {
                return output::print_raw_text(&body);
            }
            return output::print_json(
                g,
                &serde_json::json!({
                    "ok": true,
                    "mode": if reverse { "reverse" } else { "forward" },
                    "type": ty,
                    "result": v
                }),
            );
        }
        last_body = body;
    }
    // 전 유형 실패 — 결과 없음.
    if g.raw {
        return output::print_raw_text(&last_body);
    }
    output::print_json(
        g,
        &serde_json::json!({
            "ok": true,
            "result": Value::Null,
            "note": format!("결과 없음: {q} (도로명·지번 모두 미발견)")
        }),
    )
}

// ───────────────────────── geocoder (apis.vworld.kr) ─────────────────────────

const GEOCODER_HOST: &str = "https://apis.vworld.kr";

#[derive(Args, Debug)]
pub struct GeocoderArgs {
    /// 주소(지번/도로명) 또는 좌표 "x,y" — 입력 형식을 자동 감지해 변환.
    pub query: String,
    /// 응답 좌표계.
    #[arg(long, default_value = "epsg:4326")]
    pub epsg: String,
}

/// apis.vworld.kr Geocoder — 주소/좌표를 받아 **좌표·지번·도로명을 한 번에** 반환.
/// 주소면 지번→도로명 순으로 좌표화 후, 그 좌표로 지번·도로명을 모두 역변환.
pub async fn run_geocoder(g: &GlobalArgs, a: GeocoderArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let key = pick_auth(&auths).key.clone();
    let client = Client::new()?;

    // 좌표 확보: 입력이 "x,y"면 그대로, 아니면 주소→좌표(지번→도로명 순).
    let (x, y) = if looks_like_coord(&a.query) {
        let p: Vec<&str> = a.query.split(',').collect();
        (p[0].trim().to_string(), p[1].trim().to_string())
    } else {
        let mut found = None;
        for path in ["jibun2coord.do", "new2coord.do"] {
            let params = vec![
                ("q".into(), a.query.clone()),
                ("output".into(), "json".into()),
                ("epsg".into(), a.epsg.clone()),
            ];
            let body = client
                .get_text_apikey(&format!("{GEOCODER_HOST}/{path}"), params, &key)
                .await?;
            if body.contains("인증에 실패") {
                return Err(anyhow!("API 키 인증 실패 — 등록된 키/도메인을 확인하세요."));
            }
            if let Ok(v) = serde_json::from_str::<Value>(&body) {
                if let (Some(xx), Some(yy)) =
                    (v["EPSG_4326_X"].as_str(), v["EPSG_4326_Y"].as_str())
                {
                    found = Some((xx.to_string(), yy.to_string()));
                    break;
                }
            }
        }
        found.ok_or_else(|| anyhow!("주소를 좌표로 변환하지 못했습니다: {}", a.query))?
    };

    // 좌표 → 지번 + 도로명(둘 다 조회; 없으면 null).
    let jibun = geocoder_reverse(&client, &key, &a.epsg, &x, &y, "coord2jibun.do", "ADDR").await?;
    let road = geocoder_reverse(&client, &key, &a.epsg, &x, &y, "coord2new.do", "NEW_JUSO").await?;

    output::print_json(
        g,
        &serde_json::json!({
            "ok": true,
            "input": a.query,
            "point": {"x": x, "y": y},
            "jibun": jibun,
            "road": road,
        }),
    )
}

/// 좌표 → 주소 역변환 단건(coord2jibun/coord2new). 결과 없으면 None.
async fn geocoder_reverse(
    client: &Client,
    key: &str,
    epsg: &str,
    x: &str,
    y: &str,
    path: &str,
    field: &str,
) -> Result<Option<String>> {
    let params = vec![
        ("x".into(), x.to_string()),
        ("y".into(), y.to_string()),
        ("output".into(), "json".into()),
        ("epsg".into(), epsg.to_string()),
    ];
    let body = client
        .get_text_apikey(&format!("{GEOCODER_HOST}/{path}"), params, key)
        .await?;
    if let Ok(v) = serde_json::from_str::<Value>(&body) {
        if let Some(s) = v[field].as_str() {
            if !s.is_empty() {
                return Ok(Some(s.to_string()));
            }
        }
    }
    Ok(None)
}

/// "x,y"(숫자 두 개, 쉼표 구분) 형태인지 — 역지오코딩 자동 감지용.
fn looks_like_coord(q: &str) -> bool {
    let parts: Vec<&str> = q.split(',').collect();
    parts.len() == 2 && parts.iter().all(|p| p.trim().parse::<f64>().is_ok())
}

/// 지오/역지오 응답에 유효 결과가 있는지 — auto 폴백 성공 판정.
fn geocode_has_result(v: &Value, reverse: bool) -> bool {
    if v["response"]["status"].as_str() == Some("NOT_FOUND") {
        return false;
    }
    if reverse {
        let res = &v["response"]["result"];
        let text = if res.is_array() {
            res[0]["text"].as_str()
        } else {
            res["text"].as_str()
        };
        text.is_some()
    } else {
        v["response"]["result"]["point"]["x"].is_string()
    }
}

/// 주소 → 좌표 변환. GetCoord 호출 후 `response.result.point`에서 추출.
/// 도로명(ROAD) 우선 → 실패 시 지번(PARCEL)으로 자동 폴백.
/// crs: 응답 좌표계 (예: "EPSG:4326", "EPSG:5187"). 3D 분석 지도 중심 이동 등 내부 재사용용.
pub async fn geocode_point(g: &GlobalArgs, address: &str, crs: &str) -> Result<(String, String)> {
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let auth = pick_auth(&auths);
    // 도로명 → 지번 순으로 시도(둘 중 먼저 좌표가 잡히는 것을 사용).
    for ty in ["ROAD", "PARCEL"] {
        let (url, params) = QueryBuilder::new("address", "GetCoord")
            .format("json")
            .set("type", ty)
            .set("crs", crs)
            .set("address", address)
            .build();
        let body = client.get_text(&url, params, auth).await?;
        let v = normalize::parse_to_json(&body)?;
        let point = &v["response"]["result"]["point"];
        if let (Some(lon), Some(lat)) = (point["x"].as_str(), point["y"].as_str()) {
            return Ok((lon.to_string(), lat.to_string()));
        }
    }
    Err(anyhow!(
        "주소를 좌표로 변환하지 못했습니다(도로명·지번 모두 실패): {address}"
    ))
}

// ───────────────────────── search ─────────────────────────

#[derive(Args, Debug)]
pub struct SearchArgs {
    pub query: String,
    /// 검색 대상.
    #[arg(long, default_value = "PLACE")]
    pub r#type: String,
    #[arg(long)]
    pub category: Option<String>,
    /// 페이지 크기(1~1000).
    #[arg(long)]
    pub size: Option<u32>,
    #[arg(long)]
    pub page: Option<u32>,
    /// 검색 영역 bbox "minx,miny,maxx,maxy".
    #[arg(long)]
    pub bbox: Option<String>,
    #[arg(long, default_value = "EPSG:4326")]
    pub crs: String,
}

pub async fn run_search(g: &GlobalArgs, a: SearchArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let size = a.size.map(|s| s.to_string());
    let page = a.page.map(|p| p.to_string());
    let (url, params) = QueryBuilder::new("search", "search")
        .format("json")
        .set("query", &a.query)
        .set("type", &a.r#type)
        .set("crs", &a.crs)
        .opt("category", a.category.as_deref())
        .opt("size", size.as_deref())
        .opt("page", page.as_deref())
        .opt("bbox", a.bbox.as_deref())
        .build();
    fetch_one(g, &client, url, params, pick_auth(&auths)).await
}

// ───────────────────────── 2D data ─────────────────────────

/// `data layers` 서브명령 인자.
#[derive(Args, Debug)]
pub struct DataLayersArgs {
    /// 키워드 부분일치 필터(data_id·name·cat, 대소문자 무시).
    #[arg(long)]
    pub search: Option<String>,
    /// 카테고리 일치 필터.
    #[arg(long)]
    pub cat: Option<String>,
    /// geometry 타입 필터(polygon|line|point, 대소문자 무시·Multi 접두 무시).
    #[arg(long)]
    pub geom: Option<String>,
}

/// `data describe <data_id>` 서브명령 인자.
#[derive(Args, Debug)]
pub struct DataDescribeArgs {
    /// 데이터ID (예: LT_C_UQ111). 소문자 입력 허용.
    pub data_id: String,
}

/// `data` 서브커맨드 enum (layers / describe / fetch).
#[derive(clap::Subcommand, Debug)]
pub enum DataSub {
    /// 전체 158개 2D 레이어 목록(오프라인, 키 불요).
    Layers(DataLayersArgs),
    /// 레이어 상세 정보 — 속성표·단일검색키·샘플URL(오프라인, 키 불요).
    Describe(DataDescribeArgs),
    /// 2D데이터 GetFeature 조회 (/req/data). `data <id>` 위치인자와 동일.
    #[command(name = "fetch")]
    Fetch(DataArgs),
}

/// `data` 최상위 커맨드 — 서브커맨드(`layers`/`describe`/`fetch`)와
/// 기존 `data <데이터ID> [옵션]` 위치인자 조회가 **공존**한다.
///
/// clap 공존 패턴: `args_conflicts_with_subcommands = true` +
/// `flatten DataArgs`(기존 위치인자/옵션) + `subcommand Option<DataSub>`.
/// 서브커맨드가 없으면 flatten된 DataArgs로 기존 `run_data`를 호출한다.
#[derive(Args, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct DataTopArgs {
    /// 서브커맨드(layers / describe / fetch). 없으면 위치인자 조회 모드.
    #[command(subcommand)]
    pub sub: Option<DataSub>,

    /// 기존 `data <데이터ID> [옵션]` 위치인자 조회 인자.
    #[command(flatten)]
    pub fetch: DataArgs,
}

/// geom 입력 정규화: "MultiPolygon"→"Polygon", "multiline"→"Line" 등.
/// 입력 대소문자·"Multi" 접두를 제거한 뒤 Polygon/Line/Point 로 정규화.
fn normalize_geom_input(input: &str) -> Option<&'static str> {
    let s = input.to_lowercase();
    let s = s.strip_prefix("multi").unwrap_or(&s);
    if s.starts_with("polygon") || s == "poly" {
        Some("Polygon")
    } else if s.starts_with("line") || s == "linestring" {
        Some("Line")
    } else if s.starts_with("point") {
        Some("Point")
    } else {
        None
    }
}

/// `data layers` 실행.
pub fn run_data_layers(g: &GlobalArgs, a: DataLayersArgs) -> Result<()> {
    use crate::twod_registry;

    // geom 입력 정규화
    let geom_filter = if let Some(ref gv) = a.geom {
        match normalize_geom_input(gv) {
            Some(n) => Some(n),
            None => {
                return Err(anyhow::anyhow!(
                    "알 수 없는 geom 값: '{gv}' (polygon|line|point 중 하나)"
                ));
            }
        }
    } else {
        None
    };

    let layers: Vec<_> = twod_registry::all()
        .iter()
        .filter(|l| {
            // --search 필터
            if let Some(ref q) = a.search {
                let q_lc = q.to_lowercase();
                let matched = l.data_id.to_lowercase().contains(&q_lc)
                    || l.name.to_lowercase().contains(&q_lc)
                    || l.cat.to_lowercase().contains(&q_lc);
                if !matched {
                    return false;
                }
            }
            // --cat 필터
            if let Some(ref c) = a.cat {
                if !l.cat.eq_ignore_ascii_case(c) {
                    return false;
                }
            }
            // --geom 필터(정규화 후 비교)
            if let Some(gf) = geom_filter {
                if l.geom != gf {
                    return false;
                }
            }
            true
        })
        .collect();

    if g.raw {
        // raw: JSON 배열
        let arr: Vec<_> = layers
            .iter()
            .map(|l| {
                serde_json::json!({
                    "data_id": l.data_id,
                    "name": l.name,
                    "cat": l.cat,
                    "geom": l.geom,
                    "attr_count": l.attrs.len(),
                })
            })
            .collect();
        return output::print_json(g, &serde_json::json!({"ok": true, "count": arr.len(), "layers": arr}));
    }

    // 테이블 출력
    println!(
        "{:<30} {:<40} {:<20} {:<10} {}",
        "데이터ID", "레이어명", "카테고리", "geom", "속성수"
    );
    println!("{}", "-".repeat(110));
    for l in &layers {
        println!(
            "{:<30} {:<40} {:<20} {:<10} {}",
            l.data_id,
            l.name,
            l.cat,
            l.geom,
            l.attrs.len()
        );
    }
    println!();
    println!("총 {}건", layers.len());
    Ok(())
}

/// `data describe <data_id>` 실행.
pub fn run_data_describe(g: &GlobalArgs, a: DataDescribeArgs) -> Result<()> {
    use crate::twod_registry;

    let layer = twod_registry::find(&a.data_id).ok_or_else(|| {
        anyhow::anyhow!(
            "알 수 없는 데이터ID: '{}' (`vworld data layers`로 전체 목록 확인)",
            a.data_id
        )
    })?;

    let sample_url = format!(
        "https://api.vworld.kr/req/data?service=data&request=GetFeature&data={}&key=<KEY>&format=json&size=10",
        layer.data_id
    );

    if g.raw {
        let single_search_keys: Vec<&str> = layer
            .attrs
            .iter()
            .filter(|a| a.single_search)
            .map(|a| a.name)
            .collect();
        return output::print_json(
            g,
            &serde_json::json!({
                "ok": true,
                "data_id": layer.data_id,
                "svc_ide": layer.svc_ide,
                "name": layer.name,
                "cat": layer.cat,
                "geom": layer.geom,
                "endpoint": "/req/data GetFeature",
                "attrs": layer.attrs,
                "single_search_keys": single_search_keys,
                "sample_url": sample_url,
            }),
        );
    }

    // 텍스트 출력
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("데이터ID   : {}", layer.data_id);
    println!("svcIde     : {}", layer.svc_ide);
    println!("레이어명   : {}", layer.name);
    println!("카테고리   : {}", layer.cat);
    println!("geom       : {}", layer.geom);
    println!("엔드포인트 : /req/data GetFeature");
    println!();

    if layer.attrs.is_empty() {
        println!("[속성 미수집] 이 레이어의 속성 메타가 없습니다.");
    } else {
        println!(
            "{:<25} {:<8} {:<15} {}",
            "속성명", "단일검색", "타입", "설명"
        );
        println!("{}", "-".repeat(80));
        for attr in layer.attrs {
            println!(
                "{:<25} {:<8} {:<15} {}",
                attr.name,
                if attr.single_search { "Y" } else { "-" },
                attr.r#type,
                attr.desc
            );
        }
        println!();

        let single_keys: Vec<&str> = layer
            .attrs
            .iter()
            .filter(|a| a.single_search)
            .map(|a| a.name)
            .collect();
        if single_keys.is_empty() {
            println!("단일검색 가능 키: 없음");
        } else {
            println!("단일검색 가능 키: {}", single_keys.join(", "));
        }
    }

    println!();
    println!("샘플 요청 URL:");
    println!("  {sample_url}");
    println!();

    Ok(())
}

#[derive(Args, Debug)]
pub struct DataArgs {
    /// 데이터셋 ID(예: LP_PA_CBND_BUBUN).
    /// 서브커맨드(`layers`/`describe`) 사용 시에는 생략 가능.
    pub data: Option<String>,
    /// 공간 필터(POINT(x y) / POLYGON((...)) / BOX(...)).
    #[arg(long)]
    pub geom_filter: Option<String>,
    /// 속성 필터(속성:연산자:값|...).
    #[arg(long)]
    pub attr_filter: Option<String>,
    /// 읍면동코드(geom/attr 둘 다 없을 때 필수).
    #[arg(long)]
    pub emd_cd: Option<String>,
    #[arg(long)]
    pub columns: Option<String>,
    #[arg(long)]
    pub size: Option<u32>,
    #[arg(long)]
    pub page: Option<u32>,
    #[arg(long, default_value = "EPSG:4326")]
    pub crs: String,
}

pub async fn run_data(g: &GlobalArgs, a: DataArgs) -> Result<()> {
    let data_id = a.data.as_deref().ok_or_else(|| {
        anyhow!("데이터셋 ID가 필요합니다(예: vworld data LT_C_ADSIDO_INFO --attr-filter ...)")
    })?;
    if a.geom_filter.is_none() && a.attr_filter.is_none() && a.emd_cd.is_none() {
        return Err(anyhow!(
            "geom_filter/attr_filter 둘 다 없으면 --emd-cd(읍면동코드)가 필요합니다(설계 §1.3)"
        ));
    }
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let size = a.size.map(|s| s.to_string());
    let page = a.page.map(|p| p.to_string());
    let (url, params) = QueryBuilder::new("data", "GetFeature")
        .format("json")
        .set("data", data_id)
        .set("crs", &a.crs)
        .opt("geomFilter", a.geom_filter.as_deref())
        .opt("attrFilter", a.attr_filter.as_deref())
        .opt("emdCd", a.emd_cd.as_deref())
        .opt("columns", a.columns.as_deref())
        .opt("size", size.as_deref())
        .opt("page", page.as_deref())
        .build();
    fetch_one(g, &client, url, params, pick_auth(&auths)).await
}

// ───────────────────────── 행정동 경계 DB ─────────────────────────

#[derive(clap::Subcommand, Debug)]
pub enum HjdDbCmd {
    /// 행정동 경계 SHP(BND_ADM_DONG_PG.shp)를 SQLite로 적재.
    Build {
        /// 행정동 경계 SHP 경로.
        #[arg(long)]
        shp: std::path::PathBuf,
        /// 출력 SQLite DB 경로.
        #[arg(long)]
        db: std::path::PathBuf,
    },
    /// DB 요약(행정동 수) 출력.
    Info {
        #[arg(long)]
        db: std::path::PathBuf,
    },
    /// 센서스 지역코드 xlsx → `region_code` 테이블 적재(행정동 경계와 ADM_CD로 조인).
    Region {
        /// 지역코드 xlsx 경로.
        #[arg(long)]
        xlsx: std::path::PathBuf,
        #[arg(long)]
        db: std::path::PathBuf,
        /// 시트명(기본: SHP 기준일에 맞는 최신).
        #[arg(long, default_value = "2025년 6월")]
        sheet: String,
    },
    /// ADM_CD 또는 동명으로 경계+지역코드 조인 조회.
    Lookup {
        #[arg(long)]
        db: std::path::PathBuf,
        /// 행정동코드(8자리) 또는 동명 일부.
        query: String,
    },
}

pub async fn run_hjd_db(g: &GlobalArgs, cmd: HjdDbCmd) -> Result<()> {
    match cmd {
        HjdDbCmd::Build { shp, db } => {
            let n = crate::hjd_db::build_from_shp(&shp, &db)?;
            output::print_json(
                g,
                &serde_json::json!({"ok": true, "적재된_행정동": n, "db": db, "shp": shp}),
            )
        }
        HjdDbCmd::Info { db } => {
            // bbox 전국 범위로 전수 로드해 개수만 보고(간단).
            let all = crate::geomath::Bbox { minx: f64::MIN, miny: f64::MIN, maxx: f64::MAX, maxy: f64::MAX };
            let b = crate::hjd_db::load_for_bbox(&db, all)?;
            output::print_json(g, &serde_json::json!({"ok": true, "행정동수": b.len(), "db": db}))
        }
        HjdDbCmd::Region { xlsx, db, sheet } => {
            let n = crate::hjd_db::build_region_from_xlsx(&xlsx, &sheet, &db)?;
            output::print_json(
                g,
                &serde_json::json!({"ok": true, "적재된_지역코드": n, "시트": sheet, "db": db}),
            )
        }
        HjdDbCmd::Lookup { db, query } => {
            let rows = crate::hjd_db::lookup(&db, &query)?;
            output::print_json(g, &serde_json::json!({"ok": true, "건수": rows.len(), "결과": rows}))
        }
    }
}

// ───────────────────────── NED(국가중점) ─────────────────────────

#[derive(Args, Debug)]
pub struct NedArgs {
    /// 오퍼레이션 이름(예: getBuildingAge) 또는 `--list`로 목록.
    pub operation: Option<String>,
    /// 레지스트리 목록 출력.
    #[arg(long)]
    pub list: bool,
    /// 필지고유번호(또는 WFS `--all` 시 법정동 8자리 접두).
    #[arg(long)]
    pub pnu: Option<String>,
    /// bbox(WMS/WFS).
    #[arg(long)]
    pub bbox: Option<String>,
    /// WFS 전수 수집 — `--pnu <접두>` 범위를 PNU 접두 분할로 1000 cap 우회해 모두 수집.
    #[arg(long)]
    pub all: bool,
    /// PNU 목록 파일(줄당 1건) — 각 PNU에 오퍼레이션을 병렬 배치 실행(data 계열).
    #[arg(long)]
    pub input: Option<std::path::PathBuf>,
    /// **행정동별 집계**(WFS): 법정동 전수 수집 → 각 필지 역지오로 행정동 분류 → 수치필드 통계.
    /// `--pnu <법정동8자리>` 필요. WFS 계열 전용.
    #[arg(long = "by-hjd")]
    pub by_hjd: bool,
    /// `--by-hjd`에서 집계할 수치 property 필드명(기본 `pblntf_pclnd`=개별공시지가).
    #[arg(long, default_value = "pblntf_pclnd")]
    pub value_field: String,
    /// `--by-hjd` 격자 분류 정밀도(좌표 소수 자릿수). 기본 3(≈100m): 인접 필지를 격자로 묶어
    /// 격자당 1회만 역지오 → 호출 수십~수백 배 감소. 4면 ≈10m(정밀·느림), 6이면 사실상 필지별.
    #[arg(long, default_value_t = 3)]
    pub hjd_grid: u32,
    /// `--by-hjd`를 **행정동경계 SHP(EPSG:5186)** 기반 point-in-polygon으로 처리(역지오 불필요·즉시·정확).
    /// VWorld 센서스 `BND_ADM_DONG_PG.shp` 경로 지정.
    #[arg(long)]
    pub hjd_shp: Option<std::path::PathBuf>,
    /// `--by-hjd`를 **행정동경계 SQLite DB** 기반으로 처리(`hjd-db build`로 적재한 것). SHP보다 빠름.
    #[arg(long)]
    pub hjd_db: Option<std::path::PathBuf>,
    /// 임의 파라미터 패스스루(k=v, 반복). 예: `--param layers=레이어명`.
    /// 목록 보기는 `--params`(복수형) 사용.
    #[arg(long = "param")]
    pub params: Vec<String>,
    /// 해당 오퍼레이션의 요청변수 목록을 출력하고 종료.
    /// `--param`(값 전달)과 다름 — 이 플래그는 목록 조회 전용.
    #[arg(long = "params")]
    pub show_params: bool,
    /// WMS 이미지 너비(픽셀). WMS 계열 전용(data/wfs에서는 무시).
    #[arg(long, default_value_t = 512)]
    pub width: u32,
    /// WMS 이미지 높이(픽셀). WMS 계열 전용(data/wfs에서는 무시).
    #[arg(long, default_value_t = 512)]
    pub height: u32,
    /// WMS 이미지 포맷. WMS 계열 전용.
    #[arg(long = "img-format", default_value = "image/png")]
    pub img_format: String,
    /// WMS 투명 배경. WMS 계열 전용(data/wfs에서는 무시).
    #[arg(long)]
    pub transparent: bool,
    /// 좌표계(WMS/WFS 단건). 기본 EPSG:5187(동부원점 TM, 미터).
    /// 중부원점은 EPSG:5186, 서부원점은 EPSG:5185, 위경도는 EPSG:4326.
    /// `--by-hjd`는 내부 EPSG:4326 고정이라 이 값을 무시함.
    #[arg(long, default_value = "EPSG:5187")]
    pub crs: Option<String>,
    /// `--address` 수집 결과를 Shapefile로 저장(WFS 계열 전용).
    /// .shp 경로 지정 → .shp/.shx/.dbf/.prj/.cpg 5종 생성.
    #[arg(long)]
    pub shp: Option<std::path::PathBuf>,
    /// WMS 이미지 저장 경로. 미지정 시 `ned_<op>.png`로 저장.
    #[arg(long, short)]
    pub output: Option<std::path::PathBuf>,
    /// 주소 반경 bbox 격자수집 — 지오코딩 중심 주소(WFS 계열 전용).
    #[arg(long)]
    pub address: Option<String>,
    /// `--address` 반경(미터, 기본 1000m). 정사각 bbox 반폭.
    #[arg(long, default_value_t = 1000u32)]
    pub radius: u32,
    /// `--address` 수집 결과를 DXF 파일로 저장(WFS 계열 전용).
    #[arg(long)]
    pub dxf: Option<std::path::PathBuf>,
    /// DXF 텍스트 인코딩(기본 cp949).
    #[arg(long, default_value = "cp949")]
    pub encoding: String,
    /// DXF 심볼/문자 기준 도면 스케일 분모(기본 1000 = 1:1000).
    #[arg(long = "symbol-scale", default_value_t = 1000u32)]
    pub symbol_scale: u32,
}

/// bbox 문자열의 축 순서를 CRS에 따라 처리.
/// EPSG:4326/5185/5186/5187/5188은 WMS 1.3.0에서 (ymin,xmin,ymax,xmax) 순.
/// 그 외(EPSG:900913/3857 등)는 (minx,miny,maxx,maxy) 그대로.
fn ned_wms_bbox(bbox: &str, crs: &str) -> String {
    // 입력이 이미 쉼표로 4개 숫자인 경우만 처리.
    let parts: Vec<&str> = bbox.split(',').collect();
    if parts.len() != 4 {
        return bbox.to_string();
    }
    let crs_upper = crs.to_uppercase();
    let needs_swap = matches!(
        crs_upper.as_str(),
        "EPSG:4326" | "EPSG:5185" | "EPSG:5186" | "EPSG:5187" | "EPSG:5188"
    );
    if needs_swap {
        // 입력이 (ymin,xmin,ymax,xmax) 또는 (lat1,lon1,lat2,lon2)로 들어왔다고 가정 — 그대로 전달.
        // 실제 축반전: 사용자가 (minx,miny,maxx,maxy) 입력 시 → (miny,minx,maxy,maxx) 변환.
        // run_wms와 동일 패턴: bbox 값 자체를 그대로 전달(사용자가 CRS에 맞는 순서로 입력하도록 안내).
        bbox.to_string()
    } else {
        bbox.to_string()
    }
}

/// NED WMS/WFS/data 공통 필수 파라미터 검증.
/// op.params 중 required=true인 항목이 전용 플래그 또는 --param k=v로 충족되는지 확인.
///
/// 주의: 레지스트리 메타의 `default`(API 문서 예시값)는 충족 근거에서 **제외**.
/// CLI 기능 기본값(width=512, height=512, crs, format 등)은 호출자가 `provided_flags`에 포함해 전달.
fn validate_ned_required(
    op: &crate::ned_registry::NedOp,
    provided_flags: &[&str],         // CLI 전용 플래그 또는 CLI 기능 기본값으로 실제 전송되는 파라미터명 목록
    passthrough_keys: &[String],      // --param k=v에서 추출한 키 목록
) -> Result<()> {
    // key, domain은 인증계층이 처리하므로 검증 제외.
    const AUTH_KEYS: &[&str] = &["key", "domain", "apikey", "servicekey"];

    let mut missing: Vec<&str> = Vec::new();
    for param in op.params {
        if !param.required {
            continue;
        }
        let name_lower = param.name.to_lowercase();
        // 인증 파라미터는 제외.
        if AUTH_KEYS.iter().any(|k| *k == name_lower) {
            continue;
        }
        // 레지스트리 default(예시값)는 충족 근거에서 제외 — 사용자 제공 또는 CLI 기능 기본값만 인정.
        // 전용 플래그 또는 CLI 기능 기본값으로 실제 전송되면 충족.
        let by_flag = provided_flags
            .iter()
            .any(|f| f.to_lowercase() == name_lower);
        // --param k=v로 제공됐으면 충족.
        let by_passthrough = passthrough_keys
            .iter()
            .any(|k| k.to_lowercase() == name_lower);
        if !by_flag && !by_passthrough {
            missing.push(param.name);
        }
    }
    if !missing.is_empty() {
        let list = missing.join(", ");
        return Err(anyhow!(
            "필수 요청변수가 누락되었습니다: {list}\n\
             `vworld ned {} --params`로 요청변수 목록을 확인하세요.",
            op.endpoint_op
        ));
    }
    Ok(())
}

pub async fn run_ned(g: &GlobalArgs, a: NedArgs) -> Result<()> {
    if a.list || a.operation.is_none() {
        let ops = ned_registry::all();
        return output::print_json(g, &serde_json::json!({"ok": true, "count": ops.len(), "operations": ops}));
    }
    let op_name = a.operation.clone().unwrap();
    let op = ned_registry::find(&op_name)
        .ok_or_else(|| anyhow!("알 수 없는 NED 오퍼레이션: {op_name} (`vworld ned --list` 참고)"))?;

    // ── --params: 요청변수 표 출력 후 종료 ──
    if a.show_params {
        if op.params.is_empty() {
            eprintln!("[안내] '{op_name}'의 요청변수 메타가 없습니다(--param k=v로 직접 전달 가능).");
            return output::print_json(
                g,
                &serde_json::json!({"ok": true, "operation": op_name, "params": []}),
            );
        }
        // 표 형식 출력
        println!("오퍼레이션: {op_name} ({})", op.name);
        println!("{:<30} {:<8} {:<12} {:<20} {}", "이름", "필수", "타입", "기본값", "설명");
        println!("{}", "-".repeat(90));
        for p in op.params {
            println!(
                "{:<30} {:<8} {:<12} {:<20} {}",
                p.name,
                if p.required { "필수" } else { "옵션" },
                p.r#type,
                if p.default.is_empty() { "-" } else { p.default },
                p.desc
            );
        }
        println!();
        println!("* --param <이름>=<값> 으로 전달  예: --param pnu=1111017700102110000");
        return Ok(());
    }

    let auths = load_auths(g)?;
    let client = Client::new()?;

    let kind = match op.kind {
        "wms" => NedKind::Wms,
        "wfs" => NedKind::Wfs,
        _ => NedKind::Data,
    };

    // ── WMS 플래그를 data/wfs에서 지정 시 경고 ──
    if kind != NedKind::Wms {
        if a.transparent {
            eprintln!("[경고] --transparent는 WMS 계열 전용입니다(현재 {op_name}은 {} 계열 — 무시됨).", op.kind);
        }
        // width/height/img-format은 기본값이 있으므로 명시 여부를 직접 알 수 없음 — 무시(clap 제한).
    }

    // ── --crs + --by-hjd 충돌 경고 ──
    if a.by_hjd && a.crs.is_some() {
        eprintln!("[경고] --by-hjd는 내부적으로 EPSG:4326을 고정 사용합니다. --crs 값은 무시됩니다.");
    }

    // ── WMS GetMap 처리 ──
    if kind == NedKind::Wms {
        let bbox = a.bbox.as_deref().ok_or_else(|| {
            anyhow!("NED WMS GetMap은 --bbox가 필요합니다(예: --bbox 37.49,126.99,37.54,127.05).")
        })?;
        let crs = a.crs.as_deref().unwrap_or("EPSG:900913");

        // --param k=v 키 집합(우선순위 판단용).
        let passthrough_pairs = parse_passthrough(&a.params)?;
        let passthrough_keys: Vec<String> = passthrough_pairs.iter().map(|(k, _)| k.clone()).collect();

        // 필수 파라미터 검증.
        // WMS 전용플래그(crs/bbox/width/height/format)와 레지스트리 default(layers 등)로 충족되므로
        // validate_ned_required는 bbox·layers 외 실제 누락된 항목만 잡는다.
        let mut provided: Vec<&str> = vec!["crs", "bbox", "width", "height", "format"];
        if a.pnu.is_some() { provided.push("pnu"); }
        validate_ned_required(op, &provided, &passthrough_keys)?;

        let processed_bbox = ned_wms_bbox(bbox, crs);
        let width_str = a.width.to_string();
        let height_str = a.height.to_string();

        // 전용플래그로 덮어쓸 파라미터 이름 집합(소문자).
        let flag_keys: std::collections::HashSet<&str> =
            ["crs", "bbox", "width", "height", "format", "transparent"].iter().copied().collect();

        let mut b = NedBuilder::new(NedKind::Wms, op.endpoint_op)
            .set("crs", crs)
            .set("bbox", &processed_bbox)
            .set("width", &width_str)
            .set("height", &height_str)
            .set("format", &a.img_format);

        // 레지스트리 WMS 파라미터 default 자동 적용.
        // 우선순위: 전용플래그 > --param k=v > 레지스트리 default.
        // bbox는 자동 적용 제외(레지스트리 default는 소규모 샘플 영역이라 사용자 의도와 다름).
        for param in op.params {
            let name_lc = param.name.to_lowercase();
            // bbox·인증키는 건너뜀.
            if name_lc == "bbox" || name_lc == "key" || name_lc == "domain" {
                continue;
            }
            // 레지스트리 default가 없으면 건너뜀.
            if param.default.is_empty() {
                continue;
            }
            // 전용플래그로 이미 적용된 항목은 건너뜀.
            if flag_keys.contains(name_lc.as_str()) {
                continue;
            }
            // --param k=v로 사용자가 직접 지정한 항목은 건너뜀(아래에서 적용).
            if passthrough_keys.iter().any(|k| k.to_lowercase() == name_lc) {
                continue;
            }
            // 레지스트리 default를 자동 적용.
            b = b.set(param.name, param.default);
        }

        if a.transparent {
            b = b.set("transparent", "TRUE");
        }
        // --param k=v: 사용자 지정값(레지스트리 default 덮어씀).
        for (k, v) in &passthrough_pairs {
            b = b.set(k, v);
        }
        let (url, params) = b.build();

        let bytes = client.get_bytes(&url, params, pick_auth(&auths)).await?;

        // 빈 이미지 경고(디코딩 불요 — 바이트 길이 휴리스틱)
        let empty_image = bytes.len() < 10240;
        if empty_image {
            eprintln!(
                "[경고] 응답 이미지에 데이터가 없습니다(빈 이미지 추정 — 인증키의 NED WMS 렌더링 권한 또는 layers 파라미터 확인 필요). \
                 동일 데이터는 WFS/속성(data) 계열로 조회 가능."
            );
        }

        let out_path = a.output.unwrap_or_else(|| {
            std::path::PathBuf::from(format!("ned_{}.png", op.endpoint_op))
        });
        let saved = output::save_bytes(&out_path, &bytes)?;
        return output::print_json(
            g,
            &serde_json::json!({
                "ok": true,
                "saved": saved,
                "empty_image": empty_image,
            }),
        );
    }

    // ── 배치: PNU 목록 파일을 병렬 실행 ──
    if let Some(path) = &a.input {
        let pnus = read_lines(path)?;
        return run_ned_batch(g, &client, &auths, op.kind, op.endpoint_op, pnus).await;
    }

    // ── 행정동별 집계: WFS 전수 → 역지오 분류 → 통계 ──
    if a.by_hjd {
        if kind != NedKind::Wfs {
            return Err(anyhow!("--by-hjd는 WFS 계열에서만 지원합니다(현재 {})", op.kind));
        }
        let prefix = a
            .pnu
            .clone()
            .ok_or_else(|| anyhow!("--by-hjd는 --pnu <법정동8자리>가 필요합니다(예: 26500101)"))?;
        return run_ned_by_hjd(
            g,
            &client,
            &auths,
            op.endpoint_op,
            &prefix,
            &a.value_field,
            a.hjd_grid,
            a.hjd_shp.as_deref(),
            a.hjd_db.as_deref(),
        )
        .await;
    }

    // ── 전수 수집: WFS PNU 접두 분할 ──
    if a.all {
        if kind != NedKind::Wfs {
            return Err(anyhow!("--all은 WFS 계열에서만 지원합니다(현재 {})", op.kind));
        }
        let prefix = a
            .pnu
            .clone()
            .ok_or_else(|| anyhow!("--all은 --pnu <법정동 접두>가 필요합니다(예: 31140104)"))?;
        let extra = parse_passthrough(&a.params)?;
        let conc = g.concurrency.unwrap_or_else(|| auths.len().max(2));
        let feats = harvest_wfs_all(&client, op.endpoint_op, &prefix, &auths, conc, &extra).await?;
        return output::print_json(
            g,
            &serde_json::json!({"ok": true, "type": "FeatureCollection", "count": feats.len(), "features": feats}),
        );
    }

    // ── 주소 반경 bbox 격자수집 ──
    if let Some(ref address) = a.address {
        if kind != NedKind::Wfs {
            return Err(anyhow!(
                "--address는 WFS 계열에서만 지원합니다(현재 {})",
                op.kind
            ));
        }
        // --crs 기본값: EPSG:5187 (동부원점 TM, 미터). 투영계/4326 모두 지원.
        let crs = a.crs.as_deref().unwrap_or("EPSG:5187");
        let (x_str, y_str) = geocode_point(g, address, crs).await?;
        let x: f64 = x_str
            .parse()
            .map_err(|_| anyhow!("지오코딩 결과 x 파싱 실패: {x_str}"))?;
        let y: f64 = y_str
            .parse()
            .map_err(|_| anyhow!("지오코딩 결과 y 파싱 실패: {y_str}"))?;
        let radius_m = a.radius as f64;
        let bbox5 = bbox_from_center_radius(x, y, radius_m, crs);
        eprintln!(
            "[주소→bbox] {address} → x={x:.6}, y={y:.6}, radius={radius_m}m, bbox={bbox5}, crs={crs}"
        );

        let conc = g.concurrency.unwrap_or_else(|| auths.len().max(2));
        let (feats, total_cells, max_depth) =
            harvest_wfs_bbox(&client, op.endpoint_op, &bbox5, crs, &auths, conc).await?;

        let collected = feats.len();

        if collected == 0 {
            return Err(anyhow!(
                "수집된 필지가 없습니다. 주소·반경을 확인하세요(address={address}, radius={radius_m}m)"
            ));
        }

        eprintln!(
            "[격자수집 완료] 격자={total_cells}칸, 최대깊이={max_depth}, 수집={collected}건"
        );

        let fc = serde_json::json!({
            "type": "FeatureCollection",
            "features": feats,
        });

        // --dxf / --shp 중 하나라도 지정되면 파일 출력 모드
        let has_file_output = a.dxf.is_some() || a.shp.is_some();

        // --dxf 지정 시 DXF 저장
        if let Some(ref dxf_path) = a.dxf {
            // 4326은 degree 좌표(텍스트 높이 degree 환산), 그 외 투영계는 미터 그대로.
            let is_degree = crs.to_uppercase() == "EPSG:4326";
            let dxf_bytes = crate::dxf::feature_collection_to_dxf(
                &fc,
                &crate::dxf::DxfOpts {
                    encoding: a.encoding.clone(),
                    symbol_scale: a.symbol_scale,
                    label_field: "pnu".into(),
                    is_degree,
                },
            )?;
            std::fs::write(dxf_path, &dxf_bytes)?;
            eprintln!("[DXF 저장] {}", dxf_path.display());
        }

        // --shp 지정 시 Shapefile 저장
        if let Some(ref shp_path) = a.shp {
            let shp_count = crate::shp::feature_collection_to_shp(&fc, shp_path, crs)?;
            eprintln!("[SHP 저장] {} ({shp_count}건)", shp_path.display());
        }

        if has_file_output {
            let mut meta = serde_json::json!({
                "ok": true,
                "center": {"x": x, "y": y},
                "radius_m": radius_m,
                "bbox": bbox5,
                "cells": total_cells,
                "max_depth": max_depth,
                "collected": collected,
                "dedup_count": collected,
                "crs": crs,
            });
            if let Some(ref dxf_path) = a.dxf {
                meta["dxf"] = serde_json::json!(dxf_path);
                meta["encoding"] = serde_json::json!(a.encoding);
            }
            if let Some(ref shp_path) = a.shp {
                meta["shp"] = serde_json::json!(shp_path);
            }
            return output::print_json(g, &meta);
        }

        return output::print_json(
            g,
            &serde_json::json!({
                "ok": true,
                "type": "FeatureCollection",
                "center": {"x": x, "y": y},
                "radius_m": radius_m,
                "bbox": bbox5,
                "cells": total_cells,
                "max_depth": max_depth,
                "collected": collected,
                "dedup_count": collected,
                "crs": crs,
                "features": feats,
            }),
        );
    }

    // ── 단건: data / wfs ──
    // 필수 파라미터 검증
    {
        let mut provided: Vec<&str> = Vec::new();
        if a.pnu.is_some() { provided.push("pnu"); }
        if a.bbox.is_some() { provided.push("bbox"); }
        if kind == NedKind::Wfs {
            provided.push("output"); // WFS는 output=application/json 고정
        }
        let passthrough_keys: Vec<String> = parse_passthrough(&a.params)?
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        validate_ned_required(op, &provided, &passthrough_keys)?;
    }

    let mut b = NedBuilder::new(kind, op.endpoint_op);
    b = b.opt("pnu", a.pnu.as_deref()).opt("bbox", a.bbox.as_deref());
    if let Some(crs) = a.crs.as_deref() {
        b = b.set("crs", crs);
    }
    for (k, v) in parse_passthrough(&a.params)? {
        b = b.set(&k, &v);
    }
    let (url, params) = b.build();
    fetch_one(g, &client, url, params, pick_auth(&auths)).await
}

/// PNU 목록을 병렬 배치 — 각 PNU에 NED 오퍼레이션 실행, index 순서 보존 출력(§5.1).
async fn run_ned_batch(
    g: &GlobalArgs,
    client: &Client,
    auths: &[Auth],
    kind_str: &str,
    endpoint_op: &str,
    pnus: Vec<String>,
) -> Result<()> {
    let concurrency = g.concurrency.unwrap_or_else(|| auths.len().max(2));
    let kind = if kind_str == "wfs" { NedKind::Wfs } else { NedKind::Data };
    let endpoint_op = endpoint_op.to_string();
    let client2 = client.clone();
    let raw = g.raw;

    let jobs: Vec<(usize, String)> = pnus.into_iter().enumerate().collect();
    let results = crate::concurrency::run_jobs(
        jobs,
        auths.to_vec(),
        concurrency,
        move |(idx, pnu), auth| {
            let client = client2.clone();
            let endpoint_op = endpoint_op.clone();
            async move {
                let (url, params) = NedBuilder::new(kind, &endpoint_op).set("pnu", &pnu).build();
                let body = client.get_text(&url, params, &auth).await?;
                let value = if raw {
                    Value::String(body)
                } else {
                    let v = normalize::parse_to_json(&body)?;
                    match normalize::check_body_error(&v) {
                        Ok(()) => v,
                        Err(e) if e.empty_ok => Value::Null,
                        Err(e) => return Err(anyhow!("{e}")),
                    }
                };
                Ok::<(usize, String, Value), anyhow::Error>((idx, pnu, value))
            }
        },
    )
    .await;

    let items: Vec<Value> = results
        .into_iter()
        .enumerate()
        .map(|(i, r)| match r {
            Ok((idx, pnu, v)) => serde_json::json!({"index": idx, "pnu": pnu, "result": v}),
            Err(e) => serde_json::json!({"index": i, "error": e.to_string()}),
        })
        .collect();
    output::print_json(g, &serde_json::json!({"ok": true, "count": items.len(), "items": items}))
}

/// 행정동별 집계 — WFS 전수 → 각 필지 역지오로 행정동 분류 → 수치필드 통계.
///
/// 파이프라인을 단일 명령으로 내장: ① WFS 접두분할 전수(srsName=EPSG:4326),
/// ② 각 필지 대표점 역지오(`GetAddress` level4A/level4AC), ③ 429 실패분 자동 재처리(동시성 하향),
/// ④ 행정동별 평균/중앙값/사분위/최저/최고 집계.
#[allow(clippy::too_many_arguments)]
async fn run_ned_by_hjd(
    g: &GlobalArgs,
    client: &Client,
    auths: &[Auth],
    endpoint_op: &str,
    prefix: &str,
    value_field: &str,
    grid_decimals: u32,
    shp_path: Option<&std::path::Path>,
    db_path: Option<&std::path::Path>,
) -> Result<()> {
    // ① 법정동 전수 수집(lon/lat) — 키풀 병렬 harvest.
    let harvest_conc = g.concurrency.unwrap_or_else(|| auths.len().max(2));
    let feats = harvest_wfs_all(
        client,
        endpoint_op,
        prefix,
        auths,
        harvest_conc,
        &[("srsName".to_string(), "EPSG:4326".to_string())],
    )
    .await?;

    // 필지별 (lon, lat, value) 추출(값 없으면 분류 제외 대상).
    let mut points: Vec<(f64, f64, f64)> = Vec::new();
    let mut no_value = 0usize;
    for f in &feats {
        let v = f
            .get("properties")
            .and_then(|p| p.get(value_field))
            .and_then(num_of);
        let ll = f.get("geometry").and_then(centroid_lonlat);
        match (ll, v) {
            (Some((lon, lat)), Some(val)) => points.push((lon, lat, val)),
            _ => no_value += 1,
        }
    }
    let total = points.len();

    // ★ SHP/DB 모드: 행정동경계 폴리곤으로 point-in-polygon 분류(역지오 불필요).
    if shp_path.is_some() || db_path.is_some() {
        return classify_by_polygon(g, shp_path, db_path, &points, feats.len(), value_field, prefix, no_value);
    }

    // ② 격자 묶기 — 인접 필지는 같은 행정동이므로 좌표를 격자로 양자화. 격자당 **후보 대표점 여러 개**
    //    보관(대표가 도로 필지라 행정동이 안 나오면 다음 후보로 재시도해 미분류 격자 누락 방지).
    use std::collections::BTreeMap;
    const MAX_CAND: usize = 5;
    let gd = grid_decimals as usize;
    let cell_key = |lon: f64, lat: f64| format!("{:.*},{:.*}", gd, lon, gd, lat);
    let mut cell_cands: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    let parcel_cell: Vec<String> = points
        .iter()
        .map(|&(lon, lat, _)| {
            let k = cell_key(lon, lat);
            let e = cell_cands.entry(k.clone()).or_default();
            if e.len() < MAX_CAND {
                e.push((lon, lat));
            }
            k
        })
        .collect();
    let cells: Vec<(String, Vec<(f64, f64)>)> = cell_cands.into_iter().collect();
    let n_cells = cells.len();
    let cell_idx: BTreeMap<&str, usize> =
        cells.iter().enumerate().map(|(i, (k, _))| (k.as_str(), i)).collect();

    // ③ 격자 분류 — 후보를 순차 시도(행정동 못 받으면 다음 후보), 네트워크 에러는 동시성 하향 재시도.
    let concurrency = g.concurrency.unwrap_or(3);
    let mut cell_hjd: Vec<Option<(String, String)>> = vec![None; n_cells];
    let mut cand_idx = vec![0usize; n_cells]; // 현재 시도 중인 후보 인덱스
    let mut resolved = vec![false; n_cells]; // 행정동 확정
    let mut exhausted = vec![false; n_cells]; // 모든 후보가 행정동 없음 → 비대상(도로 등) 확정
    let mut round = 0u32;
    loop {
        let pend: Vec<usize> = (0..n_cells)
            .filter(|&i| !resolved[i] && !exhausted[i] && cand_idx[i] < cells[i].1.len())
            .collect();
        if pend.is_empty() || round >= 12 {
            break;
        }
        let conc = if round == 0 { concurrency } else { 2 };
        let jobs: Vec<(usize, (f64, f64))> = pend.iter().map(|&i| (i, cells[i].1[cand_idx[i]])).collect();
        let client2 = client.clone();
        let results = crate::concurrency::run_jobs(jobs, auths.to_vec(), conc, move |(i, (lon, lat)), auth| {
            let client = client2.clone();
            async move {
                let (url, params) = QueryBuilder::new("address", "GetAddress")
                    .format("json")
                    .set("type", "BOTH")
                    .set("crs", "EPSG:4326")
                    .set("point", &format!("{lon},{lat}"))
                    .build();
                let body = client.get_text(&url, params, &auth).await?;
                let v = normalize::parse_to_json(&body)?;
                Ok::<(usize, Option<(String, String)>), anyhow::Error>((i, extract_hjd(&v)))
            }
        })
        .await;
        // Ok 결과만 처리(네트워크 에러는 cand_idx 유지 → 다음 라운드 재시도).
        for (i, h) in results.into_iter().flatten() {
            match h {
                Some(hjd) => {
                    cell_hjd[i] = Some(hjd);
                    resolved[i] = true;
                }
                None => {
                    cand_idx[i] += 1; // 이 후보는 행정동 없음 → 다음 후보로.
                    if cand_idx[i] >= cells[i].1.len() {
                        exhausted[i] = true; // 모든 후보 소진 → 진짜 비대상.
                    }
                }
            }
        }
        round += 1;
    }

    // ④ 필지를 자기 격자의 행정동으로 집계.
    let mut groups: BTreeMap<(String, String), Vec<f64>> = BTreeMap::new();
    let mut no_hjd = 0usize; // 비대상(도로·하천 등): 모든 후보가 행정동 없음.
    let unresolved_cells = (0..n_cells).filter(|&i| !resolved[i] && !exhausted[i]).count();
    let mut unresolved = 0usize;
    for (pi, k) in parcel_cell.iter().enumerate() {
        let ci = cell_idx[k.as_str()];
        match &cell_hjd[ci] {
            Some((a, c)) => groups.entry((a.clone(), c.clone())).or_default().push(points[pi].2),
            None if exhausted[ci] => no_hjd += 1,
            None => unresolved += 1,
        }
    }

    let mut out: Vec<Value> = groups
        .into_iter()
        .map(|((a, c), v)| serde_json::json!({
            "hjd": a, "hjdCode": c, "count": v.len(), "stats": stats_of(&v)
        }))
        .collect();
    out.sort_by(|x, y| {
        x["hjdCode"].as_str().unwrap_or("").cmp(y["hjdCode"].as_str().unwrap_or(""))
    });

    let classified: usize = out.iter().map(|g| g["count"].as_u64().unwrap_or(0) as usize).sum();
    output::print_json(
        g,
        &serde_json::json!({
            "ok": true,
            "법정동접두": prefix,
            "필드": value_field,
            "전체필지": feats.len(),
            "집계대상": total,
            "격자수_역지오호출": n_cells,
            "격자정밀도_소수자릿수": grid_decimals,
            "분류됨": classified,
            "비대상_도로하천등": no_hjd + no_value,
            "미해결에러": unresolved,
            "미해결격자": unresolved_cells,
            "커버리지": format!("{:.1}%", 100.0 * (classified + no_hjd + no_value) as f64 / feats.len().max(1) as f64),
            "행정동별": out,
        }),
    )
}

/// 행정동경계(SHP 또는 DB)로 필지점을 point-in-polygon 분류 → ADM_NM별 통계(역지오 0회).
#[allow(clippy::too_many_arguments)]
fn classify_by_polygon(
    g: &GlobalArgs,
    shp: Option<&std::path::Path>,
    db: Option<&std::path::Path>,
    points: &[(f64, f64, f64)],
    total_feats: usize,
    value_field: &str,
    prefix: &str,
    no_value: usize,
) -> Result<()> {
    use std::collections::BTreeMap;
    // 필지 bbox(EPSG:5186) + 여유 200m로 관련 행정동 폴리곤만 로드(DB 우선).
    let lls: Vec<(f64, f64)> = points.iter().map(|&(lon, lat, _)| (lon, lat)).collect();
    let target = crate::hjd_shp::points_bbox_5186(&lls, 200.0);
    let (bounds, src) = match (db, shp) {
        (Some(db), _) => (crate::hjd_db::load_for_bbox(db, target)?, "SQLite DB"),
        (None, Some(shp)) => (crate::hjd_shp::HjdBoundaries::load_for_bbox(shp, target)?, "SHP"),
        (None, None) => unreachable!(),
    };

    let mut groups: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut outside = 0usize; // 어느 폴리곤에도 안 들어감(경계 밖·해안 등).
    for &(lon, lat, val) in points {
        match bounds.classify_lonlat(lon, lat) {
            Some(name) => groups.entry(name.to_string()).or_default().push(val),
            None => outside += 1,
        }
    }

    let mut out: Vec<Value> = groups
        .into_iter()
        .map(|(name, v)| serde_json::json!({"hjd": name, "count": v.len(), "stats": stats_of(&v)}))
        .collect();
    out.sort_by(|a, b| {
        b["stats"]["mean"].as_i64().unwrap_or(0).cmp(&a["stats"]["mean"].as_i64().unwrap_or(0))
    });
    let classified: usize = out.iter().map(|x| x["count"].as_u64().unwrap_or(0) as usize).sum();

    output::print_json(
        g,
        &serde_json::json!({
            "ok": true,
            "법정동접두": prefix,
            "필드": value_field,
            "방식": format!("{src} point-in-polygon"),
            "전체필지": total_feats,
            "집계대상": points.len(),
            "사용행정동폴리곤": bounds.len(),
            "분류됨": classified,
            "경계밖": outside,
            "값없음": no_value,
            "커버리지": format!("{:.1}%", 100.0 * (classified + no_value) as f64 / total_feats.max(1) as f64),
            "행정동별": out,
        }),
    )
}

/// 역지오 응답에서 행정동(level4A)/행정동코드(level4AC) 추출.
fn extract_hjd(v: &Value) -> Option<(String, String)> {
    let arr = v.get("response")?.get("result")?.as_array()?;
    for item in arr {
        if let Some(s) = item.get("structure") {
            let code = s.get("level4AC").and_then(|x| x.as_str()).unwrap_or("");
            if !code.is_empty() {
                let name = s.get("level4A").and_then(|x| x.as_str()).unwrap_or("").to_string();
                return Some((name, code.to_string()));
            }
        }
    }
    None
}

/// geometry에서 대표점 = **외곽 링 정점들의 평균(centroid)**.
///
/// 첫 꼭짓점은 도로·경계에 걸려 역지오에 행정동이 안 나오는 경우가 많다.
/// 작은 필지의 정점 평균은 거의 내부에 위치해 오분류(미분류)를 크게 줄인다.
fn centroid_lonlat(geom: &Value) -> Option<(f64, f64)> {
    fn as_pair(v: &Value) -> Option<(f64, f64)> {
        let a = v.as_array()?;
        if a.len() >= 2 && a[0].is_number() && a[1].is_number() {
            Some((a[0].as_f64()?, a[1].as_f64()?))
        } else {
            None
        }
    }
    // 좌표쌍의 배열(링)을 처음 발견하면 반환.
    fn find_ring(v: &Value) -> Option<Vec<(f64, f64)>> {
        let arr = v.as_array()?;
        if !arr.is_empty() && arr.iter().all(|e| as_pair(e).is_some()) {
            return Some(arr.iter().filter_map(as_pair).collect());
        }
        for child in arr {
            if let Some(r) = find_ring(child) {
                return Some(r);
            }
        }
        None
    }
    let ring = find_ring(geom.get("coordinates")?)?;
    if ring.is_empty() {
        return None;
    }
    // 닫힌 링은 마지막=첫 점 중복 → centroid 계산에서 제외.
    let pts: &[(f64, f64)] = if ring.len() > 1 && ring.first() == ring.last() {
        &ring[..ring.len() - 1]
    } else {
        &ring[..]
    };
    let m = pts.len().max(1) as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |(ax, ay), &(x, y)| (ax + x, ay + y));
    Some((sx / m, sy / m))
}

/// 문자열/숫자 Value를 f64로.
fn num_of(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// 수치 벡터의 통계(count/mean/median/q1/q3/min/max).
fn stats_of(values: &[f64]) -> Value {
    if values.is_empty() {
        return serde_json::json!({});
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    let mean = v.iter().sum::<f64>() / n as f64;
    let pct = |p: f64| -> f64 {
        if n == 1 {
            return v[0];
        }
        let idx = p * (n - 1) as f64;
        let lo = idx.floor() as usize;
        let hi = idx.ceil() as usize;
        v[lo] + (v[hi] - v[lo]) * (idx - lo as f64)
    };
    serde_json::json!({
        "mean": mean.round() as i64,
        "median": pct(0.5).round() as i64,
        "q1": pct(0.25).round() as i64,
        "q3": pct(0.75).round() as i64,
        "min": v[0] as i64,
        "max": v[n - 1] as i64,
    })
}

/// 견고 GET — 클라이언트 내부 재시도(3) 위에 추가 라운드로 서버 502/연결끊김을 흡수.
/// 대량 harvest(수십~수백 호출) 중 단일 일시 실패가 전체를 abort하지 않도록 한다.
async fn resilient_get_text(
    client: &Client,
    url: &str,
    params: Vec<(String, String)>,
    auth: &Auth,
) -> Result<String> {
    let mut last: Option<anyhow::Error> = None;
    for attempt in 0..6u32 {
        match client.get_text(url, params.clone(), auth).await {
            Ok(b) => return Ok(b),
            Err(e) => {
                last = Some(e);
                let ms = 400u64 * (attempt as u64 + 1);
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("harvest GET 반복 실패")))
}

/// bbox 5토큰 문자열을 WFS에 조회해 `totalFeatures` 반환.
/// maxFeatures=1 카운트 프로브 — 실제 건수는 cap과 무관하게 totalFeatures에 노출됨(스파이크 확인).
async fn wfs_count_bbox(
    client: &Client,
    endpoint_op: &str,
    bbox5: &str,
    crs: &str,
    auth: &Auth,
) -> Result<u64> {
    let (url, mut params) = NedBuilder::new(NedKind::Wfs, endpoint_op).build();
    params.push(("bbox".into(), bbox5.to_string()));
    params.push(("srsName".into(), crs.to_string()));
    params.push(("maxFeatures".into(), "1".into()));
    let body = resilient_get_text(client, &url, params, auth).await?;
    let v = normalize::parse_to_json(&body)?;
    Ok(v.get("totalFeatures").and_then(|x| x.as_u64()).unwrap_or(0))
}

/// 중심(x, y) + 반경(미터) + crs → WFS bbox 5토큰 문자열.
///
/// - EPSG:4326: (lat,lon) 축순서 degree 변환 → latmin,lonmin,latmax,lonmax,EPSG:4326
/// - 투영계(5186/5187 등): (E,N) 미터 그대로 → Emin,Nmin,Emax,Nmax,<CRS>
fn bbox_from_center_radius(x: f64, y: f64, radius_m: f64, crs: &str) -> String {
    if crs.to_uppercase() == "EPSG:4326" {
        // 4326: x=lon, y=lat. WFS 축순서: lat,lon
        let lat = y;
        let lon = x;
        let delta_lat = radius_m / 111_320.0;
        let delta_lon = radius_m / (111_320.0 * lat.to_radians().cos());
        let lat_min = lat - delta_lat;
        let lon_min = lon - delta_lon;
        let lat_max = lat + delta_lat;
        let lon_max = lon + delta_lon;
        format!("{lat_min},{lon_min},{lat_max},{lon_max},EPSG:4326")
    } else {
        // 투영계: x=E, y=N. WFS 축순서: E,N (미터 그대로)
        let e = x;
        let n = y;
        format!("{},{},{},{},{}", e - radius_m, n - radius_m, e + radius_m, n + radius_m, crs)
    }
}

/// bbox 5토큰을 내부 배열로 파싱. EPSG:4326이면 [lat_min,lon_min,lat_max,lon_max],
/// 투영계면 [Emin,Nmin,Emax,Nmax].
fn parse_bbox5(bbox5: &str) -> Option<[f64; 4]> {
    let parts: Vec<&str> = bbox5.splitn(5, ',').collect();
    if parts.len() < 4 {
        return None;
    }
    let v0 = parts[0].trim().parse::<f64>().ok()?;
    let v1 = parts[1].trim().parse::<f64>().ok()?;
    let v2 = parts[2].trim().parse::<f64>().ok()?;
    let v3 = parts[3].trim().parse::<f64>().ok()?;
    Some([v0, v1, v2, v3])
}

/// bbox 내부 배열 → WFS 5토큰 문자열(crs 포함).
fn bbox5_of(v0: f64, v1: f64, v2: f64, v3: f64, crs: &str) -> String {
    format!("{v0},{v1},{v2},{v3},{crs}")
}

/// bbox [lat_min, lon_min, lat_max, lon_max]를 4등분(2×2 격자).
fn split_bbox4(b: [f64; 4]) -> [[f64; 4]; 4] {
    let [lat_min, lon_min, lat_max, lon_max] = b;
    let lat_mid = (lat_min + lat_max) / 2.0;
    let lon_mid = (lon_min + lon_max) / 2.0;
    [
        [lat_min, lon_min, lat_mid, lon_mid],
        [lat_min, lon_mid, lat_mid, lon_max],
        [lat_mid, lon_min, lat_max, lon_mid],
        [lat_mid, lon_mid, lat_max, lon_max],
    ]
}

/// 주소 반경 bbox 격자수집 — BFS 레벨동기 분할 + PNU dedup.
/// 각 레벨마다 run_jobs로 count 프로브 병렬 → ≥1000이면 4분할, <1000이면 leaf.
/// leaf들을 run_jobs로 WFS GetFeature 병렬 수집 → PNU dedup.
async fn harvest_wfs_bbox(
    client: &Client,
    endpoint_op: &str,
    initial_bbox5: &str,
    crs: &str,
    auths: &[Auth],
    concurrency: usize,
) -> Result<(Vec<Value>, usize, u32)> {
    const CAP: u64 = 1000;
    const MAX_DEPTH: u32 = 8;

    let initial = parse_bbox5(initial_bbox5)
        .ok_or_else(|| anyhow!("bbox 파싱 실패: {initial_bbox5}"))?;

    let mut frontier: Vec<[f64; 4]> = vec![initial];
    let mut leaves: Vec<[f64; 4]> = Vec::new();
    let mut max_depth = 0u32;

    // BFS 레벨동기: 각 레벨에서 count 프로브 병렬
    for depth in 0..MAX_DEPTH {
        if frontier.is_empty() {
            break;
        }
        max_depth = depth;
        let op = endpoint_op.to_string();
        let crs2 = crs.to_string();
        let client2 = client.clone();
        let jobs: Vec<(usize, [f64; 4])> = frontier.iter().cloned().enumerate().collect();
        let results = crate::concurrency::run_jobs(
            jobs,
            auths.to_vec(),
            concurrency,
            move |(_, b), auth| {
                let client = client2.clone();
                let op = op.clone();
                let crs = crs2.clone();
                async move {
                    let bbox5 = bbox5_of(b[0], b[1], b[2], b[3], &crs);
                    let n = wfs_count_bbox(&client, &op, &bbox5, &crs, &auth).await?;
                    Ok::<([f64; 4], u64), anyhow::Error>((b, n))
                }
            },
        )
        .await;

        let mut next: Vec<[f64; 4]> = Vec::new();
        for (slot, r) in results.into_iter().enumerate() {
            match r {
                Ok((b, n)) => {
                    if n == 0 {
                        // 빈 칸: 스킵
                    } else if n < CAP {
                        leaves.push(b);
                    } else {
                        // depth가 MAX_DEPTH-1이면 더 분할 불가 → leaf로 강등 + 경고
                        if depth + 1 >= MAX_DEPTH {
                            eprintln!(
                                "[경고] bbox 깊이 상한({MAX_DEPTH}) 도달: {n}건 부분수집(분할불가). \
                                 bbox={}",
                                bbox5_of(b[0], b[1], b[2], b[3], crs)
                            );
                            leaves.push(b);
                        } else {
                            for sub in split_bbox4(b) {
                                next.push(sub);
                            }
                        }
                    }
                }
                Err(e) => {
                    // 프로브 에러: leaf로 강등(보수적)
                    eprintln!("[경고] count 프로브 실패(slot {slot}): {e}");
                    leaves.push(frontier[slot]);
                }
            }
        }
        frontier = next;
    }

    // 남은 frontier(MAX_DEPTH 도달)도 leaf 처리
    for b in frontier {
        leaves.push(b);
    }

    let total_cells = leaves.len();
    eprintln!(
        "[격자수집] leaf 격자 {total_cells}칸 수집 시작(최대깊이 {max_depth})"
    );

    // leaf들 WFS GetFeature 병렬 수집
    let op = endpoint_op.to_string();
    let crs2 = crs.to_string();
    let client2 = client.clone();
    let jobs: Vec<(usize, [f64; 4])> = leaves.iter().cloned().enumerate().collect();
    let results = crate::concurrency::run_jobs(
        jobs,
        auths.to_vec(),
        concurrency,
        move |(_, b), auth| {
            let client = client2.clone();
            let op = op.clone();
            let crs = crs2.clone();
            async move {
                let bbox5 = bbox5_of(b[0], b[1], b[2], b[3], &crs);
                let (url, mut params) = NedBuilder::new(NedKind::Wfs, &op).build();
                params.push(("bbox".into(), bbox5));
                params.push(("srsName".into(), crs.clone()));
                params.push(("maxFeatures".into(), CAP.to_string()));
                let body = resilient_get_text(&client, &url, params, &auth).await?;
                let v = normalize::parse_to_json(&body)?;
                let feats: Vec<Value> = v
                    .get("features")
                    .and_then(|f| f.as_array())
                    .cloned()
                    .unwrap_or_default();
                Ok::<Vec<Value>, anyhow::Error>(feats)
            }
        },
    )
    .await;

    // 병합 + PNU dedup
    let mut by_pnu: std::collections::BTreeMap<String, Value> =
        std::collections::BTreeMap::new();
    for (slot, r) in results.into_iter().enumerate() {
        match r {
            Ok(feats) => {
                for f in feats {
                    let pnu = f
                        .get("properties")
                        .and_then(|p| p.get("pnu"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let key = if pnu.is_empty() {
                        f.get("id")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("_{}", by_pnu.len()))
                    } else {
                        pnu
                    };
                    by_pnu.entry(key).or_insert(f);
                }
            }
            Err(e) => {
                eprintln!("[경고] leaf fetch 실패(slot {slot}): {e}");
            }
        }
    }

    Ok((by_pnu.into_values().collect(), total_cells, max_depth))
}

/// WFS `totalFeatures`(cap 무관 진짜 건수)를 1건 조회로 확인(견고 재시도).
async fn wfs_count(client: &Client, endpoint_op: &str, pnu: &str, auth: &Auth) -> Result<u64> {
    let (url, mut params) = NedBuilder::new(NedKind::Wfs, endpoint_op).set("pnu", pnu).build();
    params.push(("maxFeatures".into(), "1".into()));
    let body = resilient_get_text(client, &url, params, auth).await?;
    let v = normalize::parse_to_json(&body)?;
    Ok(v.get("totalFeatures").and_then(|x| x.as_u64()).unwrap_or(0))
}

/// WFS 전수 수집 — PNU 접두를 1000 이하 조각으로 적응 분할(무효 접두는 count 0으로 가지치기).
///
/// 병렬화: ① 접두 분할은 **레벨별 병렬 count 프로브**, ② leaf fetch는 **키풀 병렬**.
/// 키가 여러 개면 라운드로빈으로 분산돼 harvest도 가속된다.
async fn harvest_wfs_all(
    client: &Client,
    endpoint_op: &str,
    prefix: &str,
    auths: &[Auth],
    concurrency: usize,
    extra: &[(String, String)],
) -> Result<Vec<Value>> {
    const CAP: u64 = 1000;

    // 1) 적응 분할(레벨별 병렬 BFS): 각 frontier 접두의 count를 병렬 조회.
    let mut leaves: Vec<String> = Vec::new();
    let mut frontier: Vec<String> = vec![prefix.to_string()];
    let mut depth = 0u32;
    while !frontier.is_empty() && depth < 16 {
        let op = endpoint_op.to_string();
        let client2 = client.clone();
        let jobs: Vec<(usize, String)> = frontier.iter().cloned().enumerate().collect();
        let results = crate::concurrency::run_jobs(jobs, auths.to_vec(), concurrency, move |(_i, p), auth| {
            let client = client2.clone();
            let op = op.clone();
            async move {
                let n = wfs_count(&client, &op, &p, &auth).await?;
                Ok::<(String, u64), anyhow::Error>((p, n))
            }
        })
        .await;

        let mut next: Vec<String> = Vec::new();
        for (slot, r) in results.into_iter().enumerate() {
            match r {
                Ok((p, n)) => {
                    if n == 0 {
                        continue;
                    }
                    if n <= CAP || p.len() >= 19 {
                        leaves.push(p);
                    } else {
                        for d in 0..10 {
                            next.push(format!("{p}{d}"));
                        }
                    }
                }
                // 프로브 에러(서버 일시 장애 등): 해당 접두를 다음 라운드로 재투입.
                Err(_) => next.push(frontier[slot].clone()),
            }
        }
        frontier = next;
        depth += 1;
    }

    // 2) leaf 전수 fetch(키풀 병렬), PNU 기준 dedup. 에러 leaf는 재시도 라운드.
    let mut by_pnu: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    let mut pending = leaves.clone();
    let mut round = 0u32;
    while !pending.is_empty() && round < 4 {
        let conc = if round == 0 { concurrency } else { 2 };
        let op = endpoint_op.to_string();
        let extra = extra.to_vec();
        let client2 = client.clone();
        let jobs: Vec<(usize, String)> = pending.iter().cloned().enumerate().collect();
        let results = crate::concurrency::run_jobs(jobs, auths.to_vec(), conc, move |(_i, leaf), auth| {
            let client = client2.clone();
            let op = op.clone();
            let extra = extra.clone();
            async move {
                let (url, mut params) = NedBuilder::new(NedKind::Wfs, &op).set("pnu", &leaf).build();
                params.push(("maxFeatures".into(), CAP.to_string()));
                for (k, v) in &extra {
                    params.push((k.clone(), v.clone()));
                }
                let body = resilient_get_text(&client, &url, params, &auth).await?;
                let v = normalize::parse_to_json(&body)?;
                let feats: Vec<Value> = v
                    .get("features")
                    .and_then(|f| f.as_array())
                    .cloned()
                    .unwrap_or_default();
                Ok::<Vec<Value>, anyhow::Error>(feats)
            }
        })
        .await;

        let mut failed: Vec<String> = Vec::new();
        for (slot, r) in results.into_iter().enumerate() {
            match r {
                Ok(feats) => {
                    for f in feats {
                        let pnu = f
                            .get("properties")
                            .and_then(|p| p.get("pnu"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string();
                        let key = if pnu.is_empty() {
                            format!("_{}", by_pnu.len())
                        } else {
                            pnu
                        };
                        by_pnu.insert(key, f);
                    }
                }
                Err(_) => failed.push(pending[slot].clone()),
            }
        }
        pending = failed;
        round += 1;
    }
    Ok(by_pnu.into_values().collect())
}

// ───────────────────────── WMS / WFS ─────────────────────────

#[derive(Args, Debug)]
pub struct WmsArgs {
    /// 오퍼레이션(GetMap/GetCapabilities).
    #[arg(long, default_value = "GetCapabilities")]
    pub request: String,
    #[arg(long)]
    pub layers: Option<String>,
    #[arg(long)]
    pub bbox: Option<String>,
    #[arg(long)]
    pub width: Option<u32>,
    #[arg(long)]
    pub height: Option<u32>,
    /// 좌표계. 주의: EPSG:4326·5185~5188은 bbox가 (ymin,xmin,ymax,xmax)=위도,경도 순(WMS 1.3.0).
    #[arg(long, default_value = "EPSG:4326")]
    pub crs: String,
    /// 레이어별 스타일(GetMap, 생략 시 기본 스타일).
    #[arg(long)]
    pub styles: Option<String>,
    /// 이미지 포맷(GetMap).
    #[arg(long, default_value = "image/png")]
    pub format: String,
    /// 투명 배경(GetMap).
    #[arg(long)]
    pub transparent: bool,
    /// GetMap 이미지 저장 경로(PNG 등). GetMap은 필수.
    #[arg(long, short)]
    pub output: Option<std::path::PathBuf>,
}

pub async fn run_wms(g: &GlobalArgs, a: WmsArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let width = a.width.map(|w| w.to_string());
    let height = a.height.map(|h| h.to_string());
    let is_getmap = a.request.eq_ignore_ascii_case("GetMap");
    // WMS는 OGC 표준 version=1.3.0(서버 유효값 [1.3.0]). version=2.0은 ServiceException.
    let mut b = QueryBuilder::new("wms", &a.request)
        .version("1.3.0")
        .set("crs", &a.crs)
        .opt("layers", a.layers.as_deref())
        .opt("styles", a.styles.as_deref())
        .opt("bbox", a.bbox.as_deref())
        .opt("width", width.as_deref())
        .opt("height", height.as_deref());
    if is_getmap {
        b = b.set("format", &a.format);
        if a.transparent {
            b = b.set("transparent", "TRUE");
        }
    }
    let (url, params) = b.build();

    // GetMap은 이미지(PNG) — get_bytes로 받아 파일 저장. (텍스트/JSON 파싱 경로로는 깨짐)
    if is_getmap {
        let path = a.output.as_ref().ok_or_else(|| {
            anyhow!("GetMap은 이미지 출력이라 `-o <파일.png>`가 필요합니다.")
        })?;
        let bytes = client.get_bytes(&url, params, pick_auth(&auths)).await?;
        let saved = output::save_bytes(path, &bytes)?;
        return output::print_json(g, &serde_json::json!({"ok": true, "saved": saved}));
    }

    // GetCapabilities/GetFeatureInfo 등 텍스트 응답.
    fetch_one(g, &client, url, params, pick_auth(&auths)).await
}

#[derive(Args, Debug)]
pub struct WfsArgs {
    #[arg(long, default_value = "GetFeature")]
    pub request: String,
    #[arg(long)]
    pub typename: Option<String>,
    #[arg(long)]
    pub bbox: Option<String>,
    #[arg(long)]
    pub pnu: Option<String>,
    #[arg(long)]
    pub max_features: Option<u32>,
    #[arg(long, default_value = "EPSG:4326")]
    pub crs: String,
    /// HTML 뷰어로 저장(미지정 시 GeoJSON/JSON 출력). 토스 디자인 지도에 피처를 그림.
    #[arg(long, short)]
    pub output: Option<std::path::PathBuf>,
}

pub async fn run_wfs(g: &GlobalArgs, a: WfsArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let maxf = a.max_features.map(|m| m.to_string());
    // WFS는 OGC 표준: version=1.1.0 + outputFormat(JSON) + srsName(좌표계) 필수.
    // typename은 소문자여야 매칭됨. (version 2.0/crs/output 조합은 빈 결과를 냄)
    let typename = a.typename.as_ref().map(|t| t.to_lowercase());
    let (url, mut params) = QueryBuilder::new("wfs", &a.request)
        .version("1.1.0")
        .opt("typename", typename.as_deref())
        .opt("bbox", a.bbox.as_deref())
        .opt("pnu", a.pnu.as_deref())
        .opt("maxFeatures", maxf.as_deref())
        .set("srsName", &a.crs)
        .build();
    params.push(("outputFormat".into(), "application/json".into()));

    // HTML 뷰어 출력: GeoJSON을 토스 디자인 2D 지도에 오버레이.
    if let Some(path) = &a.output {
        let body = client.get_text(&url, params, pick_auth(&auths)).await?;
        let key = &pick_auth(&auths).key;
        let domain = pick_auth(&auths).domain.clone().unwrap_or_default();
        let html = super::embed_cmds::render_wfs_viewer(&body, a.bbox.as_deref(), key, &domain);
        let saved = output::save_bytes(path, html.as_bytes())?;
        return output::print_json(g, &serde_json::json!({"ok": true, "saved": saved}));
    }

    fetch_one(g, &client, url, params, pick_auth(&auths)).await
}

// ───────────────────────── catalog ─────────────────────────

#[derive(Args, Debug)]
pub struct CatalogArgs {
    /// 오퍼레이션: datasets / gids / gid-datasets.
    #[arg(default_value = "datasets")]
    pub op: String,
    #[arg(long)]
    pub gid_cd: Option<String>,
    #[arg(long)]
    pub ds_id: Option<String>,
    #[arg(long)]
    pub page: Option<u32>,
    #[arg(long)]
    pub num_rows: Option<u32>,
}

pub async fn run_catalog(g: &GlobalArgs, a: CatalogArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let op_path = match a.op.as_str() {
        "datasets" => "dtmk/getDatasetList.do",
        "gids" => "dtmk/getGidList.do",
        "gid-datasets" => "dtmk/getGidDsList.do",
        other => return Err(anyhow!("알 수 없는 catalog op: {other} (datasets/gids/gid-datasets)")),
    };
    let url = format!("{}/{}", crate::api::NED_BASE, op_path);
    let page = a.page.map(|p| p.to_string());
    let num = a.num_rows.map(|n| n.to_string());
    let mut params = vec![("format".to_string(), "json".to_string())];
    if let Some(v) = &a.gid_cd {
        params.push(("gid_cd".into(), v.clone()));
    }
    if let Some(v) = &a.ds_id {
        params.push(("ds_id".into(), v.clone()));
    }
    if let Some(v) = &page {
        params.push(("pageNo".into(), v.clone()));
    }
    if let Some(v) = &num {
        params.push(("numOfRows".into(), v.clone()));
    }
    fetch_one(g, &client, url, params, pick_auth(&auths)).await
}

// ───────────────────────── batch ─────────────────────────

#[derive(Args, Debug)]
pub struct BatchArgs {
    /// 배치 대상 명령(현재 geocode 지원).
    pub command: String,
    /// 입력 파일(줄당 1건).
    #[arg(long)]
    pub from: std::path::PathBuf,
    #[arg(long, default_value = "ROAD")]
    pub r#type: String,
    #[arg(long)]
    pub reverse: bool,
}

pub async fn run_batch(g: &GlobalArgs, a: BatchArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let lines = read_lines(&a.from)?;
    match a.command.as_str() {
        "geocode" | "geo" => {
            let reverse = a.reverse;
            let ty = a.r#type.clone();
            let request = if reverse { "GetAddress" } else { "GetCoord" };
            let build = move |q: &str| -> (String, Vec<(String, String)>) {
                let mut b = QueryBuilder::new("address", request)
                    .format("json")
                    .set("type", &ty)
                    .set("crs", "EPSG:4326");
                if reverse {
                    b = b.set("point", q);
                } else {
                    b = b.set("address", q);
                }
                b.build()
            };
            run_batch_lines(g, &client, &auths, lines, build).await
        }
        other => Err(anyhow!("batch 미지원 명령: {other} (현재 geocode 지원)")),
    }
}

// ───────────────────────── 배치 공통 ─────────────────────────

fn read_lines(path: &std::path::Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("입력 파일 읽기 실패 {}: {e}", path.display()))?;
    Ok(text
        .split(['\n'])
        .map(|l| l.trim_end_matches('\r').trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect())
}

fn pick_auth(auths: &[Auth]) -> &Auth {
    &auths[0]
}

/// 줄 입력을 병렬 처리하고 index 순서 보존 JSON 배열 출력(§5.1).
async fn run_batch_lines<B>(
    g: &GlobalArgs,
    client: &Client,
    auths: &[Auth],
    lines: Vec<String>,
    build: B,
) -> Result<()>
where
    B: Fn(&str) -> (String, Vec<(String, String)>) + Send + Sync + 'static,
{
    let concurrency = g.concurrency.unwrap_or_else(|| auths.len().max(2));
    let client = client.clone();
    let build = std::sync::Arc::new(build);
    let raw = g.raw;

    let jobs: Vec<(usize, String)> = lines.into_iter().enumerate().collect();
    let results = crate::concurrency::run_jobs(
        jobs,
        auths.to_vec(),
        concurrency,
        move |(idx, line), auth| {
            let client = client.clone();
            let build = build.clone();
            async move {
                let (url, params) = build(&line);
                let body = client.get_text(&url, params, &auth).await?;
                let value = if raw {
                    Value::String(body)
                } else {
                    let v = normalize::parse_to_json(&body)?;
                    match normalize::check_body_error(&v) {
                        Ok(()) => v,
                        Err(e) if e.empty_ok => Value::Null,
                        Err(e) => return Err(anyhow!("{e}")),
                    }
                };
                Ok::<(usize, Value), anyhow::Error>((idx, value))
            }
        },
    )
    .await;

    let items: Vec<output::BatchItem> = results
        .into_iter()
        .enumerate()
        .map(|(i, r)| match r {
            Ok((idx, v)) => output::BatchItem {
                index: idx,
                result: Some(v),
                error: None,
            },
            Err(e) => output::BatchItem {
                index: i,
                result: None,
                error: Some(serde_json::json!({"message": e.to_string()})),
            },
        })
        .collect();

    output::print_json(g, &serde_json::json!({"ok": true, "count": items.len(), "items": items}))
}
