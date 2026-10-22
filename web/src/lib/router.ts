import { useEffect, useState } from "react";

// 画面の切り替えは History API だけで行う（ライブラリは使わない）。
// `/stocks/<コード>` が銘柄の詳細、それ以外はトップ（スクリーニング / ポートフォリオは # で分ける）

const EVENT = "app:navigate";

export function navigate(path: string) {
  window.history.pushState(null, "", path);
  window.dispatchEvent(new Event(EVENT));
}

/** 今のパス。pushState と戻る/進むで更新される */
export function usePath(): string {
  const [path, setPath] = useState(() => window.location.pathname);
  useEffect(() => {
    const update = () => setPath(window.location.pathname);
    window.addEventListener("popstate", update);
    window.addEventListener(EVENT, update);
    return () => {
      window.removeEventListener("popstate", update);
      window.removeEventListener(EVENT, update);
    };
  }, []);
  return path;
}

/** `/stocks/7203` → "7203"。銘柄コードは4桁の英数字だけ */
export function stockCodeFromPath(path: string): string | null {
  const m = /^\/stocks\/([0-9A-Z]{4})\/?$/.exec(path);
  return m?.[1] ?? null;
}

/** 一覧へ戻るリンク。直前が一覧なら履歴を戻し、直接開いた場合はトップへ */
export function backToList() {
  if (window.history.length > 1 && document.referrer.startsWith(window.location.origin)) {
    window.history.back();
  } else {
    navigate("/");
  }
}
