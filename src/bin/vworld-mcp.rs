//! vworld-mcp — VWorld OpenAPI 를 읽기 전용 도구로 내놓는 stdio MCP 서버.
//!
//! 공통 규약(`<name>-mcp`): stdout 에는 JSON-RPC 만, 진단은 stderr(키 없이).
//! 로직은 라이브러리 `vworld::mcp`(키 선택·스크럽·검증·도구 실행)에 있고, 여기는 rmcp 연결만 한다.

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router, RoleServer, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::sync::Arc;
use vworld::api::{Auth, Client};
use vworld::mcp::{self, DataInput, KeySource, NedInput, SearchInput, META_KEYS};

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
    /// 응답 좌표계. 기본 EPSG:4326
    #[schemars(pattern(r"^EPSG:[0-9]{4,6}$"))]
    crs: Option<String>,
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
}

// ───────────────────────── 서버 ─────────────────────────

struct State {
    client: Client,
    defaults: Vec<Auth>,
    domain: Option<String>,
}

#[derive(Clone)]
struct Server {
    st: Arc<State>,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Server>,
}

impl Server {
    fn new(defaults: Vec<Auth>, domain: Option<String>) -> anyhow::Result<Self> {
        Ok(Server {
            st: Arc::new(State {
                client: Client::new()?,
                defaults,
                domain,
            }),
            tool_router: Self::tool_router(),
        })
    }

    /// 이번 요청의 키(요청별 `_meta` 우선, 없으면 기본 키).
    fn keys(&self, ctx: &RequestContext<RoleServer>) -> Result<Vec<Auth>, String> {
        mcp::select_auths(
            ctx.meta.get(META_KEYS),
            &self.st.defaults,
            self.st.domain.as_deref(),
        )
        .map(|(a, _)| a)
    }

    /// 결과를 텍스트로 만들고, 요청 키·기본 키를 지운 뒤 크기를 제한한다.
    fn finish(&self, used: &[Auth], r: anyhow::Result<Value>) -> CallToolResult {
        let secrets: Vec<&str> = used
            .iter()
            .chain(self.st.defaults.iter())
            .map(|a| a.key.as_str())
            .collect();
        let (text, ok) = match r {
            Ok(v) => (
                serde_json::to_string(&v).unwrap_or_else(|e| format!("직렬화 실패: {e}")),
                true,
            ),
            Err(e) => (format!("{e:#}"), false),
        };
        let text = mcp::cap_output(mcp::scrub(&text, &secrets));
        if ok {
            CallToolResult::success(vec![ContentBlock::text(text)])
        } else {
            CallToolResult::error(vec![ContentBlock::text(text)])
        }
    }

    /// 키가 필요한 도구 공통: 키 선택 → 실행 → 마무리.
    async fn keyed<F, Fut>(&self, ctx: &RequestContext<RoleServer>, f: F) -> CallToolResult
    where
        F: FnOnce(Arc<State>, Vec<Auth>) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<Value>>,
    {
        let auths = match self.keys(ctx) {
            Ok(a) => a,
            Err(e) => return self.finish(&[], Err(anyhow::anyhow!(e))),
        };
        let r = tokio::select! {
            r = f(self.st.clone(), auths.clone()) => r,
            _ = ctx.ct.cancelled() => Err(anyhow::anyhow!("요청이 취소됐어요")),
        };
        self.finish(&auths, r)
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
        self.keyed(&ctx, |st, k| async move {
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
        self.keyed(&ctx, |st, k| async move {
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
        self.keyed(&ctx, |st, k| async move {
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
        description = "2D 데이터 조회(/req/data GetFeature) — 레이어 + 공간 필터(geom_filter) 또는 속성 필터(attr_filter). 둘 다 없으면 emd_cd 필요. 함정: geom_filter 에 EPSG 접미사 금지(좌표는 EPSG:4326 해석), BOX/POLYGON 은 면적 10km² 이내(넘으면 INVALID_RANGE — 나눠서 조회). size 기본 100·상한 1000. 좌표→필지(PNU): data_id=LP_PA_CBND_BUBUN, geom_filter=\"POINT(경도 위도)\"",
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
            crs: crs_or_4326(a.crs),
        };
        self.keyed(&ctx, |st, k| async move {
            mcp::data(&st.client, &k, &input).await
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
        self.keyed(&ctx, |st, k| async move {
            mcp::zoning(&st.client, &k, a.address.as_deref(), a.x, a.y).await
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
        description = "국가중점데이터(NED) data·WFS 단건 조회(/ned/data|wfs/<operation>). 먼저 vworld_ned_list 로 오퍼레이션명·필수 변수를 확인하세요. 필지 속성은 pnu(19자리, 주소면 vworld_zoning 이나 vworld_data 로 먼저 얻기). INVALID_TYPE 이면 pnu 를 8자리 이상으로. WFS 는 기본 EPSG:4326(srsName), 건수는 params.maxFeatures(최대 1000), data 계열은 params.numOfRows·pageNo. WMS(이미지) 계열은 거부",
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
        self.keyed(&ctx, |st, k| async move {
            mcp::ned(&st.client, &k, &input).await
        })
        .await
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "vworld-mcp",
    instructions = "VWorld(국토교통부 공간정보 오픈플랫폼) 읽기 전용 도구. 주소→좌표 vworld_geocode, 좌표→주소 vworld_reverse_geocode, 검색 vworld_search, 용도지역 vworld_zoning, 2D 데이터 vworld_data_layers→vworld_data_describe→vworld_data, 국가중점데이터 vworld_ned_list→vworld_ned. 지도 이미지·파일 내보내기는 없어요."
)]
impl ServerHandler for Server {}

// ───────────────────────── 실행 ─────────────────────────

const HELP: &str = "vworld-mcp — VWorld OpenAPI 읽기 전용 MCP(stdio) 서버

사용: vworld-mcp            (stdin/stdout 으로 MCP JSON-RPC)
      vworld-mcp --version
      vworld-mcp --help

환경변수:
  VWORLD_API_KEYS   기본 키(쉼표로 여러 개). 없으면 ~/.vworld/config.toml(vworld config add-key)
  VWORLD_DOMAIN     도메인 등록 키의 도메인(domain= 쿼리·Referer). 선택
요청별 키: tools/call 의 params._meta[\"kr.pes/keys\"] = [\"KEY\", ...] (있으면 그 키만 사용)";

#[tokio::main]
async fn main() -> std::process::ExitCode {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("vworld-mcp {VERSION}");
                return std::process::ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                println!("{HELP}");
                return std::process::ExitCode::SUCCESS;
            }
            other => {
                eprintln!("알 수 없는 인자: {other}\n\n{HELP}");
                return std::process::ExitCode::from(2);
            }
        }
    }
    let domain = mcp::env_domain();
    let (defaults, source) = mcp::load_default_auths();
    let src = match source {
        KeySource::Env => "VWORLD_API_KEYS",
        KeySource::ConfigFile => "설정 파일",
        KeySource::None => "없음(요청별 _meta 키만 사용 가능)",
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
    let run = async {
        server
            .serve(rmcp::transport::stdio())
            .await?
            .waiting()
            .await?;
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
            Err(anyhow::anyhow!(
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
}
