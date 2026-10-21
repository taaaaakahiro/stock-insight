//! J-Quants の応答を全項目そのまま、日付ごとの gzip JSON にして手元に書く（あとで R2 に上げる）。
//! 1ファイル = 1日分の全銘柄。項目を選ばないので、後からどんな分析にも使える。

use domain::Date;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use usecase::PortError;

/// `{dir}/{kind}/{YYYY-MM-DD}.json.gz` に書き、そのパスを返す。空の日は書かない（None）
pub fn write_day(
    dir: &Path,
    kind: &str,
    date: Date,
    rows: &[Value],
) -> Result<Option<PathBuf>, PortError> {
    if rows.is_empty() {
        return Ok(None);
    }
    let sub = dir.join(kind);
    std::fs::create_dir_all(&sub).map_err(|e| PortError(format!("raw: {e}")))?;
    let path = sub.join(format!("{date}.json.gz"));
    let file = std::fs::File::create(&path).map_err(|e| PortError(format!("raw: {e}")))?;
    let mut gz = GzEncoder::new(file, Compression::default());
    serde_json::to_writer(&mut gz, rows).map_err(|e| PortError(format!("raw: {e}")))?;
    gz.finish()
        .and_then(|mut f| f.flush())
        .map_err(|e| PortError(format!("raw: {e}")))?;
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::Read;

    #[test]
    fn writes_gzipped_json_per_day() {
        let dir = std::env::temp_dir().join(format!("stock-insight-raw-{}", std::process::id()));
        let rows = vec![serde_json::json!({"Code": "72030", "C": 3094.0})];
        let path = write_day(&dir, "bars", Date::new(2026, 10, 21), &rows)
            .unwrap()
            .unwrap();
        assert!(path.ends_with("bars/2026-10-21.json.gz"));
        let mut s = String::new();
        GzDecoder::new(std::fs::File::open(&path).unwrap())
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, r#"[{"C":3094.0,"Code":"72030"}]"#);
        assert_eq!(
            write_day(&dir, "fins", Date::new(2026, 10, 21), &[]).unwrap(),
            None
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
