// 画面の言語の状態（設計書 2.3）。
//
// Svelte 5 の $state で持つので、テンプレート内の t() は言語が切り替わると再評価される。
// 言語の決定はここだけで行い、t() と format.ts はこの値を読む（ブラウザ単体でも動く）。

/** 画面の言語。設定の `language`（ja / en）と一致させる */
export type Locale = 'ja' | 'en';

const state = $state<{ current: Locale }>({ current: 'ja' });

/** いまの言語 */
export function currentLocale(): Locale {
	return state.current;
}

/** 言語を切り替える。`<html lang>` も合わせる（支援技術と組版のため） */
export function setLocale(next: Locale): void {
	state.current = next;
	if (typeof document !== 'undefined') document.documentElement.lang = next;
}
