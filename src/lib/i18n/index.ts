import ja from './ja';
import en from './en';
import { currentLocale, type Locale } from './locale.svelte';

export { currentLocale, setLocale, type Locale } from './locale.svelte';

// 文言のキーは ja.ts が正。en.ts は同じキー集合を型で強制する
export type MessageKey = keyof typeof ja;
export type MessageParams = Record<string, string | number>;

const DICTIONARIES: Record<Locale, Record<MessageKey, string>> = { ja, en };

/** 動的に組み立てたキーが定義済みかどうか（キー集合は ja.ts と一致する） */
export function hasMessage(key: string): key is MessageKey {
	return Object.hasOwn(ja, key);
}

/** いまの言語の文言を返す。テンプレートで呼ぶと、言語の切り替えに追随する */
export function t(key: MessageKey, params?: MessageParams): string {
	const template: string = DICTIONARIES[currentLocale()][key];
	if (!params) return template;
	return template.replace(/\{(\w+)\}/g, (match, name: string) =>
		name in params ? String(params[name]) : match
	);
}
