//! 이미지/파일 저장형 명령 핸들러(설계 §1.3·§2, §5.3 이미지형).

use super::GlobalArgs;
use super::data_cmds::load_auths;
use crate::api::{wmts_row_to_tms, Client, PathBuilder, QueryBuilder};
use crate::output;
use anyhow::{anyhow, Result};
use clap::Args;

/// 이미지 바이트를 파일 저장(기본) 또는 --raw 시 stdout.
async fn fetch_image(
    g: &GlobalArgs,
    client: &Client,
    url: String,
    params: Vec<(String, String)>,
    auth: &crate::api::Auth,
    out: &std::path::Path,
) -> Result<()> {
    let bytes = client.get_bytes(&url, params, auth).await?;
    if g.raw {
        return output::print_raw_bytes(&bytes);
    }
    let saved = output::save_bytes(out, &bytes)?;
    output::print_json(g, &serde_json::json!({"ok": true, "saved": saved}))
}

// ───────────────────────── StaticMap ─────────────────────────

#[derive(Args, Debug)]
pub struct StaticMapArgs {
    /// 중심 좌표(필수 위치 인자).
    /// 형식: "x,y" — crs가 EPSG:4326(기본)이면 x=경도, y=위도.
    /// 예: "127.0,37.5"(서울시청 부근).
    pub center: String,
    /// 줌 레벨(필수).
    /// 허용 범위 6~18 — 벗어나면 실행 전 에러로 즉시 거부.
    /// 예: --zoom 14.
    #[arg(long)]
    pub zoom: u32,
    /// 이미지 크기 "width,height".
    /// 기본 "512,512", 최대 "1024,1024"(초과 시 서버에서 거부될 수 있음).
    /// 예: --size 800,600.
    #[arg(long, default_value = "512,512")]
    pub size: String,
    /// 지도 유형(배경지도).
    /// 허용값: NONE / GRAPHIC(기본) / GRAPHIC_WHITE / SATELLITE / HYBRID.
    /// 예: --basemap SATELLITE.
    #[arg(long, default_value = "GRAPHIC")]
    pub basemap: String,
    /// 이미지 포맷.
    /// 기본 "png". 서버 지원 포맷(png/jpg 등)에 맞춰 지정.
    #[arg(long, default_value = "png")]
    pub format: String,
    /// 좌표계(CRS).
    /// 기본 "EPSG:4326"(경도,위도 순 입력). 다른 좌표계 사용 시 center 값도 해당 좌표계 단위로 맞출 것.
    /// 예: --crs EPSG:4326.
    #[arg(long, default_value = "EPSG:4326")]
    pub crs: String,
    /// 저장 경로.
    /// 기본 "staticmap.png". `-o`/`--output`으로 지정. `--raw`(전역 옵션) 지정 시 파일 저장 대신 stdout으로 바이트 출력.
    #[arg(long, short, default_value = "staticmap.png")]
    pub output: std::path::PathBuf,
}

pub async fn run_staticmap(g: &GlobalArgs, a: StaticMapArgs) -> Result<()> {
    if !(6..=18).contains(&a.zoom) {
        return Err(anyhow!("zoom은 6~18 범위여야 합니다(현재 {})", a.zoom));
    }
    let auths = load_auths(g)?;
    let client = Client::new()?;
    let zoom = a.zoom.to_string();
    let (url, params) = QueryBuilder::new("image", "GetMap")
        .set("center", &a.center)
        .set("zoom", &zoom)
        .set("size", &a.size)
        .set("basemap", &a.basemap)
        .set("format", &a.format)
        .set("crs", &a.crs)
        .build();
    fetch_image(g, &client, url, params, &auths[0], &a.output).await
}

// ───────────────────────── 범례 ─────────────────────────

#[derive(Args, Debug)]
pub struct LegendArgs {
    /// 대상 레이어(필수 위치 인자).
    /// 예: lt_c_uq111. --style에 보통 동일한 이름을 지정한다.
    pub layer: String,
    /// 스타일명 — 사실상 필수.
    /// 미지정 시 응답이 547바이트 "결과없음"으로 조용히 실패한다(실측 함정).
    /// 보통 layer와 동일한 값을 지정. 예: --style lt_c_uq111.
    #[arg(long)]
    pub style: Option<String>,
    /// 범례 타입.
    /// 허용값: ALL(기본) / LAYER / SUB.
    #[arg(long, default_value = "ALL")]
    pub r#type: String,
    /// 이미지 포맷.
    /// 기본 "png". --sld 지정 시에는 사용되지 않음(SLD는 XML 텍스트 응답).
    #[arg(long, default_value = "png")]
    pub format: String,
    /// SLD 스타일 정의(XML) 조회 — request=GetLegendStyle. 미지정 시 범례 이미지(GetLegendGraphic).
    /// 부울 플래그(값 없이 --sld). 저장 경로는 동일하게 -o/--output 사용.
    /// 예: vworld legend lt_c_uq111 --style lt_c_uq111 --sld -o legend.sld.xml
    #[arg(long)]
    pub sld: bool,
    /// 저장 경로.
    /// 기본 "legend.png"(--sld 지정 시 이 기본값을 유지하면 자동으로 "legend.sld.xml"로 보정됨).
    /// `--raw`(전역 옵션) 지정 시 파일 저장 대신 stdout으로 출력.
    #[arg(long, short, default_value = "legend.png")]
    pub output: std::path::PathBuf,
}

pub async fn run_legend(g: &GlobalArgs, a: LegendArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let client = Client::new()?;
    // --sld: GetLegendStyle(SLD XML) / 기본: GetLegendGraphic(범례 이미지).
    let request = if a.sld { "GetLegendStyle" } else { "GetLegendGraphic" };
    let (url, params) = QueryBuilder::new("image", request)
        .set("layer", &a.layer)
        .set("type", &a.r#type)
        .set("format", &a.format)
        .opt("style", a.style.as_deref())
        .build();
    // SLD는 XML 텍스트라 별도 경로(이미지 검증 없는 get_text) + 기본 출력명(legend.png) 사용 시 .xml로 보정.
    if a.sld {
        let body = client.get_text(&url, params, &auths[0]).await?;
        if g.raw {
            return output::print_raw_text(&body);
        }
        let out = if a.output == std::path::Path::new("legend.png") {
            std::path::PathBuf::from("legend.sld.xml")
        } else {
            a.output.clone()
        };
        let saved = output::save_bytes(&out, body.as_bytes())?;
        return output::print_json(g, &serde_json::json!({"ok": true, "saved": saved}));
    }
    fetch_image(g, &client, url, params, &auths[0], &a.output).await
}

// ───────────────────────── 타일(WMTS/TMS/벡터) ─────────────────────────

/// 타일 응답 종류(설계 §1.3-결정1 TileKind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    /// WMTS/TMS 래스터(PNG/JPEG 바이트).
    Raster,
    /// 벡터 MVT 바이너리(디코딩 Non-Goal, 패스스루 저장).
    VectorMvt,
    /// 벡터 스타일 JSON(좌표 없음).
    VectorStyle,
    /// WMTS GetCapabilities XML(좌표 없음, 메타데이터).
    Capabilities,
}

#[derive(Args, Debug)]
pub struct TileArgs {
    /// 타일 스킴(필수 위치 인자).
    /// 허용값: wmts | tms | vector | vector-style | wmts-themes | wmts-capabilities.
    /// wmts/tms=래스터 타일, vector=MVT 바이너리(또는 --ext png/jpeg 지정 시 래스터),
    /// vector-style=스타일 JSON, wmts-themes=해외위성영상 시계열, wmts-capabilities=능력문서(XML).
    pub scheme: String,
    /// 레이어명.
    /// WMTS/TMS: Base(기본)/white/midnight/Hybrid/Satellite.
    /// 벡터 MVT: poi(z>=15부터 데이터)/traffic만 가능 — Base 지정 시 InvalidParameterValue 에러(실측).
    /// Base를 벡터 계열로 쓰려면 --ext png/jpeg로 래스터 벡터 엔드포인트를 이용할 것.
    #[arg(long, default_value = "Base")]
    pub layer: String,
    /// wmts-themes 전용(필수): 테마 카테고리.
    /// 예: --category cities.
    #[arg(long)]
    pub category: Option<String>,
    /// wmts-themes 전용(필수): 영상 연도.
    /// 예: --year 2025.
    #[arg(long)]
    pub year: Option<String>,
    /// wmts-themes 전용(필수): 도시명.
    /// 예: --city Oslo.
    #[arg(long)]
    pub city: Option<String>,
    /// 줌 레벨. wmts/tms/vector/wmts-themes에서 --row --col과 함께 필수.
    /// 예: --z 14.
    #[arg(long)]
    pub z: Option<u32>,
    /// 타일 행.
    /// **wmts/tms는 Y(--row=Y, --col=X), vector는 반대로 X(--row=X, --col=Y)**(실측 함정, §1.3-결정1).
    /// tms는 CLI가 입력값(WMTS 기준 row)을 Y축 자동 반전 처리하므로 wmts와 동일한 값을 넣으면 된다.
    #[arg(long)]
    pub row: Option<u32>,
    /// 타일 열.
    /// **wmts/tms는 X(--col=X), vector는 반대로 Y(--col=Y)**(실측 함정, §1.3-결정1). row 설명 참고.
    #[arg(long)]
    pub col: Option<u32>,
    /// 타일 확장자.
    /// 기본: wmts/tms=png, vector(미지정 또는 pbf)=MVT(.pbf). vector에 png/jpeg 지정 시 래스터 벡터 엔드포인트로 전환.
    /// 예: --ext png (vector 래스터), 미지정 시 vector는 자동 pbf.
    #[arg(long)]
    pub ext: Option<String>,
    /// 저장 경로.
    /// 미지정 시 스킴별 기본 파일명 사용(tile.{ext} / tile.mvt / style.json / WMTSCapabilities.xml).
    /// `--raw`(전역 옵션) 지정 시 파일 저장 대신 stdout으로 출력.
    #[arg(long, short)]
    pub output: Option<std::path::PathBuf>,
}

pub async fn run_tile(g: &GlobalArgs, a: TileArgs) -> Result<()> {
    let auths = load_auths(g)?;
    let key = auths[0].key.clone();
    let (kind, url, resolved_ext) = build_tile_url(&a, &key)?;
    let client = Client::new()?;
    let default_name = match kind {
        TileKind::Raster => format!("tile.{resolved_ext}"),
        TileKind::VectorMvt => "tile.mvt".to_string(),
        TileKind::VectorStyle => "style.json".to_string(),
        TileKind::Capabilities => "WMTSCapabilities.xml".to_string(),
    };
    let out = a.output.clone().unwrap_or_else(|| default_name.into());

    match kind {
        TileKind::Capabilities => {
            // GetCapabilities는 XML 텍스트(좌표 없음). key 경로 포함 → 인증 미주입.
            let body = client.get_text_plain(&url).await?;
            if g.raw {
                return output::print_raw_text(&body);
            }
            let saved = output::save_bytes(&out, body.as_bytes())?;
            output::print_json(g, &serde_json::json!({"ok": true, "saved": saved}))
        }
        TileKind::VectorStyle => {
            // 스타일은 JSON 텍스트(좌표 없음) — 데이터형 취급. key는 경로 포함.
            let body = client.get_text_plain(&url).await?;
            if g.raw {
                return output::print_raw_text(&body);
            }
            let value: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::Value::String(body));
            output::print_json(g, &serde_json::json!({"ok": true, "style": value}))
        }
        _ => {
            // key가 경로에 포함되므로 인증 미주입 fetch + 본문 에러 검사.
            let bytes = client.get_bytes_plain(&url).await?;
            if g.raw {
                return output::print_raw_bytes(&bytes);
            }
            let saved = output::save_bytes(&out, &bytes)?;
            output::print_json(g, &serde_json::json!({"ok": true, "saved": saved}))
        }
    }
}

/// 타일 URL을 계열별로 조립(enum 분기, key 경로 포함). TMS는 Y축 반전.
/// 반환: (TileKind, url, resolved_ext) — ext는 스킴 기본값 반영.
fn build_tile_url(a: &TileArgs, key: &str) -> Result<(TileKind, String, String)> {
    match a.scheme.as_str() {
        "wmts" => {
            let ext = a.ext.as_deref().unwrap_or("png");
            let (z, row, col) = require_zrc(a)?;
            let url = PathBuilder::tile("wmts", key, &a.layer, z, row, col, ext).url();
            Ok((TileKind::Raster, url, ext.to_string()))
        }
        "tms" => {
            let ext = a.ext.as_deref().unwrap_or("png");
            let (z, row, col) = require_zrc(a)?;
            // WMTS row → TMS row(Y축 반전, §1.3-결정1 골든 공식).
            let tms_row = wmts_row_to_tms(z, row);
            let url = PathBuilder::tile("tms", key, &a.layer, z, tms_row, col, ext).url();
            Ok((TileKind::Raster, url, ext.to_string()))
        }
        "vector" => {
            let (z, row, col) = require_zrc(a)?;
            // --ext png/jpeg → 래스터 PNG 엔드포인트(key 먼저, getTile 없음).
            // --ext 미지정 또는 --ext pbf → MVT(.pbf).
            match a.ext.as_deref() {
                Some("png") | Some("jpeg") => {
                    let ext = a.ext.as_deref().unwrap();
                    let url = PathBuilder::vector_raster(key, &a.layer, z, row, col, ext).url();
                    Ok((TileKind::Raster, url, ext.to_string()))
                }
                _ => {
                    // 미지정(None) 또는 "pbf" → MVT.
                    let rest = format!("{}/{}/{}/{}.pbf", a.layer, z, row, col);
                    let url = PathBuilder::vector("getTile", key, &rest).url();
                    Ok((TileKind::VectorMvt, url, "pbf".to_string()))
                }
            }
        }
        "vector-style" => {
            // getStyle은 좌표 없음(§1.3-결정1). 스타일명 예: vectorStylePoi.
            let url = PathBuilder::vector("getStyle", key, &a.layer).url();
            Ok((TileKind::VectorStyle, url, "json".to_string()))
        }
        "wmts-themes" => {
            // 해외위성영상 시계열 — Satellite/themes/{category}/{year}/{city}/{z}/{row}/{col}.{ext}.
            let ext = a.ext.as_deref().unwrap_or("png");
            let (z, row, col) = require_zrc(a)?;
            let category = a
                .category
                .as_deref()
                .ok_or_else(|| anyhow!("wmts-themes는 --category가 필요합니다(예: cities)"))?;
            let year = a
                .year
                .as_deref()
                .ok_or_else(|| anyhow!("wmts-themes는 --year가 필요합니다(예: 2025)"))?;
            let city = a
                .city
                .as_deref()
                .ok_or_else(|| anyhow!("wmts-themes는 --city가 필요합니다(예: Oslo)"))?;
            let url =
                PathBuilder::wmts_themes(key, category, year, city, z, row, col, ext).url();
            Ok((TileKind::Raster, url, ext.to_string()))
        }
        "wmts-capabilities" => {
            // GetCapabilities — key만 필요, 좌표 없음. XML 메타데이터.
            let url = PathBuilder::wmts_capabilities(key).url();
            Ok((TileKind::Capabilities, url, "xml".to_string()))
        }
        other => Err(anyhow!(
            "알 수 없는 타일 스킴: {other} (wmts/tms/vector/vector-style/wmts-themes/wmts-capabilities)"
        )),
    }
}

fn require_zrc(a: &TileArgs) -> Result<(u32, u32, u32)> {
    match (a.z, a.row, a.col) {
        (Some(z), Some(row), Some(col)) => Ok((z, row, col)),
        _ => Err(anyhow!("타일은 --z --row --col이 모두 필요합니다")),
    }
}
