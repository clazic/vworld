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
    /// 중심 좌표 "x,y".
    pub center: String,
    /// 줌 레벨(6~18).
    #[arg(long)]
    pub zoom: u32,
    /// 이미지 크기 "width,height"(최대 1024,1024).
    #[arg(long, default_value = "512,512")]
    pub size: String,
    /// 지도 유형.
    #[arg(long, default_value = "GRAPHIC")]
    pub basemap: String,
    /// 이미지 포맷.
    #[arg(long, default_value = "png")]
    pub format: String,
    #[arg(long, default_value = "EPSG:4326")]
    pub crs: String,
    /// 저장 경로.
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
    /// 대상 레이어.
    pub layer: String,
    #[arg(long)]
    pub style: Option<String>,
    /// 범례 타입.
    #[arg(long, default_value = "ALL")]
    pub r#type: String,
    #[arg(long, default_value = "png")]
    pub format: String,
    /// SLD 스타일 정의(XML) 조회 — request=GetLegendStyle. 미지정 시 범례 이미지(GetLegendGraphic).
    #[arg(long)]
    pub sld: bool,
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
    /// 스킴: wmts | tms | vector | vector-style | wmts-themes | wmts-capabilities.
    pub scheme: String,
    /// 레이어(WMTS: Base/white/midnight/Hybrid/Satellite, 벡터: Base/poi/traffic).
    #[arg(long, default_value = "Base")]
    pub layer: String,
    /// wmts-themes 전용: 테마 카테고리(예: cities).
    #[arg(long)]
    pub category: Option<String>,
    /// wmts-themes 전용: 영상 연도(예: 2025).
    #[arg(long)]
    pub year: Option<String>,
    /// wmts-themes 전용: 도시명(예: Oslo).
    #[arg(long)]
    pub city: Option<String>,
    /// 줌 레벨.
    #[arg(long)]
    pub z: Option<u32>,
    /// 타일 행(WMTS Y / 표기 주의 §1.3-결정1).
    #[arg(long)]
    pub row: Option<u32>,
    /// 타일 열(WMTS X).
    #[arg(long)]
    pub col: Option<u32>,
    /// 타일 확장자(wmts/tms 기본: png, vector 기본: pbf, 래스터 벡터: png/jpeg).
    #[arg(long)]
    pub ext: Option<String>,
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
