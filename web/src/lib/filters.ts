// スクリーニングの絞り込み条件。画面の URL と API のクエリは同じキー・同じ単位（比率は小数）にする。
// 入力欄は人が読む単位（% や 倍）で扱い、ここで変換する。

export type Unit = "pct" | "times" | "yen" | "years";

export type MetricKey =
  | "dividend_ttm"
  | "dividend_yield"
  | "yield_percentile"
  | "pbr"
  | "per"
  | "payout_ratio"
  | "doe"
  | "consecutive_increases"
  | "operating_margin"
  | "sales_growth"
  | "op_growth"
  | "price"
  | "floating_ratio"
  | "unknown_ratio"
  | "reprice_ratio"
  | "reprice_max_ratio"
  | "avg_rate"
  | "occupancy"
  | "doe_floor"
  | "doe_margin"
  | "op_growth_q"
  | "op_growth_ttm";

/** 一覧のタブ。株式以外は財務の指標がないので、使える指標を絞る */
export type Kind = "stock" | "reit" | "etf";

export const KINDS: { key: Kind; label: string }[] = [
  { key: "stock", label: "株式" },
  { key: "reit", label: "REIT" },
  { key: "etf", label: "ETF" },
];

const ALL: Kind[] = ["stock", "reit", "etf"];
const STOCK: Kind[] = ["stock"];
const REIT: Kind[] = ["reit"];

export const METRICS: { key: MetricKey; label: string; unit: Unit; kinds: Kind[] }[] = [
  { key: "dividend_ttm", label: "1株配当", unit: "yen", kinds: ALL },
  { key: "dividend_yield", label: "配当利回り", unit: "pct", kinds: ALL },
  { key: "yield_percentile", label: "利回りの過去5年比", unit: "pct", kinds: ALL },
  { key: "pbr", label: "PBR", unit: "times", kinds: STOCK },
  { key: "per", label: "PER", unit: "times", kinds: STOCK },
  { key: "payout_ratio", label: "配当性向", unit: "pct", kinds: STOCK },
  { key: "doe", label: "DOE", unit: "pct", kinds: STOCK },
  { key: "doe_floor", label: "DOE下限", unit: "pct", kinds: STOCK },
  { key: "doe_margin", label: "DOEの余裕", unit: "pct", kinds: STOCK },
  { key: "consecutive_increases", label: "連続増配", unit: "years", kinds: STOCK },
  { key: "operating_margin", label: "営業利益率", unit: "pct", kinds: STOCK },
  { key: "sales_growth", label: "売上の伸び", unit: "pct", kinds: STOCK },
  { key: "op_growth", label: "営業利益の伸び", unit: "pct", kinds: STOCK },
  { key: "op_growth_q", label: "直近四半期の営業利益の伸び", unit: "pct", kinds: STOCK },
  { key: "op_growth_ttm", label: "直近4四半期の営業利益の伸び", unit: "pct", kinds: STOCK },
  { key: "price", label: "株価", unit: "yen", kinds: ALL },
  { key: "floating_ratio", label: "変動金利", unit: "pct", kinds: REIT },
  { key: "unknown_ratio", label: "金利の記載なし", unit: "pct", kinds: REIT },
  { key: "reprice_ratio", label: "1年内の金利見直し", unit: "pct", kinds: REIT },
  { key: "reprice_max_ratio", label: "同（最大）", unit: "pct", kinds: REIT },
  { key: "avg_rate", label: "平均利率", unit: "pct", kinds: REIT },
  { key: "occupancy", label: "稼働率", unit: "pct", kinds: REIT },
];

/** REIT の金利・稼働率の説明（有報の借入金明細・物件の表から読んだ値） */
const REIT_TIPS: Partial<Record<MetricKey, string>> = {
  floating_ratio:
    "有利子負債（借入金＋投資法人債）のうち、有報に「変動金利」と書かれている借入の割合です。金利が上がるとすぐに支払利息が増えます。",
  unknown_ratio:
    "有利子負債のうち、有報に固定か変動かが書かれていない借入の割合です。高いほど、下の「金利見直し」の幅が広くなります（決算説明資料で確かめてください）。",
  reprice_ratio:
    "1年以内に金利が新しくなる借入の割合（少なく見た値）。変動金利の借入に、1年以内に返済期限が来る借入（借り換えで今の金利になる）と投資法人債を足したもの。記載なしの借入は含めません。",
  reprice_max_ratio:
    "上の割合に、固定か変動かの記載がない借入を全部変動とみなして足した値（多く見た値）。実際はこの間のどこかです。",
  avg_rate: "借入の残高で重みを付けた平均利率です。",
  occupancy: "物件の賃貸可能面積で重みを付けた稼働率（有報の物件の表から）です。",
};

/** DOE の説明（DOE は 1株配当 ÷ BPS の簡易な値。会社は株主資本の平均などで計算することがある） */
const DOE_TIPS: Partial<Record<MetricKey, string>> = {
  doe: "1株配当（最新の本決算の実績）÷ 1株純資産（BPS）。会社の計算（株主資本の平均など）とは少しずれます。",
  doe_floor:
    "有価証券報告書・半期報告書に「DOE ○%を下限」「○%以上」と書かれた数字です。業績が落ちても、BPS × この割合の配当は守る方針です。",
  doe_margin:
    "今の DOE − DOE下限（ポイント）。今の DOE は会社予想の配当があればそれで（特別・記念配当を含む合計配当 ÷ BPS）、なければ実績で出します。マイナスは、下限の方針より配当が少ないことを示します。",
};

/** 業績の伸びの説明（累計・四半期単独・4四半期合計の違い） */
const GROWTH_TIPS: Partial<Record<MetricKey, string>> = {
  op_growth: "直近の決算の期首からの累計（1Q〜FY）と、1年前の同じ期の累計を比べた前年同期比です。",
  op_growth_q:
    "直近の四半期だけ（累計の差分）の営業利益を、前年の同じ四半期と比べた前年同期比です。累計よりも今の勢いが出ます。",
  op_growth_ttm:
    "直近4四半期の営業利益の合計を、その前の4四半期の合計と比べた伸びです。直近の四半期だけ増益で、こちらがマイナスなら一過性の疑いがあります。",
};

/** 指標の説明（あるものだけ）。InfoTip に出す */
export function metricTip(key: MetricKey, kind: Kind): string | undefined {
  if (kind === "stock" && (key === "dividend_ttm" || key === "dividend_yield")) {
    return "会社予想の年間配当総額（特別・記念配当を含む）÷ 株価で計算します。開示済みの最新対象年度の予想を使用します。予想が未定の場合は実績を使い、EDINETで確認できた特別・記念配当を除外します。";
  }
  if (key === "yield_percentile") return yieldPercentileTip(kind);
  return REIT_TIPS[key] ?? DOE_TIPS[key] ?? GROWTH_TIPS[key];
}

/** REIT の用途（data/reit_sectors.csv と同じキー） */
export const REIT_SECTORS: { key: string; label: string }[] = [
  { key: "office", label: "オフィス" },
  { key: "logistics", label: "物流" },
  { key: "residential", label: "住宅" },
  { key: "hotel", label: "ホテル" },
  { key: "retail", label: "商業" },
  { key: "healthcare", label: "ヘルスケア" },
  { key: "diversified", label: "総合・複合" },
  { key: "infra", label: "インフラ" },
];

export const reitSectorLabel = (key: string | null | undefined) =>
  REIT_SECTORS.find((s) => s.key === key)?.label ?? null;

/** REIT・ETF は「配当」ではなく「分配金」、株価は「価格」と呼ぶ */
export function metricLabel(key: MetricKey, kind: Kind): string {
  const label = METRICS.find((m) => m.key === key)?.label ?? key;
  if (kind === "stock") return label;
  return label.replace("1株配当", "1口分配金").replace("配当", "分配金").replace("株価", "価格");
}

export const metricsFor = (kind: Kind) => METRICS.filter((m) => m.kinds.includes(kind));

export const UNIT_SUFFIX: Record<Unit, string> = { pct: "%", times: "倍", yen: "円", years: "年" };

export type Valuation = "" | "cheap" | "neutral" | "expensive";

/** JPX の33業種（上場銘柄一覧の「33業種区分」と同じ表記） */
export const SECTORS33 = [
  "水産・農林業",
  "鉱業",
  "建設業",
  "食料品",
  "繊維製品",
  "パルプ・紙",
  "化学",
  "医薬品",
  "石油・石炭製品",
  "ゴム製品",
  "ガラス・土石製品",
  "鉄鋼",
  "非鉄金属",
  "金属製品",
  "機械",
  "電気機器",
  "輸送用機器",
  "精密機器",
  "その他製品",
  "電気・ガス業",
  "陸運業",
  "海運業",
  "空運業",
  "倉庫・運輸関連業",
  "情報・通信業",
  "卸売業",
  "小売業",
  "銀行業",
  "証券、商品先物取引業",
  "保険業",
  "その他金融業",
  "不動産業",
  "サービス業",
] as const;

export const MARKETS = ["プライム", "スタンダード", "グロース"] as const;
export const SIZES: { key: string; label: string; hint: string }[] = [
  {
    key: "large",
    label: "大型",
    hint: "時価総額と売買の多さで上位100社（TOPIX Core30・Large70）",
  },
  { key: "mid", label: "中型", hint: "大型に次ぐ400社（TOPIX Mid400）" },
  { key: "small", label: "小型", hint: "TOPIX に入っている残りの会社（TOPIX Small）" },
  {
    key: "none",
    label: "区分なし",
    hint: "TOPIX に入っていない会社（スタンダード・グロース市場の会社や、上場して間もない会社など）",
  },
];

export const SIZE_TIP = [
  "JPX が TOPIX（東証株価指数）の銘柄を会社の大きさで分けた区分です。",
  ...SIZES.map((s) => `・${s.label}: ${s.hint}`),
  "大きい会社ほど株価の動きが穏やかで、売り買いしやすい傾向があります。",
].join("\n");

/** 利回りの過去5年比の説明。REIT・ETF では「配当」を「分配金」に読み替える */
export function yieldPercentileTip(kind: Kind): string {
  const tip = [
    "いまの配当利回りが、過去5年（週ごと）の利回りの中でどのくらい高いかを 0〜100% で表したものです。",
    "例: 90% なら、過去5年の9割の時期より今の利回りが高い。",
    "配当が同じなら株価が下がるほど利回りは上がるので、",
    "・高い → 過去と比べて株価が安い（割安かもしれない）",
    "・低い → 過去と比べて株価が高い（割高かもしれない）",
  ];
  if (kind === "stock") {
    tip.push(
      "ただし業績が悪くて株価が下がっていることもあるので、判定（赤字なし など）も合わせて見てください。",
    );
    return tip.join("\n");
  }
  return tip.join("\n").replaceAll("配当", "分配金").replaceAll("株価", "価格");
}

/** 一覧のバッジ（株式だけ）。API のクエリ名と表示名 */
export const FLAGS: { key: FlagKey; label: string; hint: string }[] = [
  { key: "isValue", label: "バリュー株", hint: "PBR 1倍以下 かつ PER 15倍以下" },
  { key: "dividendEveryYear", label: "5年連続で配当", hint: "直近5期すべてで配当がある" },
  { key: "noLossYears", label: "通期で赤字なし", hint: "直近5期の純利益と営業利益がすべてプラス" },
  {
    key: "noQuarterlyLoss",
    label: "四半期で赤字なし",
    hint: "直近20四半期の単独の損益がすべてプラス",
  },
  { key: "profitUptrend", label: "営業利益が増加傾向", hint: "直近5期の営業利益の傾きがプラス" },
  {
    key: "dividendPolicy",
    label: "累進配当・非減配の方針",
    hint: "有価証券報告書・半期報告書（EDINET）に累進配当や「減配しない」の記述がある",
  },
];
export type FlagKey =
  | "isValue"
  | "dividendEveryYear"
  | "noLossYears"
  | "noQuarterlyLoss"
  | "profitUptrend"
  | "dividendPolicy";
const FLAG_PARAM: Record<FlagKey, string> = {
  isValue: "is_value",
  dividendEveryYear: "dividend_every_year",
  noLossYears: "no_loss_years",
  noQuarterlyLoss: "no_quarterly_loss",
  profitUptrend: "profit_uptrend",
  dividendPolicy: "dividend_policy",
};

export type Filters = {
  kind: Kind;
  /** コードまたは銘柄名で検索 */
  query: string;
  /** 株式だけ: 33業種・市場・規模（複数選択） */
  sectors: string[];
  markets: string[];
  sizes: string[];
  /** REIT だけ: 用途（複数選択） */
  reitSectors: string[];
  isValue: boolean;
  dividendEveryYear: boolean;
  noQuarterlyLoss: boolean;
  /** 入力欄の文字列（人が読む単位）。キーは `${metric}_min` / `${metric}_max` */
  ranges: Partial<Record<`${MetricKey}_${"min" | "max"}`, string>>;
  valuation: Valuation;
  noLossYears: boolean;
  profitUptrend: boolean;
  dividendPolicy: boolean;
  sort: MetricKey | "code";
  order: "asc" | "desc";
  page: number;
};

export const DEFAULT_FILTERS: Filters = {
  kind: "stock",
  query: "",
  sectors: [],
  markets: [],
  sizes: [],
  reitSectors: [],
  isValue: false,
  dividendEveryYear: false,
  noQuarterlyLoss: false,
  ranges: {},
  valuation: "",
  noLossYears: false,
  profitUptrend: false,
  dividendPolicy: false,
  sort: "dividend_yield",
  order: "desc",
  page: 1,
};

const unitOf = (key: MetricKey) => METRICS.find((m) => m.key === key)?.unit ?? "times";

/** 入力欄の値 → API の値（% は 100 で割る）。数でなければ undefined */
export function toApiNumber(input: string, unit: Unit): string | undefined {
  const t = input.trim().replace(/[,，]/g, "");
  if (t === "") return undefined;
  const n = Number(t);
  if (!Number.isFinite(n)) return undefined;
  return String(unit === "pct" ? Math.round(n * 1e6) / 1e8 : n);
}

/** API の値 → 入力欄の値 */
export function fromApiNumber(value: string, unit: Unit): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return "";
  return String(unit === "pct" ? Math.round(n * 1e8) / 1e6 : n);
}

/** 条件 → クエリ文字列（既定値は省く） */
export function toQuery(f: Filters): URLSearchParams {
  const q = new URLSearchParams();
  // API の既定は株式だが、タブを明示しておく
  q.set("kind", f.kind);
  if (f.query.trim()) q.set("q", f.query.trim());
  if (f.kind === "stock") {
    if (f.sectors.length) q.set("sector", f.sectors.join(","));
    if (f.markets.length) q.set("market", f.markets.join(","));
    if (f.sizes.length) q.set("size", f.sizes.join(","));
  }
  if (f.kind === "reit" && f.reitSectors.length) q.set("reit_sector", f.reitSectors.join(","));
  for (const m of metricsFor(f.kind)) {
    for (const b of ["min", "max"] as const) {
      const v = f.ranges[`${m.key}_${b}`];
      const api = v === undefined ? undefined : toApiNumber(v, m.unit);
      if (api !== undefined) q.set(`${m.key}_${b}`, api);
    }
  }
  if (f.valuation) q.set("valuation", f.valuation);
  if (f.kind === "stock") {
    for (const flag of FLAGS) if (f[flag.key]) q.set(FLAG_PARAM[flag.key], "1");
  }
  if (f.sort !== DEFAULT_FILTERS.sort) q.set("sort", f.sort);
  if (f.order !== DEFAULT_FILTERS.order) q.set("order", f.order);
  if (f.page > 1) q.set("page", String(f.page));
  return q;
}

/** クエリ文字列 → 条件。おかしな値は既定値に戻す */
export function fromQuery(q: URLSearchParams): Filters {
  const f: Filters = { ...DEFAULT_FILTERS, ranges: {} };
  const kind = q.get("kind");
  if (kind === "stock" || kind === "reit" || kind === "etf") f.kind = kind;
  f.query = (q.get("q") ?? "").slice(0, 50);
  const list = (key: string, allowed?: readonly string[]) =>
    (q.get(key) ?? "")
      .split(",")
      .map((s) => s.trim())
      .filter((s) => s !== "" && (!allowed || allowed.includes(s)));
  if (f.kind === "stock") {
    f.sectors = list("sector", SECTORS33);
    f.markets = list("market", MARKETS);
    f.sizes = list(
      "size",
      SIZES.map((s) => s.key),
    );
    for (const flag of FLAGS) f[flag.key] = q.get(FLAG_PARAM[flag.key]) === "1";
  }
  if (f.kind === "reit") {
    f.reitSectors = list(
      "reit_sector",
      REIT_SECTORS.map((s) => s.key),
    );
  }
  for (const m of metricsFor(f.kind)) {
    for (const b of ["min", "max"] as const) {
      const v = q.get(`${m.key}_${b}`);
      if (v !== null) {
        const input = fromApiNumber(v, unitOf(m.key));
        if (input !== "") f.ranges[`${m.key}_${b}`] = input;
      }
    }
  }
  const val = q.get("valuation");
  if (val === "cheap" || val === "neutral" || val === "expensive") f.valuation = val;
  const sort = q.get("sort");
  if (sort === "code" || metricsFor(f.kind).some((m) => m.key === sort))
    f.sort = sort as Filters["sort"];
  const order = q.get("order");
  if (order === "asc" || order === "desc") f.order = order;
  const page = Number(q.get("page"));
  if (Number.isInteger(page) && page >= 1 && page <= 1000) f.page = page;
  return f;
}

/** タブを切り替える。そのタブで使えない条件は外す */
export function switchKind(f: Filters, kind: Kind): Filters {
  const keys = new Set(metricsFor(kind).map((m) => m.key));
  const ranges = Object.fromEntries(
    Object.entries(f.ranges).filter(([k]) => keys.has(k.replace(/_(min|max)$/, "") as MetricKey)),
  ) as Filters["ranges"];
  const sortOk = f.sort === "code" || keys.has(f.sort);
  const stock = kind === "stock";
  return {
    ...f,
    kind,
    ranges,
    sectors: stock ? f.sectors : [],
    markets: stock ? f.markets : [],
    sizes: stock ? f.sizes : [],
    reitSectors: kind === "reit" ? f.reitSectors : [],
    isValue: stock && f.isValue,
    dividendEveryYear: stock && f.dividendEveryYear,
    noQuarterlyLoss: stock && f.noQuarterlyLoss,
    noLossYears: stock && f.noLossYears,
    profitUptrend: stock && f.profitUptrend,
    dividendPolicy: stock && f.dividendPolicy,
    sort: sortOk ? f.sort : DEFAULT_FILTERS.sort,
    order: sortOk ? f.order : DEFAULT_FILTERS.order,
    page: 1,
  };
}

/** 入力欄の値が数として読めないか（赤枠を出す） */
export function isInvalidInput(input: string | undefined): boolean {
  return input !== undefined && input.trim() !== "" && toApiNumber(input, "times") === undefined;
}

/**
 * 範囲の入力欄のまとまり。全部を並べると長いので、まとまりごとに畳む（open は最初から開くもの）。
 * 絞り込みにあまり使わない指標（1株配当・株価・DOE下限・直近四半期の伸び・金利の記載なし・同（最大））と、
 * 「高い・ふつう・低い」で選べる利回りの過去5年比は欄に出さない（URL で渡せば今までどおり効き、チップで外せる）
 */
export const METRIC_GROUPS: { key: string; label: string; keys: MetricKey[]; open?: boolean }[] = [
  {
    key: "dividend",
    label: "配当",
    keys: ["dividend_yield", "payout_ratio", "consecutive_increases"],
    open: true,
  },
  { key: "value", label: "割安さ", keys: ["pbr", "per"], open: true },
  { key: "doe", label: "配当方針（DOE）", keys: ["doe", "doe_margin"] },
  {
    key: "growth",
    label: "業績",
    keys: ["operating_margin", "sales_growth", "op_growth", "op_growth_ttm"],
  },
  {
    key: "reit",
    label: "借入・物件",
    keys: ["floating_ratio", "reprice_ratio", "avg_rate", "occupancy"],
  },
];

/** そのタブで使えるまとまり（使える指標だけ残し、空のまとまりは外す） */
export function metricGroupsFor(kind: Kind) {
  const keys = new Set(metricsFor(kind).map((m) => m.key));
  return METRIC_GROUPS.map((g) => ({ ...g, keys: g.keys.filter((k) => keys.has(k)) })).filter(
    (g) => g.keys.length > 0 || g.key === "value",
  );
}

const VALUATION_LABEL: Record<Exclude<Valuation, "">, string> = {
  cheap: "高い",
  neutral: "ふつう",
  expensive: "低い",
};

/** 効いている条件（一覧の上にチップで出し、× で外す）。検索語・並び順・ページは含めない */
export function activeConditions(f: Filters): { id: string; label: string; clear: Filters }[] {
  const out: { id: string; label: string; clear: Filters }[] = [];
  const add = (id: string, label: string, patch: Partial<Filters>) =>
    out.push({ id, label, clear: { ...f, ...patch, page: 1 } });
  for (const flag of FLAGS) if (f[flag.key]) add(flag.key, flag.label, { [flag.key]: false });
  for (const m of f.markets) add(`market:${m}`, m, { markets: f.markets.filter((x) => x !== m) });
  for (const s of f.sizes)
    add(`size:${s}`, SIZES.find((x) => x.key === s)?.label ?? s, {
      sizes: f.sizes.filter((x) => x !== s),
    });
  for (const s of f.sectors) add(`sector:${s}`, s, { sectors: f.sectors.filter((x) => x !== s) });
  for (const s of f.reitSectors)
    add(`reit:${s}`, reitSectorLabel(s) ?? s, {
      reitSectors: f.reitSectors.filter((x) => x !== s),
    });
  if (f.valuation)
    add(
      "valuation",
      `${f.kind === "stock" ? "配当" : "分配金"}利回りの過去5年比: ${VALUATION_LABEL[f.valuation]}`,
      { valuation: "" },
    );
  for (const m of metricsFor(f.kind)) {
    const min = f.ranges[`${m.key}_min`]?.trim();
    const max = f.ranges[`${m.key}_max`]?.trim();
    if (!min && !max) continue;
    const u = UNIT_SUFFIX[m.unit];
    const range = min && max ? `${min}〜${max}${u}` : min ? `${min}${u}以上` : `${max}${u}以下`;
    const { [`${m.key}_min` as const]: _min, [`${m.key}_max` as const]: _max, ...rest } = f.ranges;
    add(m.key, `${metricLabel(m.key, f.kind)} ${range}`, { ranges: rest });
  }
  return out;
}
