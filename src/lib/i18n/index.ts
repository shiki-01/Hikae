import ja from './ja';

export type MessageKey = keyof typeof ja;
export type MessageParams = Record<string, string | number>;

/** 動的に組み立てたキーが定義済みかどうか */
export function hasMessage(key: string): key is MessageKey {
	return Object.hasOwn(ja, key);
}

export function t(key: MessageKey, params?: MessageParams): string {
	const template: string = ja[key];
	if (!params) return template;
	return template.replace(/\{(\w+)\}/g, (match, name: string) =>
		name in params ? String(params[name]) : match
	);
}
