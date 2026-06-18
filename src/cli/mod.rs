//! CLI 진입 계층 — clap 서브커맨드 정의·별칭(kosis식 단축)·라우팅.
//!
//! 전역 플래그: `--concurrency`, `--pretty`, `--raw`, `--timing`, `--referer`, `--config`.

pub mod config_cmds;
pub mod data_cmds;
pub mod embed_cmds;
pub mod image_cmds;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// VWorld OpenAPI CLI.
#[derive(Parser, Debug)]
#[command(name = "vworld", version, about = "VWorld OpenAPI CLI (단일 바이너리)")]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Commands,
}

/// 모든 명령이 공유하는 전역 인자.
#[derive(Args, Debug, Clone)]
pub struct GlobalArgs {
    /// 동시 in-flight 요청 상한(워커 수). 미지정 시 max(키수, 2).
    #[arg(long, global = true)]
    pub concurrency: Option<usize>,

    /// JSON 들여쓰기 출력.
    #[arg(long, global = true)]
    pub pretty: bool,

    /// 원응답 그대로 출력(분류별 의미 — 설계 §5.3).
    #[arg(long, global = true)]
    pub raw: bool,

    /// 수행 시간 측정 정보 출력.
    #[arg(long, global = true)]
    pub timing: bool,

    /// 도메인 등록 키용 Referer/domain 일회성 오버라이드.
    #[arg(long, global = true)]
    pub referer: Option<String>,

    /// 설정파일 경로 오버라이드(dev/test). 미지정 시 current_exe 기준 app/config.toml.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 지오코딩/역지오코딩 (/req/address)
    #[command(alias = "geo")]
    Geocode(data_cmds::GeocodeArgs),

    /// 통합 지오코더 (apis.vworld.kr) — 주소/좌표 → 좌표·지번·도로명 한 번에
    Geocoder(data_cmds::GeocoderArgs),

    /// 검색 (/req/search)
    #[command(alias = "s")]
    Search(data_cmds::SearchArgs),

    /// 2D데이터 레지스트리·조회 (layers/describe/fetch, 158 데이터셋)
    /// `data <데이터ID> [옵션]` 위치인자 조회와 서브커맨드가 공존한다.
    Data(data_cmds::DataTopArgs),

    /// 국가중점데이터 (NED, /ned/{wms|wfs|data}, 115 오퍼레이션)
    Ned(data_cmds::NedArgs),

    /// WMS (/req/wms) — GetMap/GetCapabilities
    Wms(data_cmds::WmsArgs),

    /// WFS (/req/wfs) — GetFeature/GetCapabilities
    Wfs(data_cmds::WfsArgs),

    /// 다운로드 카탈로그 (/ned/dtmk/*)
    Catalog(data_cmds::CatalogArgs),

    /// StaticMap 이미지 (/req/image, GetMap)
    #[command(name = "staticmap", alias = "static")]
    StaticMap(image_cmds::StaticMapArgs),

    /// 범례이미지 (/req/image, GetLegendGraphic/Style)
    Legend(image_cmds::LegendArgs),

    /// 타일 (WMTS/TMS/벡터 통합)
    Tile(image_cmds::TileArgs),

    /// 지도 임베드 생성 (2D/3D/3D분석 — URL/HTML/설정)
    Map(embed_cmds::MapArgs),

    /// 다건 배치 실행 (`vworld batch <명령> --from <file>`)
    Batch(data_cmds::BatchArgs),

    /// 행정동 경계 SHP → SQLite 적재 (`vworld hjd-db build --shp ... --db ...`)
    #[command(name = "hjd-db", subcommand)]
    HjdDb(data_cmds::HjdDbCmd),

    /// 설정·키 관리
    #[command(subcommand)]
    Config(config_cmds::ConfigCmd),
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
    }
}
