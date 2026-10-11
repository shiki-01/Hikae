import { describe, it, expect } from 'vitest';
import type { AiCatalog, AiModel, AiSuggestFailure, AiSuggestion, Change } from '#lib/api/types.js';
import ja from '#lib/i18n/ja.js';
import {
	aiSuggestionReady,
	canOfferSuggestion,
	changesKey,
	reduceSuggest,
	SUGGEST_IDLE,
	suggestFailureKey,
	type SuggestState
} from './suggest';

function change(path: string, type: Change['type'] = 'modified'): Change {
	return { id: path, path, type, isConflict: false };
}

function catalog(status: AiModel['status']): AiCatalog {
	return {
		system: { totalMemoryBytes: 16, freeDiskBytes: 100 },
		models: [
			{
				id: 'gemma',
				displayName: 'G',
				stage: 2,
				sizeBytes: 1,
				licenseName: 'MIT',
				licenseUrl: 'https://example.invalid/license',
				minMemoryBytes: 1,
				recommended: true,
				status,
				partialBytes: 0,
				available: true,
				unavailableReason: null,
				downloading: false
			}
		]
	};
}

const aiOn = {
	memoSuggestion: 'ai',
	aiRunner: 'builtin',
	aiModel: 'gemma',
	aiModelSource: 'catalog',
	aiCustomGgufPath: ''
} as const;

function ok(text: string): AiSuggestion {
	return { text, failure: null };
}

function failed(failure: AiSuggestFailure): AiSuggestion {
	return { text: null, failure };
}

describe('変更一覧の内容の文字列', () => {
	it('同じ内容なら同じ、種別かパスが変われば違う値になる', () => {
		const base = [change('a.txt'), change('b.txt', 'added')];
		expect(changesKey(base)).toBe(changesKey([change('a.txt'), change('b.txt', 'added')]));
		expect(changesKey(base)).not.toBe(
			changesKey([change('a.txt', 'added'), change('b.txt', 'added')])
		);
		expect(changesKey(base)).not.toBe(changesKey([change('a.txt')]));
		expect(changesKey([])).toBe('');
	});
});

describe('提案の機能が使えるか', () => {
	it('提案が「ルールのみ」なら使わない', () => {
		expect(aiSuggestionReady({ ...aiOn, memoSuggestion: 'rules' }, catalog('ready'))).toBe(false);
	});

	it('モデルが未選択なら使わない', () => {
		expect(aiSuggestionReady({ ...aiOn, aiModel: '' }, catalog('ready'))).toBe(false);
	});

	it('内蔵のモデルは導入済みのときだけ使う', () => {
		expect(aiSuggestionReady(aiOn, catalog('ready'))).toBe(true);
		expect(aiSuggestionReady(aiOn, catalog('missing'))).toBe(false);
		expect(aiSuggestionReady(aiOn, undefined)).toBe(false);
	});

	it('自分で用意した GGUF は、パスが入っていれば使える（カタログの導入状態は見ない）', () => {
		const custom = {
			...aiOn,
			aiModelSource: 'custom-gguf',
			aiCustomGgufPath: 'C:\\models\\mine.gguf'
		} as const;
		expect(aiSuggestionReady(custom, catalog('missing'))).toBe(true);
		expect(aiSuggestionReady(custom, undefined)).toBe(true);
		expect(aiSuggestionReady({ ...custom, aiCustomGgufPath: '' }, catalog('ready'))).toBe(false);
	});

	it('Ollama はモデル名があれば使える（入手元の設定には関係しない）', () => {
		const ollama = { ...aiOn, aiRunner: 'ollama', aiModel: 'gemma3:4b' } as const;
		expect(aiSuggestionReady(ollama, undefined)).toBe(true);
		expect(aiSuggestionReady({ ...ollama, aiModel: '' }, undefined)).toBe(false);
	});

	it('Ollama は接続の確かめを生成のときに行うため、ここでは見ない', () => {
		expect(aiSuggestionReady({ ...aiOn, aiRunner: 'ollama' }, undefined)).toBe(true);
	});
});

describe('「提案を作る」を出す条件', () => {
	it('使えて変更があるときだけ出す', () => {
		expect(canOfferSuggestion({ ready: true, changeCount: 2 })).toBe(true);
		expect(canOfferSuggestion({ ready: true, changeCount: 0 })).toBe(false);
		expect(canOfferSuggestion({ ready: false, changeCount: 2 })).toBe(false);
	});
});

describe('提案の状態遷移', () => {
	it('待機 → 生成中 → 結果 → 待機（使う・作り直しの前後）', () => {
		let state: SuggestState = SUGGEST_IDLE;
		state = reduceSuggest(state, { type: 'start', seq: 1 });
		expect(state).toEqual({ kind: 'generating', seq: 1 });
		state = reduceSuggest(state, { type: 'done', seq: 1, result: ok('予算を変更しました。') });
		expect(state).toEqual({ kind: 'result', text: '予算を変更しました。' });
	});

	it('失敗は理由を保持する', () => {
		let state = reduceSuggest(SUGGEST_IDLE, { type: 'start', seq: 1 });
		state = reduceSuggest(state, { type: 'done', seq: 1, result: failed('timeout') });
		expect(state).toEqual({ kind: 'failed', failure: 'timeout' });
	});

	it('取り消しの結果は画面に出さず、待機に戻す', () => {
		let state = reduceSuggest(SUGGEST_IDLE, { type: 'start', seq: 1 });
		state = reduceSuggest(state, { type: 'done', seq: 1, result: failed('cancelled') });
		expect(state).toEqual(SUGGEST_IDLE);
	});

	it('中止（reset）は生成中を待機に戻し、遅れて届いた結果を捨てる', () => {
		let state = reduceSuggest(SUGGEST_IDLE, { type: 'start', seq: 1 });
		state = reduceSuggest(state, { type: 'reset' });
		expect(state).toEqual(SUGGEST_IDLE);
		const late = reduceSuggest(state, { type: 'done', seq: 1, result: ok('遅れた結果') });
		expect(late).toEqual(SUGGEST_IDLE);
	});

	it('変更一覧が変わったら（reset）結果・失敗は破棄される', () => {
		const result = reduceSuggest({ kind: 'result', text: '古い提案' }, { type: 'reset' });
		expect(result).toEqual(SUGGEST_IDLE);
		const failure = reduceSuggest({ kind: 'failed', failure: 'invalid' }, { type: 'reset' });
		expect(failure).toEqual(SUGGEST_IDLE);
	});

	it('古い要求の結果は、新しい要求の生成中の状態を壊さない', () => {
		let state = reduceSuggest(SUGGEST_IDLE, { type: 'start', seq: 1 });
		state = reduceSuggest(state, { type: 'start', seq: 2 });
		const stale = reduceSuggest(state, { type: 'done', seq: 1, result: ok('古い') });
		expect(stale).toEqual({ kind: 'generating', seq: 2 });
		const fresh = reduceSuggest(stale, { type: 'done', seq: 2, result: ok('新しい') });
		expect(fresh).toEqual({ kind: 'result', text: '新しい' });
	});

	it('待機中に届いた結果は無視する', () => {
		const state = reduceSuggest(SUGGEST_IDLE, { type: 'done', seq: 1, result: ok('x') });
		expect(state).toEqual(SUGGEST_IDLE);
	});
});

describe('失敗の理由と文言', () => {
	const reasons: AiSuggestFailure[] = ['timeout', 'invalid', 'not-ready', 'unavailable'];

	it('失敗の理由ごとに文言のキーがあり、日本語の文言が空でない', () => {
		for (const reason of reasons) {
			const key = suggestFailureKey(reason);
			expect(ja[key], key).toBeTruthy();
		}
	});

	it('どの文言も「変更は無事」を含む（データは無事であることを示す）', () => {
		for (const reason of reasons) {
			expect(ja[suggestFailureKey(reason)], reason).toContain('変更は無事');
		}
	});
});
