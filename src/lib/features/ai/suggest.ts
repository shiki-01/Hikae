// 保存メモの AI 提案の取得条件（純関数）。
import type { AiCatalog, AppSettings, Change } from '#lib/api/types.js';

/** 変更一覧が変わってから提案を取り直すまでの待ち時間（ミリ秒） */
export const SUGGESTION_DEBOUNCE_MS = 1500;

/** 変更一覧の内容を表す文字列（種別とパスの並び。順番が変われば別の値になる） */
export function changesKey(changes: Change[]): string {
	return changes.map((change) => `${change.type}\u0000${change.path}`).join('\u0001');
}

/**
 * 提案の機能が使える状態か。
 * 提案が「AI を使う」以外、モデルが未選択、内蔵のモデルが導入済みでない場合は使わない。
 * Ollama は接続の成否を生成のときに確かめるため、ここでは接続先を見ない。
 */
export function aiSuggestionReady(
	settings: Pick<AppSettings, 'memoSuggestion' | 'aiRunner' | 'aiModel'> | undefined,
	catalog: AiCatalog | undefined
): boolean {
	if (!settings || settings.memoSuggestion !== 'ai' || settings.aiModel === '') return false;
	if (settings.aiRunner === 'ollama') return true;
	const model = catalog?.models.find((m) => m.id === settings.aiModel);
	return model?.status === 'ready';
}

/** 提案を取りに行くか（変更があり、待ち時間を過ぎた内容が決まっているとき） */
export function shouldRequestSuggestion(input: {
	ready: boolean;
	projectId: string;
	changeCount: number;
	debouncedKey: string;
}): boolean {
	return (
		input.ready && input.projectId !== '' && input.changeCount > 0 && input.debouncedKey !== ''
	);
}
