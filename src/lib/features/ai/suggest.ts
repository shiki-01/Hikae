// 保存メモの AI 提案の取得条件と状態遷移（純関数）。
// 提案はボタンで開始する（設計 10.4）。変更一覧や設定が変わったら、結果・失敗は破棄して待機に戻す。
import type { MessageKey } from '#lib/i18n/index.js';
import type {
	AiCatalog,
	AiSuggestFailure,
	AiSuggestion,
	AppSettings,
	Change
} from '#lib/api/types.js';

/** 変更一覧の内容を表す文字列（種別とパスの並び。順番が変われば別の値になる） */
export function changesKey(changes: Change[]): string {
	return changes.map((change) => `${change.type}\u0000${change.path}`).join('\u0001');
}

/**
 * 提案の機能が使える状態か。
 * 提案が「AI を使う」以外、モデルが未選択、内蔵のモデルが導入済みでない場合は使わない。
 * 内蔵で自分で用意した GGUF を使うときは、パスが入っていれば使える扱いにする（検査は生成のときに行う）。
 * Ollama は接続の成否を生成のときに確かめるため、ここでは接続先を見ない。
 */
export function aiSuggestionReady(
	settings:
		| Pick<
				AppSettings,
				'memoSuggestion' | 'aiRunner' | 'aiModel' | 'aiModelSource' | 'aiCustomGgufPath'
		  >
		| undefined,
	catalog: AiCatalog | undefined
): boolean {
	if (!settings || settings.memoSuggestion !== 'ai') return false;
	if (settings.aiRunner === 'ollama') return settings.aiModel !== '';
	if (settings.aiModelSource === 'custom-gguf') return settings.aiCustomGgufPath !== '';
	if (settings.aiModel === '') return false;
	const model = catalog?.models.find((m) => m.id === settings.aiModel);
	return model?.status === 'ready';
}

/** 「提案を作る」ボタンを出すか（機能が使え、変更がある） */
export function canOfferSuggestion(input: { ready: boolean; changeCount: number }): boolean {
	return input.ready && input.changeCount > 0;
}

/** 提案の画面の状態。`generating` の `seq` は要求の番号（古い要求の結果を捨てるため） */
export type SuggestState =
	| { kind: 'idle' }
	| { kind: 'generating'; seq: number }
	| { kind: 'result'; text: string }
	| { kind: 'failed'; failure: AiSuggestFailure };

export type SuggestEvent =
	| { type: 'start'; seq: number }
	| { type: 'done'; seq: number; result: AiSuggestion }
	/** 利用者が「中止」を押した、または変更一覧や設定が変わった */
	| { type: 'reset' };

export const SUGGEST_IDLE: SuggestState = { kind: 'idle' };

/**
 * 提案の状態を進める。
 * - `done` は、生成中で番号が一致するときだけ受け付ける（中止・再要求の後に届いた古い結果は捨てる）
 * - 中止（取り消し）の結果は画面に出さず、待機に戻す
 */
export function reduceSuggest(state: SuggestState, event: SuggestEvent): SuggestState {
	switch (event.type) {
		case 'start':
			return { kind: 'generating', seq: event.seq };
		case 'reset':
			return SUGGEST_IDLE;
		case 'done': {
			if (state.kind !== 'generating' || state.seq !== event.seq) return state;
			const { text, failure } = event.result;
			if (text !== null) return { kind: 'result', text };
			if (failure === null || failure === 'cancelled') return SUGGEST_IDLE;
			return { kind: 'failed', failure };
		}
	}
}

/** 失敗の理由 → 画面の文言のキー（3 要素: 何が起きたか、データは無事か）。次の行動は「もう一度」ボタン */
export function suggestFailureKey(failure: AiSuggestFailure): MessageKey {
	return `save_bar.ai_failed.${failure}` as MessageKey;
}
