//! Choropleth 색 계산 + HTML 코드젠.

pub const YLORRD: [&str; 7] = ["#ffffcc","#ffeda0","#fed976","#feb24c","#fd8d3c","#e31a1c","#800026"];
pub const BLUES:  [&str; 7] = ["#eff3ff","#c6dbef","#9ecae1","#6baed6","#4292c6","#2171b5","#084594"];
pub const GREENS: [&str; 7] = ["#edf8e9","#c7e9c0","#a1d99b","#74c476","#41ab5d","#238b45","#005a32"];
pub const REDS:   [&str; 7] = ["#fff5f0","#fee0d2","#fcbba1","#fc9272","#fb6a4a","#cb181d","#67000d"];
pub const VIRIDIS:[&str; 7] = ["#440154","#3b528b","#21908c","#27ad81","#5dc963","#aadc32","#fde725"];

pub fn pick_colors(ramp: &str, n: usize) -> Vec<&'static str> {
    let palette: &[&str] = match ramp.to_lowercase().as_str() {
        "blues"   => &BLUES,
        "greens"  => &GREENS,
        "reds"    => &REDS,
        "viridis" => &VIRIDIS,
        _         => &YLORRD,
    };
    let n = n.min(palette.len()).max(1);
    if n == 1 { return vec![palette[0]]; }
    (0..n).map(|i| {
        let idx = (i * (palette.len() - 1)) / (n - 1);
        palette[idx]
    }).collect()
}

pub fn compute_breaks(values: &[f64], n: usize, method: &str) -> Vec<f64> {
    if values.is_empty() || n < 2 { return vec![]; }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    match method {
        "equal" => {
            let min = sorted[0];
            let max = *sorted.last().unwrap();
            let step = (max - min) / n as f64;
            (1..n).map(|i| min + step * i as f64).collect()
        }
        _ => {
            (1..n).map(|i| {
                let pos = (i * sorted.len()) / n;
                let pos = pos.min(sorted.len() - 1);
                sorted[pos]
            }).collect()
        }
    }
}

pub fn gen_color_fn_js(breaks: &[f64], colors: &[&str], no_data_color: &str) -> String {
    let mut js = String::from("function vwColor(v){\n  if(v===null||v===undefined) return '");
    js.push_str(no_data_color);
    js.push_str("';\n");
    for (i, &b) in breaks.iter().enumerate() {
        js.push_str(&format!("  if(v < {b}) return '{}';\n", colors[i]));
    }
    js.push_str(&format!("  return '{}';\n}}", colors[colors.len() - 1]));
    js
}

pub fn gen_legend_html(breaks: &[f64], colors: &[&str], value_field: &str, no_data_color: &str) -> String {
    let mut html = format!(
        "<div id=\"vwLegend\" style=\"position:fixed;right:24px;top:80px;z-index:100000;background:#fff;border:1px solid #e5e8eb;border-radius:16px;padding:16px 20px;box-shadow:0 4px 16px rgba(0,0,0,.12);font-family:Pretendard,sans-serif;min-width:160px\">\
        <div style=\"font:700 14px Pretendard;color:#191f28;margin-bottom:12px\">{value_field}</div>"
    );
    for i in 0..colors.len() {
        let label = if breaks.is_empty() {
            "전체".to_string()
        } else if i == 0 {
            format!("< {:.1}", breaks[0])
        } else if i < breaks.len() {
            format!("{:.1} – {:.1}", breaks[i-1], breaks[i])
        } else {
            format!("≥ {:.1}", breaks[i-1])
        };
        html.push_str(&format!(
            "<div style=\"display:flex;align-items:center;gap:10px;margin-bottom:6px\">\
            <span style=\"display:inline-block;width:18px;height:18px;border-radius:5px;background:{};flex-shrink:0\"></span>\
            <span style=\"font:500 13px Pretendard;color:#4e5968\">{label}</span></div>",
            colors[i]
        ));
    }
    html.push_str(&format!(
        "<div style=\"display:flex;align-items:center;gap:10px;margin-top:4px\">\
        <span style=\"display:inline-block;width:18px;height:18px;border-radius:5px;background:{no_data_color};flex-shrink:0\"></span>\
        <span style=\"font:500 13px Pretendard;color:#8b95a1\">값 없음</span></div></div>"
    ));
    html
}

pub fn compute_geojson_extent(geojson_str: &str) -> Option<(f64, f64, f64, f64)> {
    let v: serde_json::Value = serde_json::from_str(geojson_str).ok()?;
    let features = v["features"].as_array()?;
    let mut minx = f64::MAX; let mut miny = f64::MAX;
    let mut maxx = f64::MIN; let mut maxy = f64::MIN;
    fn scan(coords: &serde_json::Value, minx: &mut f64, miny: &mut f64, maxx: &mut f64, maxy: &mut f64) {
        if let Some(arr) = coords.as_array() {
            if arr.len() >= 2 && arr[0].is_number() && arr[1].is_number() {
                let x = arr[0].as_f64().unwrap_or(0.0);
                let y = arr[1].as_f64().unwrap_or(0.0);
                if x < *minx { *minx = x; } if x > *maxx { *maxx = x; }
                if y < *miny { *miny = y; } if y > *maxy { *maxy = y; }
            } else {
                for c in arr { scan(c, minx, miny, maxx, maxy); }
            }
        }
    }
    for f in features {
        scan(&f["geometry"]["coordinates"], &mut minx, &mut miny, &mut maxx, &mut maxy);
    }
    if minx == f64::MAX { None } else { Some((minx, miny, maxx, maxy)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantile_breaks() {
        let v: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let b = compute_breaks(&v, 5, "quantile");
        assert_eq!(b.len(), 4);
    }

    #[test]
    fn test_equal_breaks() {
        let v = vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0];
        let b = compute_breaks(&v, 5, "equal");
        assert_eq!(b.len(), 4);
        assert!((b[0] - 10.0).abs() < 0.001);
    }

    #[test]
    fn test_pick_colors_n() {
        for ramp in &["ylorrd", "blues", "greens", "reds", "viridis"] {
            for n in 2..=7usize {
                let c = pick_colors(ramp, n);
                assert_eq!(c.len(), n, "ramp={ramp} n={n}");
            }
        }
    }

    #[test]
    fn test_color_fn_nodata() {
        let breaks = vec![10.0, 20.0];
        let colors = vec!["#fff", "#aaa", "#000"];
        let js = gen_color_fn_js(&breaks, &colors, "#cccccc");
        assert!(js.contains("#cccccc"));
        assert!(js.contains("v===null"));
    }

    #[test]
    fn test_pick_colors_single() {
        let c = pick_colors("blues", 1);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn test_compute_breaks_empty() {
        let b = compute_breaks(&[], 5, "quantile");
        assert!(b.is_empty());
    }

    #[test]
    fn test_geojson_extent() {
        let gj = r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[126.9,37.5],[127.1,37.5],[127.1,37.6],[126.9,37.6],[126.9,37.5]]]},"properties":{"pop":100}}
        ]}"#;
        let ext = compute_geojson_extent(gj);
        assert!(ext.is_some());
        let (minx, miny, maxx, maxy) = ext.unwrap();
        assert!((minx - 126.9).abs() < 0.001);
        assert!((miny - 37.5).abs() < 0.001);
        assert!((maxx - 127.1).abs() < 0.001);
        assert!((maxy - 37.6).abs() < 0.001);
    }
}
