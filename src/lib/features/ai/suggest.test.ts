import { describe, it, expect } from 'vitest';
import type { AiCatalog, AiModel, Change } from '#lib/api/types.js';
import {
	aiSuggestionReady,
	changesKey,
	shouldRequestSuggestion,
	SUGGESTION_DEBOUNCE_MS
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

const aiOn = { memoSuggestion: 'ai', aiRunner: 'builtin', aiModel: 'gemma' } as const;

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

	it('待ち時間は 1.5 秒', () => {
		expect(SUGGESTION_DEBOUNCE_MS).toBe(1500);
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

	it('Ollama は接続の確かめを生成のときに行うため、ここでは見ない', () => {
		expect(aiSuggestionReady({ ...aiOn, aiRunner: 'ollama' }, undefined)).toBe(true);
	});
});

describe('提案を取りに行く条件', () => {
	const base = { ready: true, projectId: 'p1', changeCount: 2, debouncedKey: 'k' };

	it('すべて揃ったときだけ取りに行く', () => {
		expect(shouldRequestSuggestion(base)).toBe(true);
	});

	it('変更がない、待ち時間の途中、準備ができていない場合は取りに行かない', () => {
		expect(shouldRequestSuggestion({ ...base, changeCount: 0 })).toBe(false);
		expect(shouldRequestSuggestion({ ...base, debouncedKey: '' })).toBe(false);
		expect(shouldRequestSuggestion({ ...base, ready: false })).toBe(false);
		expect(shouldRequestSuggestion({ ...base, projectId: '' })).toBe(false);
	});
});
