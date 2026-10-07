import ja from './ja';

type Messages = typeof ja;
type Keys = keyof Messages;

// 翻訳キーの型チェック付き翻訳関数
export function t(key: Keys): string {
	return ja[key] || key;
}
