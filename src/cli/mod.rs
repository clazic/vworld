//! CLI 진입 계층 — clap 서브커맨드 정의·별칭(kosis식 단축)·라우팅.
//!
//! 전역 플래그: `--concurrency`, `--pretty`, `--raw`, `--timing`, `--referer`, `--config`.

pub mod choropleth;
pub mod config_cmds;
pub mod data_cmds;
pub mod embed_cmds;
pub mod image_cmds;
pub mod update_cmds;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// VWorld OpenAPI CLI.
#[derive(Parser, Debug)]
#[command(
    name = "vworld",
    version,
    about = "VWorld OpenAPI CLI (단일 바이너리)",
    long_about = "VWorld(국토교통부 공간정보 오픈플랫폼) OpenAPI를 감싼 자기완결 단일 바이너리 CLI.\n\
지오코딩·검색·2D데이터 158종·국가중점데이터(NED) 115 오퍼레이션·WMS/WFS·정적 지도 이미지·타일·지도 임베드 HTML 생성까지 한 바이너리로 처리한다.\n\
\n\
출력 규약: 데이터형 명령(geocode/search/data/ned/wfs 등)은 결과를 JSON으로 stdout에 출력한다.\n\
이미지·HTML형 명령(staticmap/legend/tile/map)은 파일로 저장하고, 저장 경로를 JSON으로 stdout에 보고한다.\n\
config/update/hjd-db 는 오프라인 명령으로 인증키가 필요 없다. 그 외 대부분의 명령은 VWorld 인증키가 필요하다(`vworld config add-key`로 등록).\n\
\n\
등록된 모든 키는 동시성 키 풀에 자동 편입되어 배치·전수 수집 시 여러 키로 요청이 분산·병렬 처리된다(`--concurrency`로 워커 수 조절).\n\
\n\
대표 사용 흐름:\n\
  vworld config add-key <KEY> --alias main   # 1) 키 등록(최초 1회)\n\
  vworld geocode \"세종대로 110\"              # 2) 주소 → 좌표\n\
  vworld map choropleth --geojson j.geojson --value-field 인구 --legend -o map.html  # 3) 결과를 지도로 시각화"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Commands,
}

/// 모든 명령이 공유하는 전역 인자.
#[derive(Args, Debug, Clone)]
pub struct GlobalArgs {
    /// 동시 in-flight 요청 상한(워커 수).
    ///
    /// 배치 조회(`--input`)나 `ned --all`/`--by-hjd` 같은 전수 수집에서
    /// 등록된 키 풀 전체를 몇 개의 워커로 동시에 사용할지 결정한다.
    /// 미지정 시 max(등록 키 수, 2) — 키가 많을수록 자동으로 병렬도가 올라간다.
    /// 429(요청 과다) 응답이 잦다면 낮추고, 대량 수집을 빨리 끝내고 싶다면 키를 늘리고 올린다.
    #[arg(long, global = true)]
    pub concurrency: Option<usize>,

    /// JSON 들여쓰기 출력.
    ///
    /// 기본은 압축된 한 줄 JSON(스크립트·파이프 연계에 적합). 사람이 눈으로 확인할 때
    /// `--pretty`를 켜면 들여쓰기된 JSON으로 출력한다. stdout 결과의 가독성에만 영향을 주며
    /// 값 자체는 바뀌지 않는다.
    #[arg(long, global = true)]
    pub pretty: bool,

    /// 원응답 그대로 출력(정규화·가공 없음).
    ///
    /// 기본은 VWorld 원응답을 CLI가 정규화해 일관된 JSON 스키마로 반환한다.
    /// `--raw`는 이 정규화를 건너뛰고 서버가 준 응답을 그대로 내보낸다.
    /// 서버 응답에 제어문자 등으로 정규화 파싱이 실패하는 경우(예: `catalog gid-datasets`)
    /// 우회 수단으로 사용한다.
    #[arg(long, global = true)]
    pub raw: bool,

    /// 수행 시간 측정 정보 출력.
    ///
    /// 요청~응답까지 걸린 시간을 결과 JSON에 함께 포함한다. 성능 튜닝이나
    /// `--concurrency` 조정 시 개선 효과를 확인하는 용도.
    #[arg(long, global = true)]
    pub timing: bool,

    /// 도메인 등록 키용 Referer/domain 일회성 오버라이드.
    ///
    /// 발급받은 키가 특정 도메인에 등록된 키(도메인 등록 키)일 경우, 그 도메인을
    /// 여기에 지정해야 서버가 요청을 허용한다. config.toml에 키별 referer를 저장해두면
    /// 보통 생략 가능하며, 이 옵션은 해당 명령 1회에 한해 설정값을 덮어쓴다.
    /// 무도메인(서버용) 키는 지정할 필요 없다. `config test-keys`에서 도메인불일치 판정 시
    /// 이 옵션 사용을 안내한다.
    #[arg(long, global = true)]
    pub referer: Option<String>,

    /// 설정파일 경로 오버라이드.
    ///
    /// 미지정 시 기본 경로 `~/.vworld/config.toml`(Windows: `%USERPROFILE%\.vworld\config.toml`)을
    /// 사용한다. 여러 계정/키 세트를 분리 운용하거나 CI에서 격리된 설정 파일을 쓸 때 지정한다.
    /// 조회 계열 명령은 지정한 경로가 없으면 에러를 낸다(`config add-key`는 예외 — 없으면 새로 만든다).
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 지오코딩/역지오코딩 (/req/address)
    ///
    /// 주소를 좌표로, 또는 좌표를 주소로 변환한다(입력 형태로 자동 감지). 인증키 필요.
    /// `--type auto`(기본)는 도로명→지번을 자동 판별·폴백하므로 유형을 모르면 그대로 둔다.
    ///
    /// 예) `vworld geocode "세종대로 110"` / `vworld geocode "126.978,37.566"`(역지오)
    #[command(alias = "geo")]
    Geocode(data_cmds::GeocodeArgs),

    /// 통합 지오코더 (apis.vworld.kr) — 주소/좌표 → 좌표·지번·도로명 한 번에
    ///
    /// geocode와 달리 좌표·지번·도로명 세 가지 표현을 한 번의 호출로 모두 받는다.
    /// 주소 또는 좌표("x,y")를 자동 감지해 처리한다. 인증키 필요.
    ///
    /// 예) `vworld geocoder "경상남도 고성군 하이면 덕명리 420-1"`
    Geocoder(data_cmds::GeocoderArgs),

    /// 검색 (/req/search)
    ///
    /// 장소·주소·행정구역·도로명을 검색한다. 인증키 필요.
    /// `--type ADDRESS`는 `--category ROAD|PARCEL`, `--type DISTRICT`는 `--category L1~L4`가
    /// 필수다(없으면 PARAM_REQUIRED 에러). PLACE/ROAD는 카테고리 생략 가능.
    ///
    /// 예) `vworld search "광화문" --type PLACE` / `vworld search "종로구" --type DISTRICT --category L2`
    #[command(alias = "s")]
    Search(data_cmds::SearchArgs),

    /// 2D데이터 레지스트리·조회 (layers/describe/fetch, 158 데이터셋)
    ///
    /// `data <데이터ID> [옵션]` 위치인자 조회와 서브커맨드(`layers`/`describe`/`fetch`/`join`)가
    /// 공존한다. `layers`/`describe`/`join`은 오프라인(키 불요), 실제 데이터 조회(`fetch`,
    /// 또는 위치인자 생략형)만 인증키가 필요하다.
    ///
    /// 예) `vworld data layers --search "지적"` / `vworld data LP_PA_CBND_BUBUN --geom-filter "POINT(126.978 37.566)"`
    Data(data_cmds::DataTopArgs),

    /// 국가중점데이터 (NED, /ned/{wms|wfs|data}, 115 오퍼레이션)
    ///
    /// 건물연령·개별공시지가 등 115종 국가중점데이터 오퍼레이션을 조회한다. 인증키 필요.
    /// `--all`(법정동 전수 수집), `--by-hjd`(행정동별 집계) 등 대량 수집 옵션을 지원하며
    /// WMS 계열(오퍼레이션명에 WMS 포함)은 이미지, WFS/속성 계열은 데이터를 반환한다.
    ///
    /// 예) `vworld ned --list` / `vworld ned getBuildingAge --pnu 1111018300101970001`
    Ned(data_cmds::NedArgs),

    /// WMS (/req/wms) — GetMap/GetCapabilities
    ///
    /// OGC WMS 표준 요청으로 지도 이미지(GetMap) 또는 능력문서(GetCapabilities)를 받는다.
    /// 인증키 필요. GetMap은 `-o` 저장 경로가 필수. EPSG:4326/5185~5188 bbox는
    /// `(ymin,xmin,ymax,xmax)`(위도 먼저) 순서에 주의.
    ///
    /// 예) `vworld wms --request GetMap --layers lt_c_uq111 --bbox 37.5,126.9,37.6,127.1 -o uq.png`
    Wms(data_cmds::WmsArgs),

    /// WFS (/req/wfs) — GetFeature/GetCapabilities
    ///
    /// OGC WFS 표준 요청으로 벡터 피처(GeoJSON/JSON)를 조회한다. 인증키 필요.
    /// `-o` 지정 시 결과를 stdout이 아닌 토스 디자인 HTML 뷰어로 저장한다.
    ///
    /// 예) `vworld wfs --request GetFeature --typename lp_pa_cbnd_bubun --bbox 37.55,126.97,37.57,126.99`
    Wfs(data_cmds::WfsArgs),

    /// 다운로드 카탈로그 (/ned/dtmk/*)
    ///
    /// NED 원본 데이터셋 다운로드 카탈로그(분류·데이터셋 목록)를 조회한다. 인증키 필요.
    /// 일부 분류(`gid-cd 02`/`03`)는 서버 응답의 제어문자로 JSON 파싱이 실패할 수 있으므로
    /// 그 경우 전역 `--raw`로 우회한다.
    ///
    /// 예) `vworld catalog datasets --gid-cd 01 --num-rows 100`
    Catalog(data_cmds::CatalogArgs),

    /// StaticMap 이미지 (/req/image, GetMap)
    ///
    /// 중심 좌표·줌 레벨 기준의 정적 지도 이미지를 파일로 저장한다. 인증키 필요.
    /// `CENTER`("x,y")와 `--zoom`(6~18)이 필수이며, 저장 경로는 JSON으로 보고된다.
    ///
    /// 예) `vworld staticmap "127.0,37.5" --zoom 14 --size 512,512 -o map.png`
    #[command(name = "staticmap", alias = "static")]
    StaticMap(image_cmds::StaticMapArgs),

    /// 범례이미지 (/req/image, GetLegendGraphic/Style)
    ///
    /// 레이어의 범례 이미지(png) 또는 SLD 스타일 정의(XML, `--sld`)를 저장한다. 인증키 필요.
    /// `--style`을 지정하지 않으면 대부분 547B "결과없음"이 반환되므로 사실상 필수이며,
    /// 보통 레이어명과 동일한 값을 쓴다.
    ///
    /// 예) `vworld legend lt_c_uq111 --style lt_c_uq111 -o legend.png`
    Legend(image_cmds::LegendArgs),

    /// 타일 (WMTS/TMS/벡터 통합)
    ///
    /// WMTS/TMS 래스터 타일과 벡터(MVT) 타일을 한 명령으로 다룬다. 인증키 필요.
    /// wmts/tms는 `--row`=Y·`--col`=X, **vector는 반대(`--row`=X·`--col`=Y)** — 실측된 함정.
    ///
    /// 예) `vworld tile wmts --layer Base --z 14 --row 6449 --col 13969 -o tile.png`
    Tile(image_cmds::TileArgs),

    /// 지도 임베드 생성 (2D/3D/3D분석 — URL/HTML/설정)
    ///
    /// CLI는 렌더링하지 않고 HTML/URL/설정만 생성한다.
    /// KIND별 서브모드: 2d, 3d, 3dsim, ol, marker, chart, theme, text, controller, choropleth, 3d-extrude (자세한 설명은 `vworld map --help`). 인증키 필요.
    /// `-o` 미지정 시 URL·스니펫을 JSON으로 보고. 산출물은 모두 토스 디자인 + 절대경로(https://) 적용.
    ///
    /// 예) `vworld map choropleth --geojson j.geojson --value-field 인구 --legend -o map.html`
    Map(embed_cmds::MapArgs),

    /// 다건 배치 실행 (`vworld batch <명령> --from <file>`)
    ///
    /// 파일에 줄 단위로 나열된 입력을 배치 처리한다(현재 geocode 전용 진입점). 인증키 필요.
    /// 키 풀 전체를 활용해 `--concurrency`로 병렬 처리된다.
    ///
    /// 예) `vworld batch geocode --from addrs.txt --concurrency 4`
    Batch(data_cmds::BatchArgs),

    /// 행정동 경계 SHP → SQLite 적재 (`vworld hjd-db build --shp ... --db ...`)
    ///
    /// `ned --by-hjd`용 행정동 경계 SQLite를 구축·조회하는 오프라인 명령(키 불요).
    /// `build`(SHP→SQLite 적재)/`region`(지역코드 xlsx 적재)/`info`(요약)/`lookup`(조회)
    /// 서브커맨드를 제공한다.
    ///
    /// 예) `vworld hjd-db build --shp BND_ADM_DONG_PG.shp --db hjd.sqlite`
    #[command(name = "hjd-db", subcommand)]
    HjdDb(data_cmds::HjdDbCmd),

    /// 설정·키 관리
    ///
    /// VWorld 인증키 등록·목록·제거·검증과 설정파일 경로 확인을 담당하는 오프라인 명령(키 불요,
    /// 단 test-keys는 실 호출을 수행). 등록된 모든 키는 동시성 키 풀에 자동 편입된다.
    ///
    /// 예) `vworld config add-key <KEY> --alias main` / `vworld config test-keys`
    #[command(subcommand)]
    Config(config_cmds::ConfigCmd),

    /// 자가 업데이트 (GitHub Releases)
    ///
    /// GitHub Releases에서 최신(또는 지정) 버전을 받아 실행 중인 바이너리를 교체하는
    /// 오프라인 명령(키 불요). `--check`로 확인만, `--yes`로 확인 프롬프트 생략 가능.
    ///
    /// 예) `vworld update --check` / `vworld update --yes`
    Update(update_cmds::UpdateArgs),
}

/// 최상위 라우터.
pub async fn run(cli: Cli) -> Result<()> {
    let g = &cli.global;
    match cli.command {
        Commands::Geocode(a) => data_cmds::run_geocode(g, a).await,
        Commands::Geocoder(a) => data_cmds::run_geocoder(g, a).await,
        Commands::Search(a) => data_cmds::run_search(g, a).await,
        Commands::Data(top) => match top.sub {
            Some(data_cmds::DataSub::Layers(a)) => data_cmds::run_data_layers(g, a),
            Some(data_cmds::DataSub::Describe(a)) => data_cmds::run_data_describe(g, a),
            Some(data_cmds::DataSub::Fetch(a)) => data_cmds::run_data(g, a).await,
            Some(data_cmds::DataSub::Join(a)) => data_cmds::run_data_join(g, a),
            None => data_cmds::run_data(g, top.fetch).await,
        },
        Commands::Ned(a) => data_cmds::run_ned(g, a).await,
        Commands::Wms(a) => data_cmds::run_wms(g, a).await,
        Commands::Wfs(a) => data_cmds::run_wfs(g, a).await,
        Commands::Catalog(a) => data_cmds::run_catalog(g, a).await,
        Commands::StaticMap(a) => image_cmds::run_staticmap(g, a).await,
        Commands::Legend(a) => image_cmds::run_legend(g, a).await,
        Commands::Tile(a) => image_cmds::run_tile(g, a).await,
        Commands::Map(a) => embed_cmds::run_map(g, a).await,
        Commands::Batch(a) => data_cmds::run_batch(g, a).await,
        Commands::HjdDb(c) => data_cmds::run_hjd_db(g, c).await,
        Commands::Config(c) => config_cmds::run(g, c).await,
        Commands::Update(a) => update_cmds::run_update(a).await,
    }
}
