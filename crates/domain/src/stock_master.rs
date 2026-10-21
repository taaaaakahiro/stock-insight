//! 銘柄マスタ（JPX「東証上場銘柄一覧」）。名称・種類・市場・業種・規模区分。

use crate::StockCode;

/// 規模区分。JPX が時価総額と流動性で決める TOPIX の区分をそのまま使う（F2）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeClass {
    /// TOPIX Core30 + Large70
    Large,
    /// TOPIX Mid400
    Mid,
    /// TOPIX Small 1 / Small 2
    Small,
    /// TOPIX に含まれない（グロース市場など）
    None,
}

impl SizeClass {
    pub fn from_jpx(s: &str) -> Self {
        match s.trim() {
            "TOPIX Core30" | "TOPIX Large70" => SizeClass::Large,
            "TOPIX Mid400" => SizeClass::Mid,
            "TOPIX Small 1" | "TOPIX Small 2" => SizeClass::Small,
            _ => SizeClass::None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SizeClass::Large => "large",
            SizeClass::Mid => "mid",
            SizeClass::Small => "small",
            SizeClass::None => "none",
        }
    }
}

/// 銘柄の種類。一覧では種類ごとにタブを分ける
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// 国内株式（プライム・スタンダード・グロース）
    Stock,
    /// REIT・インフラファンドなど（JPX の「REIT・ベンチャーファンド・カントリーファンド・インフラファンド」）
    Reit,
    /// ETF・ETN（上場投資信託など）
    Etf,
}

impl AssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetKind::Stock => "stock",
            AssetKind::Reit => "reit",
            AssetKind::Etf => "etf",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        [AssetKind::Stock, AssetKind::Reit, AssetKind::Etf]
            .into_iter()
            .find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListedStock {
    pub code: StockCode,
    pub name: String,
    pub kind: AssetKind,
    /// 株式は "プライム" / "スタンダード" / "グロース"、それ以外は JPX の区分のまま
    pub market: String,
    /// 33業種区分（例: "水産・農林業"）。不明なら None
    pub sector33: Option<String>,
    pub size_class: SizeClass,
}

/// JPX の一覧の1行（必要な列だけ）から ListedStock を作る。
/// 対象は国内株式・REIT 等・ETF/ETN。PRO Market・外国株式・出資証券は None。
pub fn listed_stock_from_jpx(
    code: &str,
    name: &str,
    market: &str,
    sector33: &str,
    size: &str,
) -> Option<ListedStock> {
    let market = market.trim();
    let (kind, market) = if let Some(m) = market.strip_suffix("（内国株式）") {
        (AssetKind::Stock, m.to_string())
    } else if market.starts_with("REIT") {
        (AssetKind::Reit, market.to_string())
    } else if market == "ETF・ETN" {
        (AssetKind::Etf, market.to_string())
    } else {
        return None;
    };
    let name = halfwidth_ascii(name.trim());
    if name.is_empty() {
        return None;
    }
    let sector33 = sector33.trim();
    Some(ListedStock {
        code: StockCode::parse(code)?,
        name,
        kind,
        market,
        sector33: (!sector33.is_empty() && sector33 != "-").then(|| sector33.to_string()),
        size_class: SizeClass::from_jpx(size),
    })
}

/// JPX の名称は英数字が全角（例: "ＮＥＸＴ　ＦＵＮＤＳ"）。英数字・記号・空白だけ半角にする（カナはそのまま）
pub fn halfwidth_ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_domestic_stocks() {
        let s = listed_stock_from_jpx(
            "1301",
            "極洋",
            "プライム（内国株式）",
            "水産・農林業",
            "TOPIX Small 1",
        )
        .unwrap();
        assert_eq!(s.code.as_str(), "1301");
        assert_eq!(s.market, "プライム");
        assert_eq!(s.sector33.as_deref(), Some("水産・農林業"));
        assert_eq!(s.size_class, SizeClass::Small);
        assert_eq!(s.kind, AssetKind::Stock);

        let g = listed_stock_from_jpx(
            "130A",
            "ベリサーブ",
            "グロース（内国株式）",
            "情報・通信業",
            "-",
        )
        .unwrap();
        assert_eq!(g.size_class, SizeClass::None);

        let etf = listed_stock_from_jpx("1305", "ETF", "ETF・ETN", "-", "-").unwrap();
        assert_eq!(etf.kind, AssetKind::Etf);
        assert_eq!(etf.sector33, None);
        let reit = listed_stock_from_jpx(
            "8951",
            "日本ビルファンド投資法人",
            "REIT・ベンチャーファンド・カントリーファンド・インフラファンド",
            "-",
            "-",
        )
        .unwrap();
        assert_eq!(reit.kind, AssetKind::Reit);
        assert!(listed_stock_from_jpx("9999", "PRO", "PRO Market", "-", "-").is_none());
        assert!(listed_stock_from_jpx("9999", "外国", "プライム（外国株式）", "-", "-").is_none());
        assert!(listed_stock_from_jpx("12", "短い", "プライム（内国株式）", "-", "-").is_none());
    }

    #[test]
    fn names_use_halfwidth_ascii() {
        assert_eq!(
            halfwidth_ascii("ＮＥＸＴ　ＦＵＮＤＳ　ＴＯＰＩＸ連動型上場投信"),
            "NEXT FUNDS TOPIX連動型上場投信"
        );
        assert_eq!(
            halfwidth_ascii("Ｋ＆Ｏエナジーグループ"),
            "K&Oエナジーグループ"
        );
        assert_eq!(
            halfwidth_ascii("三菱ＵＦＪフィナンシャル・グループ"),
            "三菱UFJフィナンシャル・グループ"
        );
    }

    #[test]
    fn size_classes() {
        assert_eq!(SizeClass::from_jpx("TOPIX Core30"), SizeClass::Large);
        assert_eq!(SizeClass::from_jpx("TOPIX Large70"), SizeClass::Large);
        assert_eq!(SizeClass::from_jpx("TOPIX Mid400"), SizeClass::Mid);
        assert_eq!(SizeClass::from_jpx("TOPIX Small 2"), SizeClass::Small);
        assert_eq!(SizeClass::from_jpx("-"), SizeClass::None);
    }
}
