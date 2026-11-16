//! vworld-mcp — VWorld OpenAPI 를 읽기 전용 도구로 내놓는 MCP 서버(stdio · Streamable HTTP).
//!
//! 공통 규약(`<name>-mcp`): stdout 에는 JSON-RPC 만, 진단은 stderr(키 없이).
//! 로직은 라이브러리 `vworld::mcp`(키 선택·스크럽·검증·도구 실행)에 있고, 여기는 rmcp 연결·
//! 진행 알림·취소, `web` 모듈은 단독 HTTP 모드(인증·Origin·세션·SSE)를 맡는다.

mod web;

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{
        CallToolResult, ContentBlock, ErrorData, Implementation, ProgressNotificationParam,
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, RequestId,
        ServerCapabilities, ServerConfig,
    },
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router, RoleServer, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vworld::api::{Auth, Client};
use vworld::mcp::{self, DataInput, KeySource, NedInput, SearchInput, META_KEYS};
use vworld::mcp_results::{self, Format, Shape, Store};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ───────────────────────── 도구 인자 ─────────────────────────

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct GeocodeArgs {
    /// 주소(도로명 또는 지번). 예: "세종대로 110", "서울 종로구 세종로 1-68"
    #[schemars(length(min = 1, max = 200))]
    address: String,
    /// 주소 유형: auto(기본, 도로명→지번 자동 폴백) · ROAD · PARCEL. 모르면 auto — 유형이 틀리면 빈 결과
    #[schemars(pattern(r"^(auto|ROAD|PARCEL)$"))]
    r#type: Option<String>,
    /// 응답 좌표계. 기본 EPSG:4326(경도·위도). 예: EPSG:5186(중부원점 TM, 미터)
    #[schemars(pattern(r"^EPSG:[0-9]{4,6}$"))]
    crs: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ReverseArgs {
    /// x 좌표(EPSG:4326 이면 경도). 예: 126.978
    x: f64,
    /// y 좌표(EPSG:4326 이면 위도). 예: 37.566
    y: f64,
    /// 돌려받을 주소: BOTH(기본, 지번+도로명) · ROAD · PARCEL
    #[schemars(pattern(r"^(BOTH|ROAD|PARCEL)$"))]
    r#type: Option<String>,
    /// 입력 좌표계. 기본 EPSG:4326
    #[schemars(pattern(r"^EPSG:[0-9]{4,6}$"))]
    crs: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct SearchArgs {
    /// 검색어(장소명·주소·행정구역명·도로명). 예: "광화문", "종로구"
    #[schemars(length(min = 1, max = 100))]
    query: String,
    /// 검색 대상: PLACE(기본) · ADDRESS · DISTRICT · ROAD
    #[schemars(pattern(r"^(PLACE|ADDRESS|DISTRICT|ROAD)$"))]
    r#type: Option<String>,
    /// 카테고리. ADDRESS 는 ROAD|PARCEL 필수, DISTRICT 는 L1(시도)·L2(시군구)·L3(읍면동)·L4(리) 필수. PLACE·ROAD 는 생략
    #[schemars(pattern(r"^(ROAD|PARCEL|L[1-4]|[A-Za-z0-9_]{1,20})$"))]
    category: Option<String>,
    /// 페이지 크기 1~100(기본 서버값 10)
    #[schemars(range(min = 1, max = 100))]
    size: Option<u32>,
    /// 페이지 번호(1부터)
    #[schemars(range(min = 1, max = 1000))]
    page: Option<u32>,
    /// 검색 영역 "minx,miny,maxx,maxy"(경도,위도)
    #[schemars(length(max = 120))]
    bbox: Option<String>,
    /// 좌표계. 기본 EPSG:4326
    #[schemars(pattern(r"^EPSG:[0-9]{4,6}$"))]
    crs: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct LayersArgs {
    /// 데이터ID·레이어명·카테고리에 들어간 낱말(예: "지적", "용도", "도로")
    #[schemars(length(max = 50))]
    search: Option<String>,
    /// 카테고리 정확히 일치
    #[schemars(length(max = 50))]
    cat: Option<String>,
    /// 도형 유형: polygon · line · point
    #[schemars(pattern(r"^(polygon|line|point)$"))]
    geom: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct DescribeArgs {
    /// 2D 데이터ID(예: LP_PA_CBND_BUBUN, LT_C_UQ111)
    #[schemars(pattern(r"^[A-Za-z0-9_]{1,40}$"))]
    data_id: String,
}

/// 큰 결과 모양 인자(vworld_data · vworld_ned 공통).
#[derive(Deserialize, schemars::JsonSchema, Default)]
#[schemars(crate = "rmcp::schemars")]
struct ShapeArgs {
    /// 결과 형식: json(속성 객체 배열) · columns(columns+rows+dict, 큰 결과 기본) · csv(머리줄+행) · geojson(도형 포함 FeatureCollection). json·columns·csv 는 도형 없이 속성만. 비우면 작은 결과는 원래 응답 그대로
    #[schemars(pattern(r"^(json|columns|csv|geojson)$"))]
    format: Option<String>,
    /// 남길 속성 이름(받아온 뒤 고르기). 예: ["pnu","jibun"]
    #[schemars(length(min = 1, max = 100))]
    properties: Option<Vec<String>>,
    /// 받아온 뒤 거르기 {"속성": 값 | [값,...] | {"from":..,"to":..}}. 예: {"jibun":{"from":"1","to":"99"}}
    filter: Option<Map<String, Value>>,
    /// geojson 좌표 소수 자릿수 0~10(6 ≈ 10cm)
    #[schemars(range(min = 0, max = 10))]
    precision: Option<u32>,
    /// true 면 작아도 파일로 저장하고 내려받기 링크(resource_link)를 줘요
    save: Option<bool>,
}

impl ShapeArgs {
    fn into_shape(self) -> anyhow::Result<Shape> {
        let shape = Shape {
            format: self.format.as_deref().map(Format::parse).transpose()?,
            properties: self.properties,
            filter: self.filter,
            precision: self.precision,
            save: self.save.unwrap_or(false),
        };
        shape.validate()?;
        Ok(shape)
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct PageArgs {
    /// 앞 응답의 next_cursor
    #[schemars(length(min = 1, max = 512))]
    cursor: String,
    /// 이번 쪽 최대 행 수(비우면 상한 안에서 최대)
    #[schemars(range(min = 1, max = 100000))]
    limit: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct DataArgs {
    /// 2D 데이터ID(예: LP_PA_CBND_BUBUN 연속지적도). 목록은 vworld_data_layers
    #[schemars(pattern(r"^[A-Za-z0-9_]{1,40}$"))]
    data_id: String,
    /// 공간 필터 POINT(x y) · BOX(minx,miny,maxx,maxy) · POLYGON((x y,...)). 좌표는 EPSG:4326, EPSG 접미사 금지, BOX/POLYGON 면적 10km² 이내
    #[schemars(
        length(max = 4000),
        pattern(r"^(POINT|BOX|POLYGON|LINESTRING|MULTIPOLYGON)\(.*\)$")
    )]
    geom_filter: Option<String>,
    /// 속성 필터 "속성:연산자:값" 을 | 로 이음. 예: "pnu:=:1114010300100310000"
    #[schemars(length(max = 500))]
    attr_filter: Option<String>,
    /// 읍면동코드(8~10자리). 일부 데이터셋은 단독으로 INVALID_RANGE — geom/attr 필터 권장
    #[schemars(pattern(r"^[0-9]{8,10}$"))]
    emd_cd: Option<String>,
    /// 돌려받을 속성(쉼표 구분). 예: "pnu,jibun"
    #[schemars(pattern(r"^[A-Za-z0-9_,]{1,500}$"))]
    columns: Option<String>,
    /// 건수 1~1000(기본 100)
    #[schemars(range(min = 1, max = 1000))]
    size: Option<u32>,
    /// 페이지 번호(1부터)
    #[schemars(range(min = 1, max = 10000))]
    page: Option<u32>,
    /// 여러 쪽 이어 받기 1~10(기본 1). page 부터 받아 features 를 합쳐요(진행 알림 지원)
    #[schemars(range(min = 1, max = 10))]
    pages: Option<u32>,
    /// 응답 좌표계. 기본 EPSG:4326
    #[schemars(pattern(r"^EPSG:[0-9]{4,6}$"))]
    crs: Option<String>,
    #[serde(flatten)]
    shape: ShapeArgs,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ZoningArgs {
    /// 주소(도로명·지번). address 또는 x·y 중 하나만
    #[schemars(length(min = 1, max = 200))]
    address: Option<String>,
    /// 경도(EPSG:4326). y 와 함께
    x: Option<f64>,
    /// 위도(EPSG:4326). x 와 함께
    y: Option<f64>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct NedListArgs {
    /// 계열 필터: data(속성) · wfs(도형) · wms(이미지, 호출 불가)
    #[schemars(pattern(r"^(data|wfs|wms)$"))]
    kind: Option<String>,
    /// 오퍼레이션명·이름·분류에 들어간 낱말(예: "공시지가", "건물", "LandUse")
    #[schemars(length(max = 50))]
    search: Option<String>,
    /// 오퍼레이션 하나의 요청변수(필수 여부·예시값)를 볼 때. 예: getLandUseAttr
    #[schemars(pattern(r"^[A-Za-z0-9_]{1,60}$"))]
    operation: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct NedArgs {
    /// NED 오퍼레이션명(data·wfs 계열). 예: getLandUseAttr, getIndvdLandPriceAttr, getBuildingAge, getIndvdLandPriceWFS
    #[schemars(pattern(r"^[A-Za-z0-9_]{1,60}$"))]
    operation: String,
    /// 필지고유번호(PNU) 8~19자리. 8자리면 법정동 단위
    #[schemars(pattern(r"^[0-9]{8,19}$"))]
    pnu: Option<String>,
    /// bbox "minx,miny,maxx,maxy[,EPSG:코드]"(WFS). WFS 는 EPSG:4326 일 때 축이 (ymin,xmin,ymax,xmax)
    #[schemars(length(max = 160))]
    bbox: Option<String>,
    /// 좌표계. WFS 는 srsName 으로 전송(기본 EPSG:4326), data 계열은 crs(기본 EPSG:5187)
    #[schemars(pattern(r"^EPSG:[0-9]{4,6}$"))]
    crs: Option<String>,
    /// 그 밖의 요청변수 {이름: 값}. 예: {"stdrYear":"2024","numOfRows":"100","maxFeatures":"100"}. key·domain·format·output 은 못 줌
    params: Option<Map<String, Value>>,
    #[serde(flatten)]
    shape: ShapeArgs,
}

// ───────────────────────── 서버 ─────────────────────────

pub(crate) struct State {
    client: Client,
    defaults: Vec<Auth>,
    domain: Option<String>,
    store: Store,
}

/// 도구 결과(텍스트 + resource_link 항목).
pub(crate) struct Reply {
    text: String,
    links: Vec<Value>,
}

impl From<Value> for Reply {
    fn from(v: Value) -> Self {
        Reply {
            text: serde_json::to_string(&v).unwrap_or_else(|e| format!("직렬화 실패: {e}")),
            links: vec![],
        }
    }
}

impl From<mcp_results::Shaped> for Reply {
    fn from(s: mcp_results::Shaped) -> Self {
        Reply {
            text: s.text,
            links: s.links,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Server {
    st: Arc<State>,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Server>,
}

/// 진행 보고 콜백(도구 → 라이브러리). 보내기는 [`progress_forwarder`] 가 묶어서 한다.
type Reporter = Arc<dyn Fn(u32, u32, &str) + Send + Sync>;

/// 취소된 요청(세션|id). 그 요청의 응답은 전송 계층(stdio 필터 · HTTP 게이트)이 버린다.
static CANCELLED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// 취소 표시 키. stdio 는 세션이 없어 빈 문자열.
pub(crate) fn cancel_key(session: &str, id_json: &str) -> String {
    format!("{session}|{id_json}")
}

fn mark_cancelled(key: String) {
    CANCELLED
        .lock()
        .unwrap()
        .get_or_insert_with(HashSet::new)
        .insert(key);
}

/// 취소된 요청이면 표시를 지우고 true(그 응답은 보내지 않는다).
pub(crate) fn take_cancelled(key: &str) -> bool {
    CANCELLED
        .lock()
        .unwrap()
        .as_mut()
        .is_some_and(|s| s.remove(key))
}

/// 요청의 세션 id(HTTP 모드에서 rmcp 가 넣어 준 요청 머리글). stdio 면 "".
fn session_of(ctx: &RequestContext<RoleServer>) -> String {
    ctx.extensions
        .get::<::http::request::Parts>()
        .and_then(|p| p.headers.get("mcp-session-id"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// 진행 알림 전송기: progressToken 이 있으면 (콜백, 끝날 때까지 기다릴 핸들).
/// 1초 안에 여러 번 오면 마지막 것만 남겨 묶어 보낸다(시작은 바로, 끝은 반드시 보냄).
fn progress_forwarder(
    ctx: &RequestContext<RoleServer>,
) -> (Reporter, Option<tokio::task::JoinHandle<()>>) {
    let Some(token) = ctx.meta.get_progress_token() else {
        return (Arc::new(mcp::no_progress), None);
    };
    let peer = ctx.peer.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u32, u32, String)>();
    let handle = tokio::spawn(async move {
        let send = |(p, t, m): (u32, u32, String)| {
            let param = ProgressNotificationParam::new(token.clone(), p as f64)
                .with_total(t as f64)
                .with_message(m);
            let peer = peer.clone();
            async move {
                let _ = peer.notify_progress(param).await;
            }
        };
        let gap = Duration::from_secs(1);
        let mut last: Option<Instant> = None;
        let mut pending: Option<(u32, u32, String)> = None;
        loop {
            let wait = match (last, &pending) {
                (Some(l), Some(_)) => (l + gap).saturating_duration_since(Instant::now()),
                _ => Duration::from_secs(3600),
            };
            tokio::select! {
                m = rx.recv() => match m {
                    Some(item) => {
                        if last.is_none_or(|l| l.elapsed() >= gap) {
                            send(item).await;
                            last = Some(Instant::now());
                            pending = None;
                        } else {
                            pending = Some(item);
                        }
                    }
                    None => break,
                },
                _ = tokio::time::sleep(wait), if pending.is_some() => {
                    if let Some(item) = pending.take() {
                        send(item).await;
                        last = Some(Instant::now());
                    }
                }
            }
        }
        if let Some(item) = pending {
            send(item).await;
        }
    });
    let rep: Reporter = Arc::new(move |p, t, m: &str| {
        let _ = tx.send((p, t, m.to_string()));
    });
    (rep, Some(handle))
}

impl Server {
    pub(crate) fn new(defaults: Vec<Auth>, domain: Option<String>) -> anyhow::Result<Self> {
        Ok(Server {
            st: Arc::new(State {
                client: Client::new()?,
                defaults,
                domain,
                store: Store::from_env(),
            }),
            tool_router: Self::tool_router(),
        })
    }

    /// 결과 저장소(정리 작업 · HTTP 내려받기용).
    pub(crate) fn store(&self) -> Store {
        self.st.store.clone()
    }

    #[cfg(test)]
    pub(crate) fn with_store(mut self, store: Store) -> Self {
        Arc::get_mut(&mut self.st).expect("생성 직후").store = store;
        self
    }

    /// 기본 키 원문(스크럽 대상) — HTTP 계층이 응답 전체를 지울 때 쓴다.
    pub(crate) fn default_secrets(&self) -> Vec<String> {
        self.st.defaults.iter().map(|a| a.key.clone()).collect()
    }

    /// 이번 요청의 키(요청별 `_meta` 우선, 없으면 기본 키)와 도메인.
    ///
    /// `_meta["kr.pes/domain"]` 이 있으면 이 호출의 모든 VWorld 요청에 그 domain 을 쓴다
    /// (`VWORLD_DOMAIN`·설정 파일보다 우선). 목록은 호출마다 새로 만들어 동시 호출끼리 섞이지 않는다.
    fn keys(&self, ctx: &RequestContext<RoleServer>) -> Result<Vec<Auth>, String> {
        let domain = mcp::select_domain(ctx.meta.get(mcp::META_DOMAIN))?;
        let (auths, _) = mcp::select_auths(
            ctx.meta.get(META_KEYS),
            &self.st.defaults,
            self.st.domain.as_deref(),
        )?;
        Ok(mcp::with_domain(auths, domain.as_deref()))
    }

    /// 결과를 텍스트로 만들고, 요청 키·기본 키를 지운 뒤 크기를 제한한다.
    fn finish<T: Into<Reply>>(&self, used: &[Auth], r: anyhow::Result<T>) -> CallToolResult {
        let secrets: Vec<&str> = used
            .iter()
            .chain(self.st.defaults.iter())
            .map(|a| a.key.as_str())
            .collect();
        let (reply, ok) = match r {
            Ok(v) => (v.into(), true),
            Err(e) => (
                Reply {
                    text: format!("{e:#}"),
                    links: vec![],
                },
                false,
            ),
        };
        let mut content = vec![ContentBlock::text(mcp::cap_output(mcp::scrub(
            &reply.text,
            &secrets,
        )))];
        // 저장된 결과 파일 안내(MCP resource_link). 파일·링크에는 키가 없다.
        content.extend(
            reply
                .links
                .into_iter()
                .filter_map(|l| serde_json::from_value::<ContentBlock>(l).ok()),
        );
        if ok {
            CallToolResult::success(content)
        } else {
            CallToolResult::error(content)
        }
    }

    /// 키가 필요한 도구 공통: 키 선택 → [`Self::run`].
    async fn keyed<F, Fut, T>(&self, ctx: &RequestContext<RoleServer>, f: F) -> CallToolResult
    where
        F: FnOnce(Arc<State>, Vec<Auth>, Reporter) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<T>>,
        T: Into<Reply>,
    {
        let auths = match self.keys(ctx) {
            Ok(a) => a,
            Err(e) => return self.finish(&[], Err::<Reply, _>(anyhow::anyhow!(e))),
        };
        let st = self.st.clone();
        let used = auths.clone();
        self.run(ctx, &used, move |rep| f(st, auths, rep)).await
    }

    /// 실행 공통: 진행 알림 연결, 취소(`notifications/cancelled` · HTTP 연결 끊김) 시 작업 중단.
    ///
    /// 취소되면 작업 future 를 버려 진행 중인 VWorld 호출도 멈추고, 그 요청의 응답은
    /// 취소 표시로 전송 계층이 버린다(규격: 취소된 요청에는 답하지 않음).
    async fn run<F, Fut, T>(
        &self,
        ctx: &RequestContext<RoleServer>,
        used: &[Auth],
        f: F,
    ) -> CallToolResult
    where
        F: FnOnce(Reporter) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<T>>,
        T: Into<Reply>,
    {
        let (rep, forwarder) = progress_forwarder(ctx);
        let work = f(rep);
        let r = tokio::select! {
            r = work => Some(r),
            _ = ctx.ct.cancelled() => None,
        };
        if let Some(h) = forwarder {
            if r.is_some() {
                let _ = h.await; // 마지막 진행 알림이 응답보다 먼저 나가게.
            } else {
                h.abort();
            }
        }
        match r {
            Some(r) => self.finish(used, r),
            None => {
                let id = serde_json::to_string(&ctx.id).unwrap_or_default();
                mark_cancelled(cancel_key(&session_of(ctx), &id));
                self.finish(used, Err::<Reply, _>(anyhow::anyhow!("요청이 취소됐어요")))
            }
        }
    }
}

fn crs_or_4326(c: Option<String>) -> String {
    c.unwrap_or_else(|| "EPSG:4326".into())
}

#[tool_router]
impl Server {
    #[tool(
        description = "주소 → 좌표(지오코딩, VWorld /req/address GetCoord). type 기본 auto 는 도로명→지번 순으로 자동 폴백하므로 유형을 모르면 그대로 두세요(유형이 틀리면 빈 결과). 결과 point.x/y(기본 EPSG:4326 경도·위도)와 정제주소 refined. 필지 속성이 필요하면 이 좌표로 vworld_zoning 또는 vworld_data(LP_PA_CBND_BUBUN, POINT) → vworld_ned(pnu)",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn vworld_geocode(
        &self,
        Parameters(a): Parameters<GeocodeArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let ty = a.r#type.unwrap_or_else(|| "auto".into());
        let crs = crs_or_4326(a.crs);
        self.keyed(&ctx, |st, k, _p| async move {
            mcp::geocode(&st.client, &k, &a.address, &ty, &crs).await
        })
        .await
    }

    #[tool(
        description = "좌표 → 주소(역지오코딩, /req/address GetAddress). x=경도, y=위도(기본 EPSG:4326). type BOTH(기본)면 지번·도로명 주소를 함께 돌려줘요. 바다·경계 밖이면 결과 없음",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn vworld_reverse_geocode(
        &self,
        Parameters(a): Parameters<ReverseArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let ty = a.r#type.unwrap_or_else(|| "BOTH".into());
        let crs = crs_or_4326(a.crs);
        self.keyed(&ctx, |st, k, _p| async move {
            mcp::reverse_geocode(&st.client, &k, a.x, a.y, &ty, &crs).await
        })
        .await
    }

    #[tool(
        description = "장소·주소·행정구역·도로명 검색(/req/search). type PLACE(기본)·ADDRESS·DISTRICT·ROAD. 함정: ADDRESS 는 category ROAD|PARCEL, DISTRICT 는 category L1(시도)·L2(시군구)·L3(읍면동)·L4(리)가 필수(없으면 PARAM_REQUIRED). size 1~100, page, bbox 로 범위 제한",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn vworld_search(
        &self,
        Parameters(a): Parameters<SearchArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let input = SearchInput {
            query: a.query,
            kind: a.r#type.unwrap_or_else(|| "PLACE".into()),
            category: a.category,
            size: a.size,
            page: a.page,
            bbox: a.bbox,
            crs: crs_or_4326(a.crs),
        };
        self.keyed(&ctx, |st, k, _p| async move {
            mcp::search(&st.client, &k, &input).await
        })
        .await
    }

    #[tool(
        description = "2D 데이터 레이어 158종 목록(내장 레지스트리, 키·네트워크 불필요). search 낱말·cat·geom 으로 거르기. 표준 순서: vworld_data_layers(발견) → vworld_data_describe(속성 확인) → vworld_data(조회)",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn vworld_data_layers(&self, Parameters(a): Parameters<LayersArgs>) -> CallToolResult {
        self.finish(
            &[],
            mcp::data_layers(a.search.as_deref(), a.cat.as_deref(), a.geom.as_deref()),
        )
    }

    #[tool(
        description = "2D 데이터 레이어 하나의 속성표(속성명·타입·설명·단일검색 가능 키). 내장 레지스트리라 키 불필요. attr_filter·columns 에 쓸 속성명을 여기서 확인",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn vworld_data_describe(
        &self,
        Parameters(a): Parameters<DescribeArgs>,
    ) -> CallToolResult {
        self.finish(&[], mcp::data_describe(&a.data_id))
    }

    #[tool(
        description = "2D 데이터 조회(/req/data GetFeature) — 레이어 + 공간 필터(geom_filter) 또는 속성 필터(attr_filter). 둘 다 없으면 emd_cd 필요. 함정: geom_filter 에 EPSG 접미사 금지(좌표는 EPSG:4326 해석), BOX/POLYGON 은 면적 10km² 이내(넘으면 INVALID_RANGE — 나눠서 조회). size 기본 100·상한 1000, pages 로 여러 쪽. 좌표→필지(PNU): data_id=LP_PA_CBND_BUBUN, geom_filter=\"POINT(경도 위도)\". 큰 결과: 먼저 properties·filter 로 줄이세요. 응답이 약 200KB 를 넘으면 첫 쪽 + next_cursor(vworld_result_page) + 전체 파일(download·resource_link, csv·geojson)을 줘요. 형식 format: columns(기본)·csv·json·geojson(도형, precision 으로 좌표 줄이기)",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn vworld_data(
        &self,
        Parameters(a): Parameters<DataArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let input = DataInput {
            data_id: a.data_id,
            geom_filter: a.geom_filter,
            attr_filter: a.attr_filter,
            emd_cd: a.emd_cd,
            columns: a.columns,
            size: a.size,
            page: a.page,
            pages: a.pages,
            crs: crs_or_4326(a.crs),
        };
        let summary = json!({"data_id": input.data_id, "geom_filter": input.geom_filter,
            "attr_filter": input.attr_filter, "emd_cd": input.emd_cd, "columns": input.columns,
            "size": input.size, "page": input.page, "pages": input.pages, "crs": input.crs});
        let shape = a.shape.into_shape();
        self.keyed(&ctx, |st, k, p| async move {
            let shape = shape?;
            let out = mcp::data(&st.client, &k, &input, &*p).await?;
            let items = mcp::data_items(&out);
            mcp_results::shape_result(
                &st.store,
                "vworld_data",
                &summary,
                &out,
                items,
                &mcp::data_info(&out),
                &shape,
            )
        })
        .await
    }

    #[tool(
        description = "한 지점의 필지와 용도지역·지구(토지이용계획). address 하나 또는 x·y(EPSG:4326 경도·위도)를 주면 (지오코딩 →) 연속지적도로 PNU 를 찾고 NED getLandUseAttr 로 토지이용계획을 조회해 land_use_summary(용도지역지구명·저촉여부)와 원응답을 돌려줘요. 도로·하천 위 좌표는 필지가 없을 수 있어요",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn vworld_zoning(
        &self,
        Parameters(a): Parameters<ZoningArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        self.keyed(&ctx, |st, k, p| async move {
            mcp::zoning(&st.client, &k, a.address.as_deref(), a.x, a.y, &*p).await
        })
        .await
    }

    #[tool(
        description = "국가중점데이터(NED) 115 오퍼레이션 목록(내장 레지스트리, 키 불필요). 오퍼레이션명이 확실하지 않으면 먼저 여기서 찾으세요(예: getLandUse 가 아니라 getLandUseAttr). operation 을 주면 그 요청변수(필수 여부·예시값)를 보여줘요. wms 계열은 이미지라 이 서버에서 못 불러요",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn vworld_ned_list(&self, Parameters(a): Parameters<NedListArgs>) -> CallToolResult {
        self.finish(
            &[],
            mcp::ned_list(
                a.kind.as_deref(),
                a.search.as_deref(),
                a.operation.as_deref(),
            ),
        )
    }

    #[tool(
        description = "국가중점데이터(NED) data·WFS 단건 조회(/ned/data|wfs/<operation>). 먼저 vworld_ned_list 로 오퍼레이션명·필수 변수를 확인하세요. 필지 속성은 pnu(19자리, 주소면 vworld_zoning 이나 vworld_data 로 먼저 얻기). INVALID_TYPE 이면 pnu 를 8자리 이상으로. WFS 는 기본 EPSG:4326(srsName), 건수는 params.maxFeatures(최대 1000), data 계열은 params.numOfRows·pageNo. WMS(이미지) 계열은 거부. 큰 결과는 properties·filter 로 줄이고, 상한을 넘으면 첫 쪽 + next_cursor(vworld_result_page) + 전체 파일(download)을 줘요. format: columns·csv·json·geojson(WFS 만)",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn vworld_ned(
        &self,
        Parameters(a): Parameters<NedArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let input = NedInput {
            operation: a.operation,
            pnu: a.pnu,
            bbox: a.bbox,
            crs: a.crs,
            params: a.params.unwrap_or_default(),
        };
        let summary = json!({"operation": input.operation, "pnu": input.pnu, "bbox": input.bbox,
            "crs": input.crs, "params": input.params});
        let shape = a.shape.into_shape();
        self.keyed(&ctx, |st, k, p| async move {
            let shape = shape?;
            let out = mcp::ned(&st.client, &k, &input, &*p).await?;
            let items = mcp::ned_items(&out);
            mcp_results::shape_result(
                &st.store,
                "vworld_ned",
                &summary,
                &out,
                items,
                &mcp::ned_info(&out),
                &shape,
            )
        })
        .await
    }

    #[tool(
        description = "저장된 큰 결과의 다음 쪽(vworld_data · vworld_ned 응답의 next_cursor 를 그대로). 다시 조회하지 않고 API 키도 필요 없어요. 같은 형식으로 이어 주고, 마지막 쪽이면 next_cursor 가 없어요. limit 로 쪽 크기를 줄일 수 있어요. 결과는 기본 1시간 뒤 만료 — 그러면 다시 조회하세요",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn vworld_result_page(
        &self,
        Parameters(a): Parameters<PageArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let st = self.st.clone();
        self.run(&ctx, &[], move |_p| async move {
            mcp_results::result_page(&st.store, &a.cursor, a.limit.map(|l| l as usize))
        })
        .await
    }
}

const INSTRUCTIONS: &str = "VWorld(국토교통부 공간정보 오픈플랫폼) 읽기 전용 도구. 주소→좌표 vworld_geocode, 좌표→주소 vworld_reverse_geocode, 검색 vworld_search, 용도지역 vworld_zoning, 2D 데이터 vworld_data_layers→vworld_data_describe→vworld_data, 국가중점데이터 vworld_ned_list→vworld_ned. 큰 결과는 properties·filter 로 줄이고, 넘치면 next_cursor 로 vworld_result_page, 전체는 resource_link(resources/read) 또는 download. 지도 이미지는 없어요.";

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("vworld-mcp", VERSION))
        .with_instructions(INSTRUCTIONS.to_string())
    }

    /// 저장된 결과 파일(`vworld://results/<id>.csv|geojson`). 20MB 넘으면 내려받기를 쓰라고 오류.
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let store = self.st.store.clone();
        let uri = request.uri.clone();
        let read = tokio::task::spawn_blocking(move || mcp_results::read_resource(&store, &uri))
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let (mime, text) = read.map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let res: ReadResourceResult = serde_json::from_value(
            json!({"contents": [{"uri": request.uri, "mimeType": mime, "text": text}]}),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(res.into())
    }
}

// ───────────────────────── 시험 전용 도구 ─────────────────────────

#[cfg(test)]
pub(crate) mod test_tools {
    use std::collections::HashSet;
    use std::sync::Mutex;

    static ABORTED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    pub(crate) fn mark(tag: &str) {
        ABORTED
            .lock()
            .unwrap()
            .get_or_insert_with(HashSet::new)
            .insert(tag.to_string());
    }

    pub(crate) fn aborted(tag: &str) -> bool {
        ABORTED
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| s.contains(tag))
    }

    /// 작업 future 가 끝나기 전에 버려지면(취소) 표시를 남긴다.
    pub(crate) struct AbortFlag(pub String, pub bool);

    impl Drop for AbortFlag {
        fn drop(&mut self) {
            if !self.1 {
                mark(&self.0);
            }
        }
    }
}

#[cfg(test)]
#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct TagArgs {
    tag: String,
}

#[cfg(test)]
#[derive(Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct SleepArgs {
    sleep_ms: Option<u64>,
}

#[cfg(test)]
#[tool_router(router = test_router)]
impl Server {
    #[tool(description = "시험용: 진행 알림 뒤 30초 잠")]
    async fn test_sleep(
        &self,
        Parameters(a): Parameters<TagArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        self.run(&ctx, &[], |rep| async move {
            let mut flag = test_tools::AbortFlag(a.tag, false);
            rep(0, 2, "시작");
            tokio::time::sleep(Duration::from_secs(30)).await;
            flag.1 = true;
            Ok(serde_json::json!({"slept": true}))
        })
        .await
    }

    #[tool(description = "시험용: 이번 요청에 쓰일 키·도메인을 그대로 돌려줌(키는 스크럽됨)")]
    async fn test_keys(
        &self,
        Parameters(a): Parameters<SleepArgs>,
        ctx: RequestContext<RoleServer>,
    ) -> CallToolResult {
        self.keyed(&ctx, |_st, k, _p| async move {
            tokio::time::sleep(Duration::from_millis(a.sleep_ms.unwrap_or(0))).await;
            Ok(serde_json::json!({
                "keys": k.iter().map(|a| a.key.clone()).collect::<Vec<_>>(),
                "domains": k.iter().map(|a| a.domain.clone()).collect::<Vec<_>>(),
            }))
        })
        .await
    }
}

#[cfg(test)]
impl Server {
    pub(crate) fn with_test_tools(mut self) -> Self {
        self.tool_router += Self::test_router();
        self
    }
}

// ───────────────────────── 실행 ─────────────────────────

const HELP: &str = "vworld-mcp — VWorld OpenAPI 읽기 전용 MCP 서버 (stdio · Streamable HTTP)

사용: vworld-mcp                 stdin/stdout 으로 MCP JSON-RPC
      vworld-mcp --http [주소]   Streamable HTTP(경로 /mcp). 기본 127.0.0.1:8080
      vworld-mcp --version
      vworld-mcp --help

환경변수:
  VWORLD_API_KEYS              기본 키(쉼표로 여러 개). 없으면 ~/.vworld/config.toml(vworld config add-key)
  VWORLD_DOMAIN                도메인 등록 키의 도메인(domain= 쿼리·Referer). 선택
  VWORLD_MCP_TOKEN             HTTP 모드 Bearer 토큰. loopback 이 아닌 주소에서는 필수
  VWORLD_MCP_ALLOWED_ORIGINS   HTTP 모드에서 허용할 Origin(쉼표). loopback 출처는 늘 허용
요청별 키: tools/call 의 params._meta[\"kr.pes/keys\"] = [\"KEY\", ...], HTTP 는 X-Api-Keys: k1,k2 머리글도 됨";

/// stdio 전송 감싸개 — 취소된 요청의 응답은 내보내지 않는다(규격).
struct CancelFilter<T>(T);

impl<T> rmcp::transport::Transport<RoleServer> for CancelFilter<T>
where
    T: rmcp::transport::Transport<RoleServer>,
{
    type Error = T::Error;

    fn send(
        &mut self,
        item: rmcp::service::TxJsonRpcMessage<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send + 'static {
        let id: Option<&RequestId> = match &item {
            rmcp::model::JsonRpcMessage::Response(r) => Some(&r.id),
            rmcp::model::JsonRpcMessage::Error(e) => e.id.as_ref(),
            _ => None,
        };
        let drop_it = id.is_some_and(|id| {
            take_cancelled(&cancel_key(
                "",
                &serde_json::to_string(id).unwrap_or_default(),
            ))
        });
        let fut = (!drop_it).then(|| self.0.send(item));
        async move {
            match fut {
                Some(f) => f.await,
                None => Ok(()),
            }
        }
    }

    fn receive(
        &mut self,
    ) -> impl std::future::Future<Output = Option<rmcp::service::RxJsonRpcMessage<RoleServer>>> + Send
    {
        self.0.receive()
    }

    fn close(&mut self) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        self.0.close()
    }
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut http_addr: Option<String> = None;
    match args.first().map(String::as_str) {
        None => {}
        Some("--version" | "-V") => {
            println!("vworld-mcp {VERSION}");
            return std::process::ExitCode::SUCCESS;
        }
        Some("--help" | "-h") => {
            println!("{HELP}");
            return std::process::ExitCode::SUCCESS;
        }
        Some("--http") if args.len() <= 2 => {
            http_addr = Some(
                args.get(1)
                    .cloned()
                    .unwrap_or_else(|| "127.0.0.1:8080".into()),
            );
        }
        Some(other) => {
            eprintln!("알 수 없는 인자: {other}\n\n{HELP}");
            return std::process::ExitCode::from(2);
        }
    }
    let domain = mcp::env_domain();
    let (defaults, source) = mcp::load_default_auths();
    let src = match source {
        KeySource::Env => "VWORLD_API_KEYS",
        KeySource::ConfigFile => "설정 파일",
        KeySource::None => "없음(요청별 키만 사용 가능)",
    };
    eprintln!(
        "vworld-mcp {VERSION} — 기본 키 {}개 ({src})",
        defaults.len()
    );
    let server = match Server::new(defaults, domain) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("초기화 실패: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    // 결과 저장소 정리: 시작할 때와 10분마다(TTL · 용량 상한).
    let store = server.store();
    tokio::spawn(async move {
        loop {
            let s = store.clone();
            let _ = tokio::task::spawn_blocking(move || s.cleanup()).await;
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    });

    if let Some(addr) = http_addr {
        let opts = web::Options::from_env(&addr);
        return match web::serve(server, opts).await {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("HTTP 모드 시작 실패: {e}");
                std::process::ExitCode::FAILURE
            }
        };
    }

    let run = async {
        let (stdin, stdout) = rmcp::transport::stdio();
        let transport = CancelFilter(rmcp::transport::async_rw::AsyncRwTransport::new_server(
            stdin, stdout,
        ));
        server.serve(transport).await?.waiting().await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    };
    match run.await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("서버 오류: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_has_read_only_vworld_tools() {
        let tools = Server::tool_router().list_all();
        let mut names: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "vworld_data",
                "vworld_data_describe",
                "vworld_data_layers",
                "vworld_geocode",
                "vworld_ned",
                "vworld_ned_list",
                "vworld_result_page",
                "vworld_reverse_geocode",
                "vworld_search",
                "vworld_zoning",
            ]
        );
        for t in &tools {
            assert!(
                t.description.as_deref().is_some_and(|d| !d.is_empty()),
                "{} description",
                t.name
            );
            let ann = t.annotations.as_ref().expect("annotations");
            assert_eq!(ann.read_only_hint, Some(true), "{} read-only", t.name);
            // 요청별 키는 _meta 로만 받는다 — 인자 스키마에 키 항목이 없어야 한다.
            let schema = serde_json::to_string(&t.input_schema).unwrap();
            assert!(
                !schema.contains("api_key") && !schema.contains("\"key\""),
                "{} schema",
                t.name
            );
        }
        let geocode = tools.iter().find(|t| t.name == "vworld_geocode").unwrap();
        let schema = serde_json::to_value(&geocode.input_schema).unwrap();
        assert_eq!(schema["required"], serde_json::json!(["address"]));
        assert!(schema["properties"]["crs"]["pattern"].is_string());
    }

    #[test]
    fn finish_scrubs_default_and_request_keys() {
        let def = Auth {
            key: "DEFAULTKEY-0000-1111-AAAA".into(),
            domain: None,
        };
        let req = Auth {
            key: "REQUESTKEY-2222-3333-BBBB".into(),
            domain: None,
        };
        let s = Server::new(vec![def.clone()], None).unwrap();
        let r = s.finish(
            std::slice::from_ref(&req),
            Err::<Value, _>(anyhow::anyhow!(
                "HTTP 401: https://api.vworld.kr/req?key={}&k2={}",
                req.key,
                def.key
            )),
        );
        assert_eq!(r.is_error, Some(true));
        let text = serde_json::to_string(&r.content).unwrap();
        assert!(
            !text.contains(&req.key) && !text.contains(&def.key),
            "{text}"
        );
        assert!(text.contains("***BBBB") && text.contains("***AAAA"));
    }

    #[test]
    fn cancelled_marks_are_taken_once() {
        mark_cancelled(cancel_key("s1", "7"));
        assert!(
            !take_cancelled(&cancel_key("s2", "7")),
            "다른 세션의 같은 id 는 무관"
        );
        assert!(take_cancelled(&cancel_key("s1", "7")));
        assert!(!take_cancelled(&cancel_key("s1", "7")));
    }
}
