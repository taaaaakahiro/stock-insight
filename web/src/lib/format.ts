// 表記は仕様書に合わせる: 日付は YYYY/MM/DD、時刻は日本時間、金額は円、比率は小数点以下2桁の %

const NONE = "—";

/** 0.0402 → "4.02%" */
export function pct(ratio: number | null | undefined, digits = 2): string {
  if (ratio == null || !Number.isFinite(ratio)) return NONE;
  return `${(ratio * 100).toFixed(digits)}%`;
}

/** 1.234 → "1.23倍" */
export function times(x: number | null | undefined): string {
  if (x == null || !Number.isFinite(x)) return NONE;
  return `${x.toFixed(2)}倍`;
}

const yenFormat = new Intl.NumberFormat("ja-JP", { maximumFractionDigits: 1 });

/** 3431 → "3,431円"、176.1999 → "176.2円" */
export function yen(x: number | null | undefined): string {
  if (x == null || !Number.isFinite(x)) return NONE;
  return `${yenFormat.format(Math.round(x * 10) / 10)}円`;
}

/** 1234567890 → "12.3億円"。百万円未満は "0.0億円" になるので、小さい値は万円で出す */
export function oku(x: number | null | undefined): string {
  if (x == null || !Number.isFinite(x)) return NONE;
  const abs = Math.abs(x);
  if (abs < 1e8) return `${(x / 1e4).toLocaleString("ja-JP", { maximumFractionDigits: 0 })}万円`;
  // 1,000億円以上は小数を出さない（490,000.0億円 のような末尾の .0 を避ける）
  return `${(x / 1e8).toLocaleString("ja-JP", { maximumFractionDigits: abs < 1e11 ? 1 : 0 })}億円`;
}

/** 1株配当など、円の小数1桁 */
export function yen1(x: number | null | undefined): string {
  if (x == null || !Number.isFinite(x)) return NONE;
  return `${x.toLocaleString("ja-JP", { minimumFractionDigits: 1, maximumFractionDigits: 1 })}円`;
}

export function count(n: number | null | undefined, unit = "年"): string {
  if (n == null) return NONE;
  return `${n}${unit}`;
}

/** "2026-10-21" → "2026/10/21" */
export function ymd(date: string | null | undefined): string {
  if (!date) return NONE;
  return date.replaceAll("-", "/");
}

const jstFormat = new Intl.DateTimeFormat("ja-JP", {
  timeZone: "Asia/Tokyo",
  month: "numeric",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

/** UNIX秒 → "10/21 18:30"（日本時間） */
export function jstDateTime(unixSec: number | null | undefined): string {
  if (unixSec == null) return NONE;
  return jstFormat.format(new Date(unixSec * 1000));
}
