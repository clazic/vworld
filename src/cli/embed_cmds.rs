//! 브라우저 렌더링형 명령 핸들러 — 지도 URL/HTML/설정 생성(설계 §1.3·§3, Non-Goal: 직접 렌더).

use super::GlobalArgs;
use super::data_cmds::load_auths;
use crate::output;
use anyhow::{anyhow, Result};
use clap::Args;

#[derive(Args, Debug)]
pub struct MapArgs {
    /// 지도 종류: 2d | 3d | 3dsim | ol | marker | chart | theme.
    /// (2d=3D엔진 평면모드 / ol=OpenLayers 2D 데이터레이어 데모)
    #[arg(default_value = "2d")]
    pub kind: String,
    /// 중심 좌표 "lon,lat". 분석 모드에선 지도 중심 이동(미지정 시 샘플 기본 위치 유지).
    #[arg(long)]
    pub center: Option<String>,
    /// 중심 주소(분석/ol 모드) — geocode로 좌표 변환 후 지도 중심 이동. --center보다 우선.
    #[arg(long)]
    pub address: Option<String>,
    /// 줌/높이 레벨.
    #[arg(long, default_value_t = 11)]
    pub zoom: u32,
    /// 3D 분석·시뮬레이션 종류(3dsim 전용). `list`로 전체 목록 출력.
    /// 예: slope, terrainvolume, profile, sunlight, sunlightrights,
    /// sunlightslope, visiblearea, viewsurface, culheritalter, route,
    /// buildingcontrol, heatmap, cluster, grid, hexbin.
    #[arg(long)]
    pub analysis: Option<String>,
    /// HTML 파일로 저장(미지정 시 JSON으로 URL·스니펫 보고).
    #[arg(long, short)]
    pub output: Option<std::path::PathBuf>,

    // --- 단계1: ol3(OpenLayers 2D) 데이터레이어 ---
    /// 배경지도 종류(ol/marker/chart/theme): GRAPHIC | GRAPHIC_WHITE | GRAPHIC_NIGHT | PHOTO | PHOTO_HYBRID.
    #[arg(long)]
    pub basemap: Option<String>,
    /// 인라인 폴리곤 좌표 "lon,lat;lon,lat;…"(ol). EPSG:4326 입력.
    #[arg(long)]
    pub polygon: Option<String>,
    /// GeoJSON FeatureCollection 파일(ol). 4326 입력 → ol3 벡터 렌더(폴리곤/포인트/라인).
    #[arg(long)]
    pub geojson: Option<std::path::PathBuf>,
    /// 마커 JSON 파일(marker): [{x,y,epsg?,title,contents,iconUrl?,text?}].
    #[arg(long)]
    pub points: Option<std::path::PathBuf>,
    /// 차트 JSON 파일(chart): [{pos:[lon,lat],title,size?,radius?,styles:[{color,label,legendLabel?}],values:[…]}].
    #[arg(long)]
    pub data: Option<std::path::PathBuf>,
    /// 차트 종류(chart): bar | stackedbar | pie.
    #[arg(long = "type")]
    pub chart_type: Option<String>,
    /// 차트 그룹 정렬(chart) — ChartGroup으로 묶어 표시.
    #[arg(long)]
    pub group: bool,
    /// 주제도 WMS named layer(theme): "이름:LAYER_ID,이름:LAYER_ID".
    #[arg(long)]
    pub layers: Option<String>,
    /// 줌 슬라이더 컨트롤(PanZoomBar) 표시(ol).
    #[arg(long)]
    pub zoom_control: bool,
    /// 배경지도 전환 버튼 표시(ol).
    #[arg(long)]
    pub basemap_switch: bool,
    /// 클릭 시 좌표 팝업 활성화(ol).
    #[arg(long)]
    pub popup: bool,

    // --- 단계2: TEXTLayer / KMLLayer ---
    /// KML 레이어 URL(ol) — 외부 KML. 절대경로 https만(자기완결·CORS).
    #[arg(long)]
    pub kml: Option<String>,
    /// 대량 포인트 TEXT 파일(text) — vworld TEXT 포맷, 자기완결 임베드(최대 500줄).
    #[arg(long)]
    pub file: Option<std::path::PathBuf>,
    /// 좌표계(text): 기본 EPSG:4326.
    #[arg(long)]
    pub epsg: Option<String>,
    /// 포인트 클러스터링 거리(text): 기본 40.
    #[arg(long, default_value_t = 40)]
    pub distance: u32,

    // --- 추가 컨트롤·경로(잔여 구현) ---
    /// 미니맵(OverviewMap) 표시(ol).
    #[arg(long)]
    pub overview: bool,
    /// 측정 툴바(ToolBar: 거리·면적·이동·전체보기 등 11종) 표시(ol).
    #[arg(long)]
    pub toolbar: bool,
    /// 사전 경로 좌표 "lon,lat;lon,lat;…"(ol) — RouteMap 폴리라인 표시(최소 2점, EPSG:4326).
    #[arg(long)]
    pub route: Option<String>,

    // --- choropleth ---
    /// 색칠 기준 properties 수치 키 (choropleth 전용).
    #[arg(long)]
    pub value_field: Option<String>,
    /// 색상 램프(choropleth): ylorrd(기본)|blues|greens|reds|viridis.
    #[arg(long, default_value = "ylorrd")]
    pub color_scale: String,
    /// 구간 수(choropleth, 기본 5).
    #[arg(long, default_value_t = 5u8)]
    pub classes: u8,
    /// 구간 분류 방법(choropleth): quantile(기본)|equal.
    #[arg(long, default_value = "quantile")]
    pub class_method: String,
    /// 수동 경계값 "a,b,c,d"(choropleth). 지정 시 class-method 무시.
    #[arg(long)]
    pub breaks: Option<String>,
    /// 값 없는 feature 색(choropleth, 기본 #cccccc).
    #[arg(long, default_value = "#cccccc")]
    pub no_data_color: String,
    /// 채움 투명도(choropleth, 0-1, 기본 0.78).
    #[arg(long, default_value_t = 0.78f32)]
    pub opacity: f32,
    /// 범례 표시(choropleth).
    #[arg(long)]
    pub legend: bool,
    /// 생성 HTML을 OS 기본 브라우저로 열기(-o 저장된 경우만).
    #[arg(long)]
    pub open: bool,
}

/// 3D 분석·시뮬레이션 템플릿 — vworld 공식 코드샘플을 임베드(자기완결 단일 바이너리).
/// `(키, 한글설명, 출처 API, 공식 HTML 템플릿)`. 템플릿의 `@{apikey}`를 실제 키로 치환해 사용.
const ANALYSES: &[(&str, &str, &str, &str)] = &[
    // 1.0 분석·시뮬레이션 (https://www.vworld.kr/dev/v4dv_opn3dsimmapguide_s001.do)
    ("slope", "경사도 분석", "1.0", include_str!("tool3d_samples/slope.html")),
    ("terrainvolume", "토공량 분석", "1.0", include_str!("tool3d_samples/terrainVolume.html")),
    ("profile", "지형 단면 분석", "1.0", include_str!("tool3d_samples/profileTerrain.html")),
    ("sunlight", "일조량 분석", "1.0", include_str!("tool3d_samples/sunlightAnalysis.html")),
    ("sunlightrights", "일조권 분석", "1.0", include_str!("tool3d_samples/sunlightrightsAnalysis.html")),
    ("sunlightslope", "일조권 사선제한 분석", "1.0", include_str!("tool3d_samples/SunlightSlopeConstraint.html")),
    ("visiblearea", "가시면적 분석", "1.0", include_str!("tool3d_samples/VisibleArea.html")),
    ("viewsurface", "시곡면 분석", "1.0", include_str!("tool3d_samples/ViewSurface.html")),
    ("culheritalter", "문화재 현상변경 분석", "1.0", include_str!("tool3d_samples/CulHeritAlter.html")),
    ("route", "드론·차량 모의주행 시뮬레이션", "1.0", include_str!("tool3d_samples/RouteSimulation.html")),
    ("buildingcontrol", "건물모델(glb) 편집", "1.0", include_str!("tool3d_samples/buildingControl.html")),
    // 2.0 가시화 (https://www.vworld.kr/dev/v4dv_opn3dsimmap2guide_s001.do)
    ("heatmap", "Heatmap 가시화", "2.0", include_str!("tool3d_samples/heatmap.html")),
    ("cluster", "Cluster 가시화", "2.0", include_str!("tool3d_samples/cluster.html")),
    ("grid", "Grid 가시화", "2.0", include_str!("tool3d_samples/grid.html")),
    ("hexbin", "Hexbin 가시화", "2.0", include_str!("tool3d_samples/hexbin.html")),
    // 3.0 WebGL 3D지도 API (https://www.vworld.kr/dev/v4dv_opnws3dmap3guide_s001.do)
    ("responsive", "반응형 웹에서 3.0 사용", "3.0", include_str!("tool3d_samples/v3_responsive.html")),
    ("lod4texture", "LOD4 텍스쳐 on/off", "3.0", include_str!("tool3d_samples/v3_lod4texture.html")),
    ("mapcontroller", "지도 생성(MapController)", "3.0", include_str!("tool3d_samples/v3_mapcontroller.html")),
    ("mapoption", "초기 옵션 지도 생성", "3.0", include_str!("tool3d_samples/v3_mapoption.html")),
    ("moveto", "좌표 이동·줌레벨 설정", "3.0", include_str!("tool3d_samples/v3_moveto.html")),
    ("geometry", "지오메트리 Point/Line/Polygon", "3.0", include_str!("tool3d_samples/v3_geometry.html")),
    ("geometryz", "z좌표 포함 지오메트리", "3.0", include_str!("tool3d_samples/v3_geometryz.html")),
    ("wms", "WMS 정보 표출", "3.0", include_str!("tool3d_samples/v3_wms.html")),
    ("buildinginfo", "건물 클릭 정보", "3.0", include_str!("tool3d_samples/v3_buildinginfo.html")),
    ("cameraturn", "카메라 방향 전환", "3.0", include_str!("tool3d_samples/v3_cameraturn.html")),
    ("flight", "비행 시뮬레이션", "3.0", include_str!("tool3d_samples/v3_flight.html")),
    ("rotateface", "회전 정면 관찰", "3.0", include_str!("tool3d_samples/v3_rotateface.html")),
    ("rotateground", "회전 지면 관찰", "3.0", include_str!("tool3d_samples/v3_rotateground.html")),
    ("driving", "운전 시뮬레이션", "3.0", include_str!("tool3d_samples/v3_driving.html")),
    ("markerevent", "마커 이벤트 추가", "3.0", include_str!("tool3d_samples/v3_markerevent.html")),
    ("circle", "Circle/CircleZ 지오메트리", "3.0", include_str!("tool3d_samples/v3_circle.html")),
    ("regularshape", "RegularShape 지오메트리", "3.0", include_str!("tool3d_samples/v3_regularshape.html")),
    ("specialshape", "SpecialShape 지오메트리", "3.0", include_str!("tool3d_samples/v3_specialshape.html")),
    ("imagesave", "이미지 저장 API", "3.0", include_str!("tool3d_samples/v3_imagesave.html")),
    ("geojson", "Geojson/GML 해석", "3.0", include_str!("tool3d_samples/v3_geojson.html")),
    ("wfs", "WFS 레이어 생성", "3.0", include_str!("tool3d_samples/v3_wfs.html")),
    ("glb", "glb/gltf 업로드", "3.0", include_str!("tool3d_samples/v3_glb.html")),
    ("wmswfs", "WMS/WFS API 응용", "3.0", include_str!("tool3d_samples/v3_wmswfs.html")),
    ("search", "검색 API 결과 표시", "3.0", include_str!("tool3d_samples/v3_search.html")),
    ("dataapi", "데이터 API 좌표 객체 생성", "3.0", include_str!("tool3d_samples/v3_dataapi.html")),
    ("wmts", "WMTS 레이어 추가", "3.0", include_str!("tool3d_samples/v3_wmts.html")),
    ("home", "Home 버튼 위치 이동", "3.0", include_str!("tool3d_samples/v3_home.html")),
    ("measure", "높이·거리·면적 측정", "3.0", include_str!("tool3d_samples/v3_measure.html")),
    ("buildingroll", "건물 Roll 기능", "3.0", include_str!("tool3d_samples/v3_buildingroll.html")),
    ("draw", "포인트/라인 그리기·삭제·수정", "3.0", include_str!("tool3d_samples/v3_draw.html")),
    ("markergroup", "마커 그룹 관리", "3.0", include_str!("tool3d_samples/v3_markergroup.html")),
    ("boundary", "화면 바운더리 동서남북 조회", "3.0", include_str!("tool3d_samples/v3_boundary.html")),
    ("editfeature", "포인트/라인 편집", "3.0", include_str!("tool3d_samples/v3_editfeature.html")),
    ("popup", "포인트 선택 팝업 생성·제거", "3.0", include_str!("tool3d_samples/v3_popup.html")),
];

pub async fn run_map(g: &GlobalArgs, a: MapArgs) -> Result<()> {
    // --analysis list: 키 없이도 목록 출력.
    if a.analysis.as_deref() == Some("list") {
        return list_analyses(g);
    }

    // 키·도메인은 스크립트 include에 주입.
    let auths = load_auths(g)?;
    let key = &auths[0].key;
    let domain = auths[0].domain.clone().unwrap_or_default();

    // 3D 분석·시뮬레이션: 공식 샘플 템플릿에 키 주입.
    if let Some(name) = &a.analysis {
        return run_analysis(g, &a, name, key, &domain).await;
    }

    // 단계1: ol3(OpenLayers 2D) 데이터레이어 명령.
    match a.kind.as_str() {
        "ol" => return run_ol(g, &a, key, &domain).await,
        "marker" => return run_marker(g, &a, key, &domain),
        "chart" => return run_chart(g, &a, key, &domain),
        "theme" => return run_theme(g, &a, key, &domain),
        "text" => return run_text(g, &a, key, &domain),
        "controller" => return run_controller(g, &a, key, &domain),
        "vector" => return run_vector(g, &a, key, &domain).await,
        "choropleth" => return run_choropleth(g, &a, key, &domain).await,
        _ => {}
    }

    let (script_url, init) = match a.kind.as_str() {
        "2d" => (
            format!("https://map.vworld.kr/js/vworldMapInit.js.do?version=2.0&apiKey={key}&domain={domain}"),
            "vw.MapControllerOption / vw.Map() (OpenLayers 기반 2D)",
        ),
        "3d" => (
            format!("https://map.vworld.kr/js/webglMapInit.js.do?version=2.0&apiKey={key}&domain={domain}"),
            "vw.Map() WebGL 3D(Cesium 기반)",
        ),
        "3dsim" => (
            format!("https://map.vworld.kr/js/webglMapInit.js.do?version=2.0&apiKey={key}&domain={domain}"),
            "WebGL 3D + tool3d(terrainVolume/heatmap/cluster) 분석 모듈",
        ),
        other => return Err(anyhow!("알 수 없는 지도 종류: {other} (2d/3d/3dsim)")),
    };

    let center = a.center.as_deref().unwrap_or("127.0,37.5");
    let mut html = render_html(&a.kind, &script_url, center, a.zoom, init);
    // 토스 디자인 주입(모든 생성 HTML 공통).
    html = html.replace("</head>", &format!("{TOSS_STYLE}</head>"));

    if let Some(path) = &a.output {
        let saved = output::save_bytes(path, html.as_bytes())?;
        return output::print_json(g, &serde_json::json!({"ok": true, "saved": saved, "script_url": script_url}));
    }
    if g.raw {
        return output::print_raw_text(&html);
    }
    output::print_json(
        g,
        &serde_json::json!({
            "ok": true,
            "kind": a.kind,
            "script_url": script_url,
            "init": init,
            "html": html,
        }),
    )
}

/// 분석 템플릿의 초기 카메라 좌표 리터럴(15종 공통, 정확히 1회 등장).
const SAMPLE_CENTER: &str = "new vw.CoordZ(126.923100000039,37.5262083301207,2000)";

/// 토스(Toss) 디자인 시스템 스타일(DESIGN.md 기반). `</head>`에 주입.
/// 지도(#vmap)를 크게, 컨트롤은 토스 카드/버튼으로. 지도 내부 vworld 런타임 UI는 `all:revert`로 격리.
const TOSS_STYLE: &str = r#"<link rel="stylesheet" href="https://cdn.jsdelivr.net/gh/orioncactus/pretendard@latest/dist/web/static/pretendard.min.css">
<style>
:root{--tb:#3182f6;--tbd:#2272eb;--ink:#191f28;--bdy:#4e5968;--sub:#8b95a1;--surface:#f2f4f6;--line:#e5e8eb}
html,body{margin:0;background:var(--surface);font-family:Pretendard,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;color:var(--ink);-webkit-font-smoothing:antialiased}
body{padding:0;max-width:none;margin:0;line-height:1.55}
/* 맵 화면 전체 */
#vmap{width:100vw!important;height:100vh!important;min-height:100vh!important;border-radius:0!important;box-shadow:none!important;overflow:hidden!important;margin:0!important;border:0!important}
/* 샘플 하단 컨트롤 input 버튼(2~6번째) 숨김 — 풀스크린 맵 정리 */
body>input[type=button]:nth-child(2),body>input[type=button]:nth-child(3),body>input[type=button]:nth-child(4),body>input[type=button]:nth-child(5),body>input[type=button]:nth-child(6){display:none!important}
#vmap *{font-family:initial}
#vmap button,#vmap input,#vmap select,#vmap a,#vmap label{all:revert}
h1,h2,h3,h4{color:var(--ink);font-weight:700;letter-spacing:-.01em}
h3,h4{font-size:17px;margin:18px 0 10px}
label{color:var(--bdy);font-weight:600;font-size:14px;margin-right:6px}
button,input[type=button],input[type=submit],input[type=reset]{background:var(--tb);color:#fff;border:0;border-radius:14px;padding:13px 20px;font:600 16px Pretendard,sans-serif;cursor:pointer;transition:background .15s,transform .05s;margin:4px 6px 4px 0;box-shadow:0 1px 2px rgba(49,130,246,.25);-webkit-appearance:none;appearance:none}
button:hover,input[type=button]:hover,input[type=submit]:hover,input[type=reset]:hover{background:var(--tbd)}
button:active,input[type=button]:active,input[type=submit]:active,input[type=reset]:active{transform:scale(.98)}
input,select{background:#fff;color:#333d4b;border:1px solid var(--line);border-radius:12px;padding:11px 13px;font:400 15px Pretendard,sans-serif;outline:none;transition:border-color .15s,box-shadow .15s}
input:focus,select:focus{border-color:var(--tb);box-shadow:0 0 0 3px #e8f3ff}
input[type=color]{padding:4px;width:46px;height:40px;cursor:pointer;vertical-align:middle}
textarea{width:100%;box-sizing:border-box;border:1px solid var(--line);border-radius:12px;padding:12px;font:400 13px ui-monospace,SFMono-Regular,monospace;outline:none}
textarea:focus{border-color:var(--tb);box-shadow:0 0 0 3px #e8f3ff}
#result,#slopeResult{background:#fff;border-radius:16px;padding:18px 20px;box-shadow:0 1px 3px rgba(0,0,0,.06),0 4px 16px rgba(0,0,0,.05);margin-top:16px}
table{border-collapse:collapse;font-variant-numeric:tabular-nums;font-size:14px;margin-top:8px}
td,th{padding:6px 14px 6px 0;text-align:left;color:var(--bdy)}
p{color:var(--bdy);margin:6px 0}
span[id]{color:var(--ink);font-weight:600;font-variant-numeric:tabular-nums}
#vmap .vw-logo,#vmap .vw-notice{display:none!important}
/* vworld 내부 지도 툴바를 토스 투톤으로 — all:revert 오버라이드. 내비/줌=블루, 측정=화이트+블루 */
/* 컨테이너: 투명 배경 + 간격 */
#vmap .dt-api-map--tool-btns,#vmap .dt-api-map--tool-btns-measure{display:flex!important;flex-direction:column!important;gap:8px!important;background:transparent!important;border:0!important;box-shadow:none!important}
#vmap .dt-api-map--move-btns{gap:7px!important;background:transparent!important;border:0!important;box-shadow:none!important;padding:0!important}
/* 펼침(-on) 상태에서만 패널 폭 확장 — 접힘(width:0 숨김)은 건드리지 않음 */
/* 이동(pan)컨트롤 패널: 이동컨트롤열기 버튼 왼쪽에서 펼침(아래로이동이 토글에 가장 가깝게) + 4개 안 잘리게 폭 자동 */
#vmap .dt-api-map--move.dt-api-map--move-on{width:auto!important}
#vmap .dt-api-map--move-on .dt-api-map--move-btns{display:flex!important;flex-direction:row!important;flex-wrap:nowrap!important;width:auto!important}
/* 공통 베이스(catch-all): 모든 dt-api-map 버튼 — 솔리드 토스블루 + 흰 아이콘 (닫기 버튼 등 포함) */
#vmap button[class*="dt-api-map--"]{width:60px!important;min-width:60px!important;min-height:60px!important;height:auto!important;background:var(--tb)!important;border:0!important;border-radius:16px!important;color:#fff!important;font:600 10px/1.18 Pretendard,sans-serif!important;letter-spacing:-.02em!important;display:flex!important;flex-direction:column!important;align-items:center!important;justify-content:center!important;gap:3px!important;padding:8px 4px!important;cursor:pointer!important;box-shadow:0 3px 10px rgba(49,130,246,.28)!important;transition:background .15s,color .15s,transform .05s,box-shadow .15s!important;text-shadow:none!important;text-align:center!important;white-space:normal!important;word-break:keep-all!important}
#vmap button[class*="dt-api-map--"]::before{filter:brightness(0) invert(1)!important;width:24px!important;height:24px!important;flex:0 0 auto!important;background-size:contain!important;background-repeat:no-repeat!important;background-position:center!important;opacity:1!important}
#vmap button[class*="dt-api-map--"]:hover{background:var(--tbd)!important;transform:translateY(-1px)!important;box-shadow:0 5px 14px rgba(49,130,246,.40)!important}
#vmap button[class*="dt-api-map--"]:active{transform:scale(.96)!important}
/* 이동컨트롤 버튼은 좁은 컨테이너에 맞춰 폭만 축소(높이·색은 공통 유지) */
#vmap .dt-api-map--move-bt{width:46px!important;min-width:46px!important}
/* 닫기(X) 버튼 삭제 (catch-all보다 우선하도록 specificity 상향) */
#vmap .dt-api-map--move .dt-api-map--move-close,#vmap button.dt-api-map--move-close{display:none!important}
/* 접힘(-on 없음) 상태에서 이동버튼 숨김 — 강제 버튼크기가 vworld width:0 접힘을 깨므로 명시적 숨김 */
#vmap .dt-api-map--move:not(.dt-api-map--move-on) .dt-api-map--move-btns{display:none!important}
/* 패널 transform transition 제거 — JS 정렬이 애니메이션 중간값에 걸리지 않도록 즉시 적용 */
#vmap .dt-api-map--move{transition:none!important}
/* 지형투명도·시간 슬라이더 툴바를 아래로 */
#vmap #mapToolBar{top:140px!important}
/* 측정 도구(거리·면적·높이·초기화): 뉴트럴 화이트 카드 + 블루 아이콘·텍스트 */
#vmap .dt-api-map--tool-btns-measure .dt-api-map--tool-bt{background:#fff!important;color:var(--tb)!important;border:1px solid var(--line)!important;box-shadow:0 2px 8px rgba(0,0,0,.10)!important}
#vmap .dt-api-map--tool-btns-measure .dt-api-map--tool-bt::before{filter:brightness(0) saturate(100%) invert(38%) sepia(86%) saturate(1900%) hue-rotate(202deg) brightness(99%) contrast(96%)!important}
#vmap .dt-api-map--tool-btns-measure .dt-api-map--tool-bt:hover{background:var(--tb)!important;color:#fff!important;border-color:var(--tb)!important}
#vmap .dt-api-map--tool-btns-measure .dt-api-map--tool-bt:hover::before{filter:brightness(0) invert(1)!important}
/* 측정 도구 라디오 선택(active): 채워진 토스블루 + 흰 아이콘 */
#vmap .dt-api-map--tool-btns-measure .dt-api-map--tool-bt.toss-active{background:var(--tb)!important;color:#fff!important;border-color:var(--tb)!important;box-shadow:0 4px 12px rgba(49,130,246,.40)!important}
#vmap .dt-api-map--tool-btns-measure .dt-api-map--tool-bt.toss-active::before{filter:brightness(0) invert(1)!important}
</style>
"#;

/// 지도 툴바 보정 스크립트(`</body>` 앞 주입):
/// ① 이동(pan)패널을 이동컨트롤열기(mouse) 버튼과 같은 높이로 정렬.
/// ② 측정 도구(거리·면적·높이)를 라디오 버튼처럼 — 하나만 active, 초기화는 선택 해제.
const TOOLBAR_JS: &str = r#"<script>
(function(){
  function init(){
    var box=document.querySelector('#naviTopPannel3d .dt-api-map--tool-btns-measure');
    var mouse=document.querySelector('#naviTopPannel3d .dt-api-map--tool-bt-mouse');
    var bot=document.querySelector('#naviBottomPannel3d');
    // ② 라디오 동작
    if(box){
      var btns=[].slice.call(box.querySelectorAll('.dt-api-map--tool-bt'));
      btns.forEach(function(btn){
        btn.addEventListener('click',function(){
          var isReset=btn.className.indexOf('tool-bt-reset')>=0;
          btns.forEach(function(b){b.classList.remove('toss-active');});
          if(!isReset) btn.classList.add('toss-active');
        });
      });
    }
    // ① 이동패널을 이동컨트롤열기 버튼과 위(top) 정렬 + 가로 간격(transform, 실제 버튼 기준·idempotent)
    function align(){
      if(!mouse||!bot) return;
      var first=bot.querySelector('.dt-api-map--move-bt'); if(!first) return;
      var dw=bot.querySelector('.dt-api-map--move-bt-dw')||first;
      bot.style.transform='none'; // 기본 위치로 리셋 후 측정
      void bot.offsetWidth;       // 강제 reflow
      if(first.getBoundingClientRect().width<1) return; // 접힘 상태면 스킵
      var mr=mouse.getBoundingClientRect();
      var GAP=8;
      var dy=mr.top-first.getBoundingClientRect().top;        // 버튼 top → 마우스버튼 top
      var dx=(mr.left-GAP)-dw.getBoundingClientRect().right;  // 아래로이동 우측 → 마우스버튼 좌측 -GAP
      bot.style.transform='translate('+dx+'px,'+dy+'px)';
    }
    align();
    window.addEventListener('resize',align);
    // 클릭 즉시 버튼을 숨기고(opacity:0) rAF에서 정렬 후 표시 → base 위치가 한 프레임도 페인트되지 않게(깜빡임 완전 제거).
    if(mouse) mouse.addEventListener('click',function(){
      var btns=bot.querySelector('.dt-api-map--move-btns'); if(btns) btns.style.opacity='0';
      requestAnimationFrame(function(){
        align();
        requestAnimationFrame(function(){ align(); if(btns) btns.style.opacity='1'; });
      });
      setTimeout(align,120);
    });
  }
  var n=0,t=setInterval(function(){
    if(document.querySelector('#naviTopPannel3d .dt-api-map--tool-bt-mouse')){ clearInterval(t); init(); }
    else if(++n>40){ clearInterval(t); }
  },250);
})();
</script>
"#;

/// 화면 상단 중앙 주소 검색창(토스 디자인) + 지오코딩 이동 스크립트. `</body>` 앞 주입.
/// vworld 지오코드 API는 브라우저 CORS 차단 → JSONP(`&callback=`)로 우회. 도로명→지번→통합검색 순 폴백.
/// 지도 이동은 Cesium 카메라 flyTo(범용) → 실패 시 map.moveTo 폴백.
const SEARCH_BOX: &str = r##"<div id="vwSearch" style="position:fixed;top:22px;left:50%;transform:translateX(-50%);z-index:100000;display:flex;align-items:center;gap:6px;background:#fff;border:1px solid #e5e8eb;border-radius:18px;box-shadow:0 8px 28px rgba(0,0,0,.16);padding:9px 9px 9px 16px;width:min(540px,92vw);box-sizing:border-box">
<svg width="20" height="20" viewBox="0 0 24 24" fill="none" style="flex:0 0 auto"><circle cx="11" cy="11" r="7" stroke="#8b95a1" stroke-width="2"/><path d="M20 20l-3.6-3.6" stroke="#8b95a1" stroke-width="2" stroke-linecap="round"/></svg>
<input id="vwSearchInput" type="text" placeholder="주소 검색 (예: 서울특별시청, 세종대로 110)" style="flex:1;min-width:0;border:0;outline:0;background:transparent;font:500 16px Pretendard,sans-serif;color:#191f28">
<button id="vwSearchBtn" type="button" style="flex:0 0 auto;background:#3182f6;color:#fff;border:0;border-radius:12px;padding:10px 18px;font:600 15px Pretendard,sans-serif;cursor:pointer;box-shadow:0 2px 8px rgba(49,130,246,.3)">검색</button>
</div>
<div id="vwSearchToast" style="position:fixed;top:78px;left:50%;transform:translateX(-50%);z-index:100000;background:#191f28;color:#fff;padding:10px 16px;border-radius:12px;font:500 14px Pretendard,sans-serif;box-shadow:0 6px 20px rgba(0,0,0,.25);display:none"></div>
<script>
(function(){
  function key(){ var s=document.querySelector('script[src*="webglMapInit"]'); if(s){var m=s.src.match(/apiKey=([^&]+)/);if(m)return m[1];} return ''; }
  function toast(msg){ var t=document.getElementById('vwSearchToast'); t.textContent=msg; t.style.display='block'; clearTimeout(t._h); t._h=setTimeout(function(){t.style.display='none';},2600); }
  function moveTo(lon,lat){
    lon=parseFloat(lon); lat=parseFloat(lat);
    try{ var v=vw.NavigationZoom.map._wsViewer; v.camera.flyTo({destination:Cesium.Cartesian3.fromDegrees(lon,lat,2000),duration:1.5}); return true; }catch(e){}
    try{ window.map.moveTo(new vw.CameraPosition(new vw.CoordZ(lon,lat,2000),new vw.Direction(0,-80,0))); return true; }catch(e){}
    return false;
  }
  function jsonp(url,cb){
    var name='__vwg'+Math.floor(performance.now()*1000%1e9);
    window[name]=function(d){ try{cb(d);}finally{ try{delete window[name];}catch(e){} if(sc.parentNode)sc.parentNode.removeChild(sc); } };
    var sc=document.createElement('script'); sc.src=url+'&callback='+name; sc.onerror=function(){ cb(null); }; document.body.appendChild(sc);
  }
  function geoUrl(addr,type){ return 'https://api.vworld.kr/req/address?service=address&request=getcoord&version=2.0&crs=epsg:4326&address='+encodeURIComponent(addr)+'&refine=true&simple=false&format=json&type='+type+'&key='+key(); }
  function searchUrl(addr){ return 'https://api.vworld.kr/req/search?service=search&request=search&version=2.0&crs=EPSG:4326&size=1&page=1&query='+encodeURIComponent(addr)+'&type=address&format=json&key='+key(); }
  function pt(d){ try{ if(d&&d.response&&d.response.status==='OK'&&d.response.result&&d.response.result.point) return d.response.result.point; }catch(e){} return null; }
  function run(){
    var addr=(document.getElementById('vwSearchInput').value||'').trim();
    if(!addr) return;
    jsonp(geoUrl(addr,'road'), function(d){
      var p=pt(d); if(p){ moveTo(p.x,p.y); return; }
      jsonp(geoUrl(addr,'parcel'), function(d2){
        var p2=pt(d2); if(p2){ moveTo(p2.x,p2.y); return; }
        jsonp(searchUrl(addr), function(d3){
          try{ var it=d3.response.result.items[0]; if(it&&it.point){ moveTo(it.point.x,it.point.y); return; } }catch(e){}
          toast('주소를 찾을 수 없습니다: '+addr);
        });
      });
    });
  }
  function ready(){
    var inp=document.getElementById('vwSearchInput'), btn=document.getElementById('vwSearchBtn');
    if(!inp||!btn) return;
    btn.addEventListener('click',run);
    inp.addEventListener('keydown',function(e){ if(e.key==='Enter'){ e.preventDefault(); run(); } });
  }
  if(document.readyState!=='loading') ready(); else document.addEventListener('DOMContentLoaded',ready);
})();
</script>
"##;

/// 3.0 샘플 초기 카메라 좌표 치환: 첫 `new vw.CoordZ(lon, lat, alt)`의 경도·위도만 사용자 좌표로 바꾸고 고도는 유지.
/// 좌표 리터럴이 없는 패턴 B(MapControllerOption + vw.ol3.CameraPosition)면 None.
fn recenter_v3(html: &str, lon: &str, lat: &str) -> Option<String> {
    const NEEDLE: &str = "new vw.CoordZ(";
    let start = html.find(NEEDLE)?;
    let args_start = start + NEEDLE.len();
    let rel_end = html[args_start..].find(')')?;
    let end = args_start + rel_end;
    let args = &html[args_start..end];
    let parts: Vec<&str> = args.splitn(3, ',').collect();
    if parts.len() != 3 {
        return None;
    }
    let alt = parts[2].trim();
    // `&html[..args_start]`는 `new vw.CoordZ(`까지 포함(needle 보존). lon/lat 교체, 고도 유지.
    Some(format!("{}{lon}, {lat}, {alt}){}", &html[..args_start], &html[end + 1..]))
}

/// 3D 분석·시뮬레이션 HTML 생성 — 공식 샘플 템플릿의 `@{apikey}`를 실제 키(+도메인)로 치환하고,
/// `--address`/`--center` 지정 시 지도 중심을 해당 위치로 이동.
async fn run_analysis(g: &GlobalArgs, a: &MapArgs, name: &str, key: &str, domain: &str) -> Result<()> {
    let needle = name.to_lowercase();
    let Some(&(akey, desc, ver, template)) = ANALYSES.iter().find(|(k, ..)| *k == needle) else {
        let names: Vec<&str> = ANALYSES.iter().map(|(k, ..)| *k).collect();
        return Err(anyhow!(
            "알 수 없는 분석 종류: {name}\n사용 가능: {}\n(전체 목록: --analysis list)",
            names.join(", ")
        ));
    };

    // 중심 좌표 결정: --address(geocode) > --center("lon,lat") > 샘플 기본 위치.
    let center: Option<(String, String)> = if let Some(addr) = &a.address {
        Some(super::data_cmds::geocode_point(g, addr, "EPSG:4326").await?)
    } else if let Some(c) = &a.center {
        let parts: Vec<&str> = c.split(',').map(str::trim).collect();
        match parts.as_slice() {
            [lon, lat] if !lon.is_empty() && !lat.is_empty() => {
                Some((lon.to_string(), lat.to_string()))
            }
            _ => return Err(anyhow!("중심 좌표 형식 오류: '{c}' (예: --center 127.02,37.50)")),
        }
    } else {
        None
    };

    // 키 주입: 도메인이 있으면 함께 부착(https/외부 브라우저 대응).
    let cred = if domain.is_empty() {
        key.to_string()
    } else {
        format!("{key}&domain={domain}")
    };
    let mut html = template.replace("@{apikey}", &cred);

    // 토스 디자인 시스템 스타일 주입(DESIGN.md) — 지도 크게 + 토스 카드/버튼. 지도 내부는 격리.
    html = html.replace("</head>", &format!("{TOSS_STYLE}</head>"));
    // 툴바 보정 스크립트(이동패널 높이 정렬 + 측정도구 라디오) 주입.
    html = html.replace("</body>", &format!("{TOOLBAR_JS}</body>"));
    // 상단 중앙 주소 검색창(토스) 주입.
    html = html.replace("</body>", &format!("{SEARCH_BOX}</body>"));

    // 지도 중심 이동.
    // 1.0/2.0: 단일 SAMPLE_CENTER 리터럴 치환(고도 2000).
    // 3.0: 첫 vw.CoordZ(초기 카메라)의 경도·위도 치환(고도 유지). 좌표 리터럴 없는 패턴 B는 재중심 불가 → 경고.
    let center_requested = center.as_ref().map(|(lon, lat)| format!("{lon},{lat}"));
    let (center_applied, center_out) = if let Some((lon, lat)) = &center {
        if ver == "3.0" {
            match recenter_v3(&html, lon, lat) {
                Some(new_html) => {
                    html = new_html;
                    (true, Some(format!("{lon},{lat}")))
                }
                None => {
                    eprintln!(
                        "[경고] 3.0 샘플 '{}'은(는) 초기 좌표 리터럴이 없어 --center/--address 재중심을 적용할 수 없습니다(무시됨).",
                        akey
                    );
                    (false, None::<String>)
                }
            }
        } else {
            html = html.replace(SAMPLE_CENTER, &format!("new vw.CoordZ({lon},{lat},2000)"));
            (true, Some(format!("{lon},{lat}")))
        }
    } else {
        (false, None::<String>)
    };
    // 재중심이 요청됐으나 적용 안 된 경우(3.0 패턴 B)만 requested_center를 노출.
    let recenter_skipped = center_requested.is_some() && !center_applied;

    if let Some(path) = &a.output {
        let saved = output::save_bytes(path, html.as_bytes())?;
        let mut meta = serde_json::json!({
            "ok": true,
            "analysis": akey,
            "desc": desc,
            "api": ver,
            "saved": saved,
        });
        if center_applied {
            meta["center"] = serde_json::json!(center_out.clone());
            meta["center_applied"] = serde_json::json!(true);
        } else if recenter_skipped {
            meta["center"] = serde_json::json!(null);
            meta["center_applied"] = serde_json::json!(false);
            meta["requested_center"] = serde_json::json!(center_requested.clone());
        } else {
            meta["center"] = serde_json::json!(null);
        }
        return output::print_json(g, &meta);
    }
    if g.raw {
        return output::print_raw_text(&html);
    }
    let mut meta = serde_json::json!({
        "ok": true,
        "analysis": akey,
        "desc": desc,
        "api": ver,
        "html": html,
    });
    if center_applied {
        meta["center"] = serde_json::json!(center_out);
        meta["center_applied"] = serde_json::json!(true);
    } else if recenter_skipped {
        meta["center"] = serde_json::json!(null);
        meta["center_applied"] = serde_json::json!(false);
        meta["requested_center"] = serde_json::json!(center_requested);
    } else {
        meta["center"] = serde_json::json!(null);
    }
    output::print_json(g, &meta)
}

/// 사용 가능한 3D 분석·시뮬레이션 종류 목록 출력.
fn list_analyses(g: &GlobalArgs) -> Result<()> {
    if g.raw {
        // ver가 바뀌는 지점에 그룹 헤더 삽입(버전 오름차순 1.0→2.0→3.0).
        let mut lines: Vec<String> = Vec::new();
        let mut current_ver = "";
        for (k, desc, ver, _) in ANALYSES {
            if *ver != current_ver {
                current_ver = ver;
                lines.push(format!("== API {ver} =="));
            }
            lines.push(format!("{k}\t{desc} (API {ver})"));
        }
        return output::print_raw_text(&lines.join("\n"));
    }
    // JSON 출력: 헤더 없이 항목별 api 필드 유지, count는 전체 개수.
    let items: Vec<serde_json::Value> = ANALYSES
        .iter()
        .map(|(k, desc, ver, _)| serde_json::json!({"key": k, "desc": desc, "api": ver}))
        .collect();
    output::print_json(g, &serde_json::json!({"ok": true, "count": items.len(), "analyses": items}))
}

/// 벡터 지도 뷰어(`map vector`) — OpenLayers + vworld 벡터-래스터 PNG XYZ 베이스맵 + 토스 디자인 + 주소 검색.
/// vworld 벡터 스타일은 OpenLayers 기반이라 MapLibre 대신 OL로 렌더(벡터-래스터 PNG는 CORS 허용).
async fn run_vector(g: &GlobalArgs, a: &MapArgs, key: &str, _domain: &str) -> Result<()> {
    // 중심 좌표: --address(geocode) > --center("lon,lat") > 서울 기본.
    let (lon, lat) = if let Some(addr) = &a.address {
        super::data_cmds::geocode_point(g, addr, "EPSG:4326").await?
    } else if let Some(c) = &a.center {
        let parts: Vec<&str> = c.split(',').map(str::trim).collect();
        match parts.as_slice() {
            [lo, la] if !lo.is_empty() && !la.is_empty() => (lo.to_string(), la.to_string()),
            _ => return Err(anyhow!("중심 좌표 형식 오류: '{c}' (예: --center 127.02,37.50)")),
        }
    } else {
        ("126.9780".to_string(), "37.5665".to_string())
    };
    let html = VECTOR_HTML
        .replace("__KEY__", key)
        .replace("__LON__", &lon)
        .replace("__LAT__", &lat)
        .replace("__ZOOM__", &a.zoom.to_string());
    if let Some(path) = &a.output {
        let saved = output::save_bytes(path, html.as_bytes())?;
        return output::print_json(
            g,
            &serde_json::json!({"ok": true, "kind": "vector", "center": format!("{lon},{lat}"), "saved": saved}),
        );
    }
    if g.raw {
        return output::print_raw_text(&html);
    }
    output::print_json(
        g,
        &serde_json::json!({"ok": true, "kind": "vector", "center": format!("{lon},{lat}"), "html": html}),
    )
}

/// 벡터 지도 뷰어 HTML 템플릿. `__KEY__`/`__LON__`/`__LAT__`/`__ZOOM__` 치환. `{z}{x}{y}`는 OL XYZ 리터럴.
const VECTOR_HTML: &str = r##"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>VWorld 벡터 지도</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/gh/orioncactus/pretendard@latest/dist/web/static/pretendard.min.css">
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/ol@9/ol.css">
<script src="https://cdn.jsdelivr.net/npm/ol@9/dist/ol.js"></script>
<style>
:root{--tb:#3182f6;--tbd:#2272eb;--ink:#191f28;--sub:#8b95a1;--line:#e5e8eb}
html,body{margin:0;padding:0;height:100%}
#vmap{position:absolute;inset:0;width:100vw;height:100vh;background:#eaeef2}
.ol-zoom{top:auto!important;bottom:28px!important;left:auto!important;right:24px!important;background:transparent!important}
.ol-zoom button{background:var(--tb)!important;color:#fff!important;border-radius:12px!important;width:44px!important;height:44px!important;font:700 20px Pretendard,sans-serif!important;margin:4px 0!important;box-shadow:0 3px 10px rgba(49,130,246,.3)!important;border:0!important}
.ol-zoom button:hover{background:var(--tbd)!important}
.ol-attribution{font-family:Pretendard,sans-serif!important}
#vwSearch{position:fixed;top:22px;left:50%;transform:translateX(-50%);z-index:100000;display:flex;align-items:center;gap:6px;background:#fff;border:1px solid var(--line);border-radius:18px;box-shadow:0 8px 28px rgba(0,0,0,.16);padding:9px 9px 9px 16px;width:min(540px,92vw);box-sizing:border-box}
#vwSearchInput{flex:1;min-width:0;border:0;outline:0;background:transparent;font:500 16px Pretendard,sans-serif;color:var(--ink)}
#vwSearchBtn{flex:0 0 auto;background:var(--tb);color:#fff;border:0;border-radius:12px;padding:10px 18px;font:600 15px Pretendard,sans-serif;cursor:pointer;box-shadow:0 2px 8px rgba(49,130,246,.3)}
#vwSearchBtn:hover{background:var(--tbd)}
#vwSearchToast{position:fixed;top:78px;left:50%;transform:translateX(-50%);z-index:100000;background:var(--ink);color:#fff;padding:10px 16px;border-radius:12px;font:500 14px Pretendard,sans-serif;box-shadow:0 6px 20px rgba(0,0,0,.25);display:none}
#vwLayers{position:fixed;left:24px;bottom:28px;z-index:100000;display:flex;flex-direction:column;gap:8px}
.vwChip{background:#fff;color:var(--sub);border:1px solid var(--line);border-radius:14px;padding:10px 16px;font:600 14px Pretendard,sans-serif;cursor:pointer;box-shadow:0 4px 14px rgba(0,0,0,.12);display:flex;align-items:center;gap:7px;transition:all .15s}
.vwChip::before{content:'';width:9px;height:9px;border-radius:50%;background:var(--line)}
.vwChip:hover{border-color:var(--tb);color:var(--ink)}
.vwChip.on{background:var(--tb);color:#fff;border-color:var(--tb);box-shadow:0 4px 14px rgba(49,130,246,.4)}
.vwChip.on::before{background:#fff}
.vwChip[data-layer=poi].on{background:#f04452;border-color:#f04452;box-shadow:0 4px 14px rgba(240,68,82,.4)}
#vwBasemap{position:fixed;bottom:28px;left:50%;transform:translateX(-50%);z-index:100000;display:flex;gap:0;background:#fff;border:1px solid var(--line);border-radius:14px;box-shadow:0 4px 14px rgba(0,0,0,.12);padding:4px;overflow:hidden}
.vwSeg{background:transparent;color:var(--sub);border:0;border-radius:10px;padding:8px 14px;font:600 14px Pretendard,sans-serif;cursor:pointer;transition:all .15s;white-space:nowrap}
.vwSeg:hover{color:var(--ink)}
.vwSeg.on{background:var(--tb);color:#fff;box-shadow:0 2px 6px rgba(49,130,246,.35)}
#vwTip{position:fixed;display:none;z-index:100001;background:var(--ink);color:#fff;padding:6px 12px;border-radius:10px;font:600 13px Pretendard,sans-serif;box-shadow:0 4px 14px rgba(0,0,0,.3);pointer-events:none;white-space:nowrap}
</style>
</head>
<body>
<div id="vmap"></div>
<div id="vwSearch">
<svg width="20" height="20" viewBox="0 0 24 24" fill="none" style="flex:0 0 auto"><circle cx="11" cy="11" r="7" stroke="#8b95a1" stroke-width="2"/><path d="M20 20l-3.6-3.6" stroke="#8b95a1" stroke-width="2" stroke-linecap="round"/></svg>
<input id="vwSearchInput" type="text" placeholder="주소 검색 (예: 서울특별시청, 세종대로 110)">
<button id="vwSearchBtn" type="button">검색</button>
</div>
<div id="vwSearchToast"></div>
<div id="vwTip"></div>
<div id="vwLayers">
<button class="vwChip" data-layer="roads" type="button">도로</button>
<button class="vwChip" data-layer="poi" type="button">POI</button>
</div>
<div id="vwBasemap">
<button class="vwSeg on" data-base="vector" type="button">일반</button>
<button class="vwSeg" data-base="satellite" type="button">위성</button>
<button class="vwSeg" data-base="hybrid" type="button">하이브리드</button>
</div>
<script>
var VW_KEY='__KEY__';
// 베이스맵: 벡터 Base는 {z}/{x}/{y}, 표준 WMTS(위성/하이브리드)는 {z}/{y}/{x}(좌표순서 반대)
var baseLayer=new ol.layer.Tile({ source:new ol.source.XYZ({ url:'https://api.vworld.kr/req/wmts/vector/__KEY__/Base/{z}/{x}/{y}.png', maxZoom:19, crossOrigin:'anonymous', attributions:'© VWorld' }) });
var satLayer=new ol.layer.Tile({ visible:false, source:new ol.source.XYZ({ url:'https://api.vworld.kr/req/wmts/1.0.0/__KEY__/Satellite/{z}/{y}/{x}.jpeg', maxZoom:19, crossOrigin:'anonymous' }) });
var hybLayer=new ol.layer.Tile({ visible:false, source:new ol.source.XYZ({ url:'https://api.vworld.kr/req/wmts/1.0.0/__KEY__/Hybrid/{z}/{y}/{x}.png', maxZoom:19, crossOrigin:'anonymous' }) });
var roadsLayer=new ol.layer.VectorTile({ visible:false, source:new ol.source.VectorTile({ format:new ol.format.MVT(), url:'https://api.vworld.kr/req/wmts/vector/getTile/__KEY__/traffic/{z}/{x}/{y}.pbf', maxZoom:19 }), style:[ new ol.style.Style({ stroke:new ol.style.Stroke({color:'rgba(255,255,255,0.7)', width:4}) }), new ol.style.Style({ stroke:new ol.style.Stroke({color:'#3182f6', width:1.8}) }) ] });
var poiLayer=new ol.layer.VectorTile({ visible:false, declutter:true, source:new ol.source.VectorTile({ format:new ol.format.MVT(), url:'https://api.vworld.kr/req/wmts/vector/getTile/__KEY__/poi/{z}/{x}/{y}.pbf', maxZoom:19 }), style:new ol.style.Style({ image:new ol.style.Circle({radius:4, fill:new ol.style.Fill({color:'#f04452'}), stroke:new ol.style.Stroke({color:'#fff',width:1.5})}) }) });
var LYR={roads:roadsLayer, poi:poiLayer};
var map=new ol.Map({
  target:'vmap',
  layers:[ baseLayer, satLayer, hybLayer, roadsLayer, poiLayer ],
  view:new ol.View({ center: ol.proj.fromLonLat([__LON__, __LAT__]), zoom: __ZOOM__ })
});
function setBase(bs){
  baseLayer.setVisible(bs==='vector');
  satLayer.setVisible(bs==='satellite'||bs==='hybrid');
  hybLayer.setVisible(bs==='hybrid');
  [].forEach.call(document.querySelectorAll('.vwSeg'), function(s){ s.classList.toggle('on', s.getAttribute('data-base')===bs); });
}
[].forEach.call(document.querySelectorAll('.vwSeg'), function(s){ s.addEventListener('click', function(){ setBase(s.getAttribute('data-base')); }); });
// POI hover 라벨(이름+분류 툴팁)
var vwTip=document.getElementById('vwTip');
map.on('pointermove', function(evt){
  if(evt.dragging || !poiLayer.getVisible()){ vwTip.style.display='none'; map.getTargetElement().style.cursor=''; return; }
  var hit=null;
  map.forEachFeatureAtPixel(evt.pixel, function(f){ hit=f; return true; }, { layerFilter:function(l){ return l===poiLayer; }, hitTolerance:5 });
  if(hit){
    var nm=hit.get('poi_nm')||hit.get('poi_eprss_nm');
    if(nm){
      var cat=hit.get('mlsfc_nm')||hit.get('lclas_nm')||'';
      vwTip.innerHTML='<b>'+nm+'</b>'+(cat?' <span style=\"opacity:.65;font-weight:500\">'+cat+'</span>':'');
      vwTip.style.display='block';
      vwTip.style.left=(evt.originalEvent.clientX+12)+'px';
      vwTip.style.top=(evt.originalEvent.clientY+12)+'px';
      map.getTargetElement().style.cursor='pointer';
      return;
    }
  }
  vwTip.style.display='none';
  map.getTargetElement().style.cursor='';
});
[].forEach.call(document.querySelectorAll('.vwChip'), function(chip){
  chip.addEventListener('click', function(){
    var ly=LYR[chip.getAttribute('data-layer')]; if(!ly) return;
    var on=!ly.getVisible(); ly.setVisible(on); chip.classList.toggle('on', on);
  });
});
(function(){
  function toast(m){ var t=document.getElementById('vwSearchToast'); t.textContent=m; t.style.display='block'; clearTimeout(t._h); t._h=setTimeout(function(){t.style.display='none';},2600); }
  function moveTo(lon,lat){ map.getView().animate({ center: ol.proj.fromLonLat([parseFloat(lon),parseFloat(lat)]), zoom: 15, duration: 800 }); }
  function jsonp(url,cb){ var name='__vwg'+Math.floor(performance.now()*1000%1e9); window[name]=function(d){ try{cb(d);}finally{ try{delete window[name];}catch(e){} if(sc.parentNode)sc.parentNode.removeChild(sc); } }; var sc=document.createElement('script'); sc.src=url+'&callback='+name; sc.onerror=function(){ cb(null); }; document.body.appendChild(sc); }
  function geoUrl(addr,type){ return 'https://api.vworld.kr/req/address?service=address&request=getcoord&version=2.0&crs=epsg:4326&address='+encodeURIComponent(addr)+'&refine=true&simple=false&format=json&type='+type+'&key='+VW_KEY; }
  function searchUrl(addr){ return 'https://api.vworld.kr/req/search?service=search&request=search&version=2.0&crs=EPSG:4326&size=1&page=1&query='+encodeURIComponent(addr)+'&type=address&format=json&key='+VW_KEY; }
  function pt(d){ try{ if(d&&d.response&&d.response.status==='OK'&&d.response.result&&d.response.result.point) return d.response.result.point; }catch(e){} return null; }
  function run(){
    var addr=(document.getElementById('vwSearchInput').value||'').trim(); if(!addr) return;
    jsonp(geoUrl(addr,'road'), function(d){ var p=pt(d); if(p){ moveTo(p.x,p.y); return; }
      jsonp(geoUrl(addr,'parcel'), function(d2){ var p2=pt(d2); if(p2){ moveTo(p2.x,p2.y); return; }
        jsonp(searchUrl(addr), function(d3){ try{ var it=d3.response.result.items[0]; if(it&&it.point){ moveTo(it.point.x,it.point.y); return; } }catch(e){} toast('주소를 찾을 수 없습니다: '+addr); });
      });
    });
  }
  document.getElementById('vwSearchBtn').addEventListener('click',run);
  document.getElementById('vwSearchInput').addEventListener('keydown',function(e){ if(e.key==='Enter'){ e.preventDefault(); run(); } });
})();
</script>
</body>
</html>
"##;

/// WFS GeoJSON을 토스 디자인 지도(Leaflet + vworld 타일)에 오버레이하는 뷰어 HTML.
/// vworld 2D(OpenLayers 3.10.1) 내부 접근자 불확실성을 피해 Leaflet로 구현 — GeoJSON 표준 [lon,lat] 그대로.
pub fn render_wfs_viewer(geojson: &str, bbox: Option<&str>, key: &str, _domain: &str) -> String {
    // GeoJSON 안전 주입(속성 문자열의 </script> 차단).
    let safe = geojson.replace("</", "<\\/");
    // bbox "minx,miny,maxx,maxy" → fitBounds([[miny,minx],[maxy,maxx]]).
    let fit = bbox
        .and_then(|b| {
            let p: Vec<f64> = b.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if p.len() == 4 {
                Some(format!("map.fitBounds([[{},{}],[{},{}]]);", p[1], p[0], p[3], p[2]))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "map.setView([37.55,126.98],11);".into());
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>VWorld WFS 뷰어</title>
  <link rel="stylesheet" href="https://unpkg.com/leaflet@1.9.4/dist/leaflet.css"/>
  <script src="https://unpkg.com/leaflet@1.9.4/dist/leaflet.js"></script>
{TOSS_STYLE}
</head>
<body>
  <div id="vmap"></div>
  <div id="result"><h3>WFS 결과</h3><p id="cnt">로딩…</p></div>
  <script>
    var data = {safe};
    var map = L.map('vmap');
    L.tileLayer('https://api.vworld.kr/req/wmts/1.0.0/{key}/Base/{{z}}/{{y}}/{{x}}.png',
      {{maxZoom:19, attribution:'&copy; VWorld'}}).addTo(map);
    {fit}
    var layer = L.geoJSON(data, {{
      style: {{color:'#3182f6', weight:2, fillColor:'#3182f6', fillOpacity:0.15}},
      pointToLayer: function(f, ll){{ return L.circleMarker(ll, {{radius:6, color:'#3182f6', fillColor:'#3182f6', fillOpacity:0.6}}); }},
      onEachFeature: function(f, l){{
        if(f && f.properties){{
          var ks = Object.keys(f.properties).slice(0,14);
          var rows = ks.map(function(k){{ return '<tr><td style="color:#8b95a1;padding-right:14px">'+k+'</td><td><b>'+(f.properties[k]==null?'':f.properties[k])+'</b></td></tr>'; }}).join('');
          l.bindPopup('<table style="font:13px Pretendard,sans-serif">'+rows+'</table>');
        }}
      }}
    }}).addTo(map);
    var n = (data.features||[]).length;
    var tot = data.totalFeatures!=null ? ' (전체 '+data.totalFeatures+')' : '';
    document.getElementById('cnt').innerHTML = '총 <b>'+n+'</b>개 피처'+tot+' — 클릭하면 속성을 봅니다.';
    if(n>0){{ try{{ map.fitBounds(layer.getBounds(), {{maxZoom:16, padding:[20,20]}}); }}catch(e){{}} }}
  </script>
</body>
</html>
"#
    )
}

// ─────────────────────────────────────────────────────────────────────────
// 단계1: ol3(OpenLayers 2D) 데이터레이어 렌더러 (계획 plan/2026-06-18-09:30:53-2dmap-19samples.md)
// ─────────────────────────────────────────────────────────────────────────

/// 키(+도메인) 자격증명 문자열 — vworldMapInit.js 쿼리용.
fn ol_cred(key: &str, domain: &str) -> String {
    if domain.is_empty() {
        key.to_string()
    } else {
        format!("{key}&domain={domain}")
    }
}

/// 배경지도 종류 검증(기본 GRAPHIC). vw.ol3.BasemapType enum 키 반환.
fn ol_basemap(opt: Option<&str>) -> Result<String> {
    let up = opt.unwrap_or("GRAPHIC").to_uppercase();
    const OK: [&str; 5] = ["GRAPHIC", "GRAPHIC_WHITE", "GRAPHIC_NIGHT", "PHOTO", "PHOTO_HYBRID"];
    if OK.contains(&up.as_str()) {
        Ok(up)
    } else {
        Err(anyhow!(
            "알 수 없는 배경지도: {up} (GRAPHIC|GRAPHIC_WHITE|GRAPHIC_NIGHT|PHOTO|PHOTO_HYBRID)"
        ))
    }
}

/// "lon,lat" 문자열 파싱(미지정 시 기본 127.0,37.5).
fn parse_center_str(c: Option<&str>) -> Result<(f64, f64)> {
    let c = c.unwrap_or("127.0,37.5");
    let p: Vec<&str> = c.split(',').map(str::trim).collect();
    match p.as_slice() {
        [lon, lat] => {
            let lon: f64 = lon.parse().map_err(|_| anyhow!("중심 좌표 형식 오류: '{c}' (예: --center 127.02,37.50)"))?;
            let lat: f64 = lat.parse().map_err(|_| anyhow!("중심 좌표 형식 오류: '{c}' (예: --center 127.02,37.50)"))?;
            Ok((lon, lat))
        }
        _ => Err(anyhow!("중심 좌표 형식 오류: '{c}' (예: --center 127.02,37.50)")),
    }
}

/// ol 모드 중심 결정: --address(geocode) > --center > 기본.
async fn ol_center(g: &GlobalArgs, a: &MapArgs) -> Result<(f64, f64)> {
    if let Some(addr) = &a.address {
        let (lon, lat) = super::data_cmds::geocode_point(g, addr, "EPSG:4326").await?;
        let lon: f64 = lon.parse().map_err(|_| anyhow!("geocode 좌표 파싱 실패: {lon}"))?;
        let lat: f64 = lat.parse().map_err(|_| anyhow!("geocode 좌표 파싱 실패: {lat}"))?;
        return Ok((lon, lat));
    }
    parse_center_str(a.center.as_deref())
}

/// 인라인 폴리곤 "lon,lat;lon,lat;…" 파싱(최소 3정점).
fn parse_polygon(s: &str) -> Result<Vec<(f64, f64)>> {
    let mut v = Vec::new();
    for pair in s.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let xy: Vec<&str> = pair.split(',').map(str::trim).collect();
        match xy.as_slice() {
            [lon, lat] => {
                let lon: f64 = lon.parse().map_err(|_| anyhow!("폴리곤 좌표 형식 오류: '{pair}'"))?;
                let lat: f64 = lat.parse().map_err(|_| anyhow!("폴리곤 좌표 형식 오류: '{pair}'"))?;
                v.push((lon, lat));
            }
            _ => return Err(anyhow!("폴리곤 좌표 형식 오류: '{pair}' (예: 127.0,37.5;127.1,37.5;127.1,37.6)")),
        }
    }
    if v.len() < 3 {
        return Err(anyhow!("폴리곤은 최소 3개 정점이 필요합니다(현재 {}개)", v.len()));
    }
    Ok(v)
}

/// 경로 "lon,lat;lon,lat;…" 파싱(최소 2점).
fn parse_route(s: &str) -> Result<Vec<(f64, f64)>> {
    let mut v = Vec::new();
    for pair in s.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let xy: Vec<&str> = pair.split(',').map(str::trim).collect();
        match xy.as_slice() {
            [lon, lat] => {
                let lon: f64 = lon.parse().map_err(|_| anyhow!("경로 좌표 형식 오류: '{pair}'"))?;
                let lat: f64 = lat.parse().map_err(|_| anyhow!("경로 좌표 형식 오류: '{pair}'"))?;
                v.push((lon, lat));
            }
            _ => return Err(anyhow!("경로 좌표 형식 오류: '{pair}' (예: 127.0,37.5;127.1,37.55;127.2,37.6)")),
        }
    }
    if v.len() < 2 {
        return Err(anyhow!("경로는 최소 2개 점이 필요합니다(현재 {}개)", v.len()));
    }
    Ok(v)
}

/// ol3 지도 초기화 JS(공식 샘플 패턴) — CameraPosition + MapOptions + vw.ol3.Map 생성.
/// 좌표는 JS측 `ol.proj.fromLonLat`로 4326→3857 변환(계획 §4 통일).
fn ol2d_init_js(lon: f64, lat: f64, zoom: u32, basemap: &str, interaction: &str) -> String {
    format!(
        "  vw.ol3.CameraPosition.center = ol.proj.fromLonLat([{lon},{lat}]);\n\
         \x20 vw.ol3.CameraPosition.zoom = {zoom};\n\
         \x20 vw.ol3.MapOptions = {{ basemapType: vw.ol3.BasemapType.{basemap}, controlDensity: vw.ol3.DensityType.EMPTY, interactionDensity: vw.ol3.DensityType.{interaction}, controlsAutoArrange: true, homePosition: vw.ol3.CameraPosition, initPosition: vw.ol3.CameraPosition }};\n\
         \x20 var vmap = new vw.ol3.Map(\"vmap\", vw.ol3.MapOptions);\n"
    )
}

/// 토스(#3182f6) 벡터 스타일 + extent 맞춤 헬퍼 JS — 폴리곤/포인트/라인 공통.
/// vw.ol3.Map의 `getView().fit()`이 maxZoom을 무시하고 과확대되므로(단계1 검증),
/// setCenter+setResolution으로 직접 extent에 맞춘다.
const TOSS_VECTOR_STYLE_JS: &str = "  function tossVectorStyle(){ return new ol.style.Style({ stroke: new ol.style.Stroke({color:'#3182f6', width:3}), fill: new ol.style.Fill({color:'rgba(49,130,246,0.15)'}), image: new ol.style.Circle({radius:7, fill:new ol.style.Fill({color:'#3182f6'}), stroke:new ol.style.Stroke({color:'#fff',width:2})}) }); }\n  function vwFit(vmap, ext){ try{ if(!ext || !isFinite(ext[0])) return; var v=vmap.getView(), s=vmap.getSize()||[800,600]; var cx=(ext[0]+ext[2])/2, cy=(ext[1]+ext[3])/2; v.setCenter([cx,cy]); var w=ext[2]-ext[0], h=ext[3]-ext[1]; if(w<=0&&h<=0) return; var r=Math.max(w/Math.max(s[0]-80,1), h/Math.max(s[1]-80,1))*1.15; if(r>0) v.setResolution(r); }catch(e){} }\n";

/// KML 레이어용 토스(#3182f6) 스타일 함수 JS — 점/선/면 공통(원 마커 + 선/면).
const TOSS_KML_STYLE_JS: &str = "  function tossKmlStyle(feature, resolution){ return [new ol.style.Style({ image:new ol.style.Circle({radius:7, fill:new ol.style.Fill({color:'rgba(49,130,246,0.6)'}), stroke:new ol.style.Stroke({color:'#fff',width:2})}), stroke:new ol.style.Stroke({color:'#3182f6',width:3}), fill:new ol.style.Fill({color:'rgba(49,130,246,0.15)'}) })]; }\n";

/// ol3 전체 HTML 골격 — vworldMapInit.js(절대경로) + TOSS_STYLE 주입.
fn ol2d_shell(key: &str, domain: &str, title: &str, controls_html: &str, body_js: &str) -> String {
    let cred = ol_cred(key, domain);
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title}</title>
  <script src="https://map.vworld.kr/js/vworldMapInit.js.do?version=2.0&apiKey={cred}"></script>
</head>
<body>
  <div id="vmap"></div>
{controls_html}  <script>
  // vw.ol3.Map은 vworldMapInit.js의 비동기 런타임 로드 완료 후 초기화해야
  // 벡터(ol.layer.Vector)·컨트롤이 렌더 파이프라인에 정상 등록된다(단계0 S1 검증).
  window.addEventListener('load', function(){{
{body_js}  }});
  </script>
</body>
</html>
"#
    );
    html.replace("</head>", &format!("{TOSS_STYLE}</head>"))
}

/// 생성 HTML 출력(파일 저장 / raw / JSON) 공통 처리.
fn output_html(g: &GlobalArgs, a: &MapArgs, html: String, mut meta: serde_json::Value) -> Result<()> {
    let obj = meta.as_object_mut().expect("meta는 객체");
    obj.insert("ok".into(), serde_json::json!(true));
    if let Some(path) = &a.output {
        let saved = output::save_bytes(path, html.as_bytes())?;
        obj.insert("saved".into(), serde_json::json!(saved));
        return output::print_json(g, &meta);
    }
    if g.raw {
        return output::print_raw_text(&html);
    }
    obj.insert("html".into(), serde_json::json!(html));
    output::print_json(g, &meta)
}

/// `map ol` — ol3 기본 2D 지도 + 컨트롤 플래그 + 벡터(--polygon/--geojson).
async fn run_ol(g: &GlobalArgs, a: &MapArgs, key: &str, domain: &str) -> Result<()> {
    let (lon, lat) = ol_center(g, a).await?;
    let bm = ol_basemap(a.basemap.as_deref())?;
    let interaction = if a.popup { "FULL" } else { "BASIC" };
    let mut js = ol2d_init_js(lon, lat, a.zoom, &bm, interaction);
    let mut controls = String::new();
    let has_vector = a.polygon.is_some() || a.geojson.is_some() || a.route.is_some();
    if has_vector {
        js.push_str(TOSS_VECTOR_STYLE_JS);
    }
    if a.kml.is_some() {
        js.push_str(TOSS_KML_STYLE_JS);
    }

    // 미니맵(OverviewMap).
    if a.overview {
        js.push_str("  var _ov = new vw.ol3.control.OverviewMap(vmap); _ov.site = \"top-right\"; _ov.draw();\n");
    }
    // 측정 툴바(ToolBar).
    if a.toolbar {
        js.push_str("  (function(){ var _o={map:vmap, site:vw.ol3.SiteAlignType.TOP_LEFT, vertical:true, collapsed:false, collapsible:false}; var _b=[new vw.ol3.button.Init(vmap), new vw.ol3.button.ZoomIn(vmap), new vw.ol3.button.ZoomOut(vmap), new vw.ol3.button.DragZoomIn(vmap), new vw.ol3.button.DragZoomOut(vmap), new vw.ol3.button.Pan(vmap), new vw.ol3.button.Prev(vmap), new vw.ol3.button.Next(vmap), new vw.ol3.button.Full(vmap), new vw.ol3.button.Distance(vmap), new vw.ol3.button.Area(vmap)]; var _t=new vw.ol3.control.Toolbar(_o); _t.addToolButtons(_b); vmap.addControl(_t); })();\n");
    }

    // PanZoomBar(줌 슬라이더) 컨트롤.
    if a.zoom_control {
        js.push_str("  var _zoom = new vw.ol3.control.Zoom(vmap); _zoom.delta=1; _zoom.sliderVisible=true; _zoom.site=vw.ol3.SiteAlignType.CENTER_RIGHT; _zoom.draw(); vmap.addControl(_zoom);\n");
    }
    // 클릭 좌표 팝업.
    if a.popup {
        js.push_str("  var _pop=null; vmap.on('click', function(evt){ if(_pop==null){ _pop=new vw.ol3.popup.Popup(); vmap.addOverlay(_pop); } var c=ol.proj.toLonLat(evt.coordinate); _pop.title='좌표'; _pop.content=c[0].toFixed(6)+', '+c[1].toFixed(6); _pop.show(_pop.content, evt.coordinate); });\n");
    }
    // 배경지도 전환 버튼.
    if a.basemap_switch {
        controls.push_str(r#"  <div id="ctrls"><button type="button" onclick="setBasemap('GRAPHIC')">배경지도</button><button type="button" onclick="setBasemap('GRAPHIC_WHITE')">백지도</button><button type="button" onclick="setBasemap('GRAPHIC_NIGHT')">야간지도</button><button type="button" onclick="setBasemap('PHOTO')">항공사진</button><button type="button" onclick="setBasemap('PHOTO_HYBRID')">하이브리드</button></div>
"#);
        js.push_str("  window.setBasemap=function(t){ vmap.setBasemapType(vw.ol3.BasemapType[t]); };\n");
    }
    // 인라인 폴리곤(--polygon) — GeoJSON 경로로 통일(검증된 readFeatures 재사용).
    if let Some(poly) = &a.polygon {
        let mut coords = parse_polygon(poly)?;
        if coords.first() != coords.last() {
            coords.push(coords[0]); // 링 닫기.
        }
        let ring = coords.iter().map(|(x, y)| format!("[{x},{y}]")).collect::<Vec<_>>().join(",");
        let gj = format!(
            r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","properties":{{}},"geometry":{{"type":"Polygon","coordinates":[[{ring}]]}}}}]}}"#
        );
        js.push_str(&format!(
            "  (function(){{ var gj={gj}; var feats=new ol.format.GeoJSON().readFeatures(gj,{{dataProjection:'EPSG:4326',featureProjection:'EPSG:3857'}}); var vs=new ol.source.Vector({{features:feats}}); vmap.addLayer(new ol.layer.Vector({{source:vs,style:tossVectorStyle()}})); vwFit(vmap, vs.getExtent()); }})();\n"
        ));
    }
    // 범용 GeoJSON(--geojson).
    if let Some(path) = &a.geojson {
        let raw = std::fs::read_to_string(path).map_err(|e| anyhow!("GeoJSON 파일 읽기 실패 {}: {e}", path.display()))?;
        let _: serde_json::Value = serde_json::from_str(&raw).map_err(|e| anyhow!("GeoJSON 파싱 실패: {e}"))?;
        let safe = raw.replace("</", "<\\/");
        js.push_str(&format!(
            "  (function(){{ var gj={safe}; var feats=new ol.format.GeoJSON().readFeatures(gj,{{dataProjection:'EPSG:4326',featureProjection:'EPSG:3857'}}); var vs=new ol.source.Vector({{features:feats}}); vmap.addLayer(new ol.layer.Vector({{source:vs,style:tossVectorStyle()}})); if(feats.length){{ vwFit(vmap, vs.getExtent()); }} }})();\n"
        ));
    }

    // 사전 경로(--route) — RouteMap 도구 + 폴리라인 정적 렌더(폴백 보장).
    if let Some(route) = &a.route {
        let coords = parse_route(route)?;
        let line = coords.iter().map(|(x, y)| format!("[{x},{y}]")).collect::<Vec<_>>().join(",");
        js.push_str(&format!(
            "  (function(){{ var pts=[{line}].map(function(c){{return ol.proj.fromLonLat(c);}}); \
             var lf=new ol.Feature({{geometry:new ol.geom.LineString(pts)}}); \
             var lvs=new ol.source.Vector({{features:[lf]}}); \
             vmap.addLayer(new ol.layer.Vector({{source:lvs,style:tossVectorStyle()}})); \
             vwFit(vmap, lvs.getExtent()); \
             try{{ var _rm=new vw.ol3.control.RouteMap(vmap, \"route1\", null, \"https://map.vworld.kr/images/maps/marker.png\"); window._routeMap=_rm; }}catch(e){{ console.error('RouteMap 초기화 실패', e); }} }})();\n"
        ));
    }

    // KML 레이어(--kml) — 외부 URL, 절대경로 https.
    if let Some(kml) = &a.kml {
        if !kml.starts_with("https://") && !kml.starts_with("http://") {
            return Err(anyhow!("--kml은 절대 URL(https://…)이어야 합니다(자기완결·CORS): {kml}"));
        }
        let kml_js = serde_json::to_string(kml)?;
        js.push_str(&format!(
            "  (function(){{ try{{ var kl=vmap.addKMLLayer({kml_js}, tossKmlStyle); vmap.addLayer(kl); }}catch(e){{ console.error('KML 로드 실패', e); }} }})();\n"
        ));
    }

    let html = ol2d_shell(key, domain, "VWorld 2D 지도", &controls, &js);
    output_html(g, a, html, serde_json::json!({"kind":"ol","basemap":bm,"center":format!("{lon},{lat}")}))
}

/// `map text` — vw.ol3.layer.TEXTLayer로 대량 포인트(클러스터링). 파일 내용 임베드(자기완결).
fn run_text(g: &GlobalArgs, a: &MapArgs, key: &str, domain: &str) -> Result<()> {
    let path = a.file.as_ref().ok_or_else(|| anyhow!("text는 --file <points.txt> 필요"))?;
    let raw = std::fs::read_to_string(path).map_err(|e| anyhow!("TEXT 파일 읽기 실패 {}: {e}", path.display()))?;
    let lines = raw.lines().filter(|l| !l.trim().is_empty()).count();
    const MAX_LINES: usize = 500;
    if lines > MAX_LINES {
        return Err(anyhow!(
            "TEXTLayer 자기완결 임베드 상한 초과: {lines}줄(최대 {MAX_LINES}). 파일을 분할하거나 줄 수를 줄이세요(§9.3 자기완결 우선)"
        ));
    }
    let epsg = a.epsg.as_deref().unwrap_or("EPSG:4326");
    let (lon, lat) = parse_center_str(a.center.as_deref())?;
    let bm = ol_basemap(a.basemap.as_deref())?;
    // 텍스트를 안전한 JS 문자열로 직렬화(+ </script> 조기 종료 방지).
    let txt_js = serde_json::to_string(&raw)?.replace("</", "<\\/");
    let epsg_js = serde_json::to_string(epsg)?;
    let js = format!(
        "{init}  var _txt={txt_js};\n  var _tl=new vw.ol3.layer.TEXTLayer(vmap, {epsg_js}); _tl.readDraw({epsg_js}, {distance}, _txt);\n",
        init = ol2d_init_js(lon, lat, a.zoom, &bm, "BASIC"),
        distance = a.distance
    );
    let html = ol2d_shell(key, domain, "VWorld 대량 포인트", "", &js);
    output_html(g, a, html, serde_json::json!({"kind":"text","epsg":epsg,"distance":a.distance,"lines":lines}))
}

/// `map controller` — vw.MapController(2D/3D 전환). vw.ol3.Map과 다른 진입점이라 전용 init.
fn run_controller(g: &GlobalArgs, a: &MapArgs, key: &str, domain: &str) -> Result<()> {
    let (lon, lat) = parse_center_str(a.center.as_deref())?;
    let bm = ol_basemap(a.basemap.as_deref())?;
    let zoom = a.zoom;
    // MapController는 vw.MapController(option) 진입점. CameraPosition은 ol.proj로 4326→3857 변환.
    let js = format!(
        "  vw.ol3.CameraPosition.center = ol.proj.fromLonLat([{lon},{lat}]);\n\
         \x20 vw.ol3.CameraPosition.zoom = {zoom};\n\
         \x20 vw.MapControllerOption = {{ container: \"vmap\", mapMode: \"2d-map\", basemapType: vw.ol3.BasemapType.{bm}, controlDensity: vw.ol3.DensityType.EMPTY, interactionDensity: vw.ol3.DensityType.BASIC, controlsAutoArrange: true, homePosition: vw.ol3.CameraPosition, initPosition: vw.ol3.CameraPosition }};\n\
         \x20 window.mapController = new vw.MapController(vw.MapControllerOption);\n"
    );
    let controls = "  <div id=\"ctrls\"><label>지도 모드 </label><select onchange=\"mapController.setMode(this.value)\"><option value=\"2d-map\">2D 지도</option><option value=\"3d-map\">3D 지도</option></select></div>\n".to_string();
    let html = ol2d_shell(key, domain, "VWorld 2D/3D 전환", &controls, &js);
    output_html(g, a, html, serde_json::json!({"kind":"controller","basemap":bm,"center":format!("{lon},{lat}")}))
}

/// `map marker` — vw.ol3.layer.Marker로 마커+팝업(epsg 파라미터로 4326 그대로).
fn run_marker(g: &GlobalArgs, a: &MapArgs, key: &str, domain: &str) -> Result<()> {
    let path = a.points.as_ref().ok_or_else(|| anyhow!("marker는 --points <markers.json> 필요"))?;
    let raw = std::fs::read_to_string(path).map_err(|e| anyhow!("마커 파일 읽기 실패 {}: {e}", path.display()))?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| anyhow!("마커 JSON 파싱 실패: {e}"))?;
    if !parsed.is_array() {
        return Err(anyhow!("마커 JSON은 배열이어야 합니다: [{{x,y,epsg?,title,contents,…}}]"));
    }
    let count = parsed.as_array().map(|a| a.len()).unwrap_or(0);
    let safe = raw.replace("</", "<\\/");
    let (lon, lat) = parse_center_str(a.center.as_deref())?;
    let bm = ol_basemap(a.basemap.as_deref())?;
    let js = format!(
        "{init}  var _pts={safe};\n\
         \x20 var _ml=new vw.ol3.layer.Marker(vmap); vmap.addLayer(_ml);\n\
         \x20 (Array.isArray(_pts)?_pts:[]).forEach(function(p){{ vw.ol3.markerOption={{ x:p.x, y:p.y, epsg:p.epsg||'EPSG:4326', title:p.title||'', contents:p.contents||'', iconUrl:p.iconUrl||'https://map.vworld.kr/images/ol3/marker_blue.png', text:p.text, attr:p.attr||{{}} }}; _ml.addMarker(vw.ol3.markerOption); }});\n",
        init = ol2d_init_js(lon, lat, a.zoom, &bm, "FULL")
    );
    let html = ol2d_shell(key, domain, "VWorld 마커", "", &js);
    output_html(g, a, html, serde_json::json!({"kind":"marker","count":count}))
}

/// `map chart` — vw.ol3.chart.{Bar|StackedBar|Pie} 오버레이(+범례, --group ChartGroup).
fn run_chart(g: &GlobalArgs, a: &MapArgs, key: &str, domain: &str) -> Result<()> {
    let path = a.data.as_ref().ok_or_else(|| anyhow!("chart는 --data <chart.json> 필요"))?;
    let ctype = a.chart_type.as_deref().unwrap_or("bar").to_lowercase();
    let cls = match ctype.as_str() {
        "bar" => "Bar",
        "stackedbar" => "StackedBar",
        "pie" => "Pie",
        o => return Err(anyhow!("알 수 없는 차트 종류: {o} (bar|stackedbar|pie)")),
    };
    let is_pie = cls == "Pie";
    let raw = std::fs::read_to_string(path).map_err(|e| anyhow!("차트 파일 읽기 실패 {}: {e}", path.display()))?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| anyhow!("차트 JSON 파싱 실패: {e}"))?;
    if !parsed.is_array() {
        return Err(anyhow!("차트 JSON은 배열이어야 합니다: [{{pos:[lon,lat],title,styles,values,…}}]"));
    }
    let count = parsed.as_array().map(|a| a.len()).unwrap_or(0);
    let safe = raw.replace("</", "<\\/");
    let (lon, lat) = parse_center_str(a.center.as_deref())?;
    let bm = ol_basemap(a.basemap.as_deref())?;
    let sizearg = if is_pie { "(c.radius||60)" } else { "(c.size||[100,100])" };
    let draw = if a.group {
        "  if(_charts.length){ var _gc=new vw.ol3.chart.ChartGroup(vmap); _charts.forEach(function(c){_gc.chartList.push(c);}); _gc.styles=_charts[0].styles; _gc.legend=_charts[0].legend; _gc.draw(vw.ol3.SiteAlignType.top_right,[100,200]); }\n"
    } else {
        "  _charts.forEach(function(c){ c.draw(); });\n"
    };
    let js = format!(
        "{init}  var _data={safe};\n\
         \x20 var _charts=[];\n\
         \x20 (Array.isArray(_data)?_data:[]).forEach(function(c){{ var ch=new vw.ol3.chart.{cls}({sizearg}); ch.title=c.title||''; ch.legend=new vw.ol3.chart.ChartLegend(); ch.legend.visible=true; ch.styles=c.styles||[]; ch.values=c.values||[]; ch.setPosition(ol.proj.fromLonLat(c.pos)); vmap.addOverlay(ch); _charts.push(ch); }});\n{draw}",
        init = ol2d_init_js(lon, lat, a.zoom, &bm, "FULL")
    );
    let html = ol2d_shell(key, domain, "VWorld 차트", "", &js);
    output_html(g, a, html, serde_json::json!({"kind":"chart","type":ctype,"group":a.group,"count":count}))
}

/// `map theme` — WMS named layer(주제도) 추가 + 토글 버튼.
fn run_theme(g: &GlobalArgs, a: &MapArgs, key: &str, domain: &str) -> Result<()> {
    let layers = a.layers.as_ref().ok_or_else(|| anyhow!("theme는 --layers \"이름:LAYER_ID,…\" 필요"))?;
    let mut defs: Vec<(String, String)> = Vec::new();
    for part in layers.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (name, id) = part.split_once(':').ok_or_else(|| anyhow!("주제도 형식 오류: '{part}' (이름:LAYER_ID)"))?;
        defs.push((name.trim().to_string(), id.trim().to_string()));
    }
    if defs.is_empty() {
        return Err(anyhow!("주제도 레이어가 비어 있습니다: --layers \"도시지역:LT_C_UQ111\""));
    }
    let (lon, lat) = parse_center_str(a.center.as_deref())?;
    let bm = ol_basemap(a.basemap.as_deref())?;
    // JS 배열 리터럴(이름/ID JSON 이스케이프).
    let arr = defs
        .iter()
        .map(|(n, id)| {
            Ok(format!(
                "[{},{}]",
                serde_json::to_string(n)?,
                serde_json::to_string(id)?
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .join(",");
    let js = format!(
        "{init}  var _defs=[{arr}];\n\
         \x20 var _tl=[];\n\
         \x20 _defs.forEach(function(d){{ var t=vmap.addNamedLayer(d[0],d[1]); vmap.addLayer(t); _tl.push(t); }});\n\
         \x20 window.toggleTheme=function(i){{ var t=_tl[i]; if(t){{ t.setVisible(!t.getVisible()); }} }};\n",
        init = ol2d_init_js(lon, lat, a.zoom, &bm, "BASIC")
    );
    let buttons = defs
        .iter()
        .enumerate()
        .map(|(i, (n, _))| format!(r#"<button type="button" onclick="toggleTheme({i})">{n}</button>"#))
        .collect::<Vec<_>>()
        .join("");
    let controls = format!("  <div id=\"ctrls\">{buttons}</div>\n");
    let html = ol2d_shell(key, domain, "VWorld 주제도", &controls, &js);
    output_html(g, a, html, serde_json::json!({"kind":"theme","layers":defs.iter().map(|(n,i)| format!("{n}:{i}")).collect::<Vec<_>>()}))
}

fn render_html(kind: &str, script_url: &str, center: &str, zoom: u32, init: &str) -> String {
    let parts: Vec<&str> = center.split(',').collect();
    let (lon, lat) = (parts.first().copied().unwrap_or("127.0"), parts.get(1).copied().unwrap_or("37.5"));
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>VWorld {kind} 지도</title>
  <style>html,body,#vmap{{height:100%;margin:0}}</style>
  <script src="{script_url}"></script>
</head>
<body>
  <div id="vmap"></div>
  <script>
    // {init}
    // center=({lon},{lat}) zoom={zoom}
    window.addEventListener('load', function() {{
      try {{
        var opt = new vw.MapOptions(
          vw.BasemapType.GRAPHIC,
          "", vw.DensityType.FULL, vw.DensityType.BASIC,
          false,
          new vw.CameraPosition(new vw.CoordZ({lon}, {lat}, 5000), new vw.Direction(0, -90, 0)),
          new vw.CameraPosition(new vw.CoordZ({lon}, {lat}, 5000), new vw.Direction(0, -90, 0))
        );
        vw.MapControllerOption.mapMode = "{kind}";
        new vw.Map("vmap", opt);
      }} catch (e) {{ document.getElementById('vmap').innerText = 'VWorld 지도 초기화 실패: ' + e; }}
    }});
  </script>
</body>
</html>
"#
    )
}

/// choropleth 지도 — VECTOR_HTML 기반, GeoJSON 오버레이 + 색 구간 스타일.
async fn run_choropleth(g: &GlobalArgs, a: &MapArgs, key: &str, _domain: &str) -> Result<()> {
    use super::choropleth;

    let geojson_path = a.geojson.as_ref().ok_or_else(|| anyhow!("--geojson <PATH>가 필요합니다"))?;
    let value_field = a.value_field.as_deref().ok_or_else(|| anyhow!("--value-field <PROP>가 필요합니다"))?;

    let geojson_raw = std::fs::read_to_string(geojson_path)
        .map_err(|e| anyhow!("GeoJSON 파일 읽기 실패: {e}"))?;

    let geojson_val: serde_json::Value = serde_json::from_str(&geojson_raw)
        .map_err(|e| anyhow!("GeoJSON 파싱 실패: {e}"))?;

    // features에서 value_field 수치 수집 (문자열 숫자도 파싱, null/비숫자는 no-data)
    let mut values: Vec<f64> = Vec::new();
    if let Some(features) = geojson_val["features"].as_array() {
        for f in features {
            let v = &f["properties"][value_field];
            let num = match v {
                serde_json::Value::Number(n) => n.as_f64(),
                serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
                _ => None,
            };
            if let Some(n) = num {
                values.push(n);
            }
        }
    }

    // 경계값 계산
    let n = a.classes as usize;
    let breaks: Vec<f64> = if let Some(breaks_str) = &a.breaks {
        breaks_str
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .collect()
    } else {
        choropleth::compute_breaks(&values, n, &a.class_method)
    };

    let colors = choropleth::pick_colors(&a.color_scale, breaks.len() + 1);
    let color_fn_js = choropleth::gen_color_fn_js(&breaks, &colors, &a.no_data_color);

    // GeoJSON 안전 주입
    let geojson_safe = geojson_raw.replace("</", "<\\/");
    let opacity = a.opacity;
    let vf = value_field;

    // choropleth 스크립트 블록
    let choro_script = format!(
        "var CHORO_DATA = {geojson_safe};\n\
{color_fn_js}\n\
function hexA(hex,a){{hex=hex.replace('#','');if(hex.length===3)hex=hex.split('').map(function(c){{return c+c;}}).join('');var r=parseInt(hex.slice(0,2),16),g=parseInt(hex.slice(2,4),16),b=parseInt(hex.slice(4,6),16);return 'rgba('+r+','+g+','+b+','+a+')';}}\n\
var choroSource=new ol.source.Vector({{features:new ol.format.GeoJSON().readFeatures(CHORO_DATA,{{dataProjection:'EPSG:4326',featureProjection:'EPSG:3857'}})}});\n\
var choroLayer=new ol.layer.Vector({{source:choroSource,style:function(f){{var v=f.get('{vf}');v=(v==null?null:parseFloat(v));if(isNaN(v))v=null;return new ol.style.Style({{fill:new ol.style.Fill({{color:hexA(vwColor(v),{opacity})}}),stroke:new ol.style.Stroke({{color:'#333',width:1}})}});}}}});\n\
map.getLayers().insertAt(3,choroLayer);\n\
var ext=choroSource.getExtent();if(ext&&isFinite(ext[0])){{map.getView().fit(ext,{{padding:[40,40,40,40],maxZoom:14}});}}\n\
map.on('click',function(evt){{map.forEachFeatureAtPixel(evt.pixel,function(f){{var v=f.get('{vf}');var nm=f.get('adm_nm')||f.get('name')||f.get('NAME')||'';var msg=(nm?nm+': ':'')+( v!=null?v:'(값 없음)');var t=document.getElementById('vwSearchToast');if(t){{t.textContent=msg;t.style.display='block';clearTimeout(t._h);t._h=setTimeout(function(){{t.style.display='none';}},3000);}}return true;}},{{layerFilter:function(l){{return l===choroLayer;}}}});}} );"
    );

    // 범례 HTML
    let legend_html = if a.legend {
        choropleth::gen_legend_html(&breaks, &colors, value_field, &a.no_data_color)
    } else {
        String::new()
    };

    // 중심 좌표: --center 지정 시 사용, 없으면 GeoJSON extent 중심
    let (lon, lat, zoom_str) = if let Some(c) = &a.center {
        let parts: Vec<&str> = c.split(',').map(str::trim).collect();
        match parts.as_slice() {
            [lo, la] => (lo.to_string(), la.to_string(), a.zoom.to_string()),
            _ => return Err(anyhow!("중심 좌표 형식 오류: '{c}'")),
        }
    } else if let Some((minx, miny, maxx, maxy)) = choropleth::compute_geojson_extent(&geojson_raw) {
        let cx = (minx + maxx) / 2.0;
        let cy = (miny + maxy) / 2.0;
        (cx.to_string(), cy.to_string(), a.zoom.to_string())
    } else {
        ("126.978".to_string(), "37.5665".to_string(), a.zoom.to_string())
    };

    // VECTOR_HTML 기반으로 choropleth 스크립트 삽입.
    // 반드시 `var map=new ol.Map(...)` 생성 이후에 삽입해야 map.getLayers()가 동작한다.
    // map 생성 직후의 `function setBase(bs){`를 anchor로 그 앞에 주입한다.
    let html = VECTOR_HTML
        .replace("__KEY__", key)
        .replace("__LON__", &lon)
        .replace("__LAT__", &lat)
        .replace("__ZOOM__", &zoom_str)
        .replace(
            "function setBase(bs){",
            &format!("{choro_script}\n  function setBase(bs){{"),
        )
        .replace("</body>", &format!("{legend_html}</body>"));

    // --open 처리
    let open_after = a.open && a.output.is_some();

    let meta = serde_json::json!({
        "kind": "choropleth",
        "value_field": value_field,
        "classes": n,
        "data_count": values.len(),
        "colors": colors,
    });
    output_html(g, a, html, meta)?;

    if open_after {
        if let Some(path) = &a.output {
            let path_str = path.to_string_lossy();
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(path_str.as_ref()).spawn();
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd").args(["/c", "start", path_str.as_ref()]).spawn();
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = std::process::Command::new("xdg-open").arg(path_str.as_ref()).spawn();
        }
    }

    Ok(())
}
