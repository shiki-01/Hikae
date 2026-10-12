import { describe, it, expect } from 'vitest';
import type { AiModel } from '#lib/api/types.js';
import {
	formatBytes,
	modelButtons,
	progressPercent,
	recommendationKey,
	unavailableMessage
} from './model-view';

const GB = 1024 ** 3;

function model(overrides: Partial<AiModel> = {}): AiModel {
	return {
		id: 'm',
		displayName: 'M',
		stage: 2,
		sizeBytes: 3 * GB,
		licenseName: 'Apache License 2.0',
		licenseUrl: 'https://www.apache.org/licenses/LICENSE-2.0',
		minMemoryBytes: 8 * GB,
		recommended: true,
		recommendedForThisPc: false,
		status: 'missing',
		partialBytes: 0,
		available: true,
		unavailableReason: null,
		downloading: false,
		...overrides
	};
}

describe('容量と進捗の表示', () => {
	it('1 GB 以上は小数 1 桁、末尾の .0 は付けない', () => {
		expect(formatBytes(8 * GB)).toBe('8 GB');
		expect(formatBytes(2.5 * GB)).toBe('2.5 GB');
		expect(formatBytes(3_106_738_272)).toBe('2.9 GB');
	});

	it('1 GB 未満は MB', () => {
		expect(formatBytes(512 * 1024 * 1024)).toBe('512 MB');
		expect(formatBytes(1024 * 1024)).toBe('1 MB');
		expect(formatBytes(0)).toBe('0 MB');
	});

	it('進捗は 0〜100 の整数に収め、総量が不明なら 0', () => {
		expect(progressPercent(50, 200)).toBe(25);
		expect(progressPercent(199, 200)).toBe(100);
		expect(progressPercent(300, 200)).toBe(100);
		expect(progressPercent(10, 0)).toBe(0);
		expect(progressPercent(-5, 100)).toBe(0);
	});
});

describe('導入できない理由の文言', () => {
	it('メモリ不足は必要量と PC の量を差し込む', () => {
		expect(unavailableMessage({ kind: 'memory', needed: 8 * GB, have: 6 * GB })).toEqual({
			key: 'ai.unavailable_memory',
			params: { needed: '8 GB', have: '6 GB' }
		});
	});

	it('空き容量不足は disk のキーになる', () => {
		expect(unavailableMessage({ kind: 'disk', needed: 4 * GB, have: 1 * GB }).key).toBe(
			'ai.unavailable_disk'
		);
	});
});

describe('モデルの行のボタン', () => {
	it('未導入で導入可能なら「ダウンロード」だけ', () => {
		expect(modelButtons(model(), false)).toEqual({
			download: true,
			resume: false,
			cancel: false,
			use: false,
			remove: false
		});
	});

	it('導入できない PC では、取得を始めるボタンを出さない', () => {
		const buttons = modelButtons(model({ available: false }), false);
		expect(buttons.download).toBe(false);
		expect(buttons.resume).toBe(false);
	});

	it('ダウンロード中は「中止」だけ', () => {
		expect(modelButtons(model({ downloading: true, status: 'partial' }), false)).toEqual({
			download: false,
			resume: false,
			cancel: true,
			use: false,
			remove: false
		});
	});

	it('途中まで取得したものは「再開」と「削除」', () => {
		const buttons = modelButtons(model({ status: 'partial', partialBytes: GB }), false);
		expect(buttons.resume).toBe(true);
		expect(buttons.remove).toBe(true);
		expect(buttons.download).toBe(false);
	});

	it('導入済みで選ばれていなければ「使う」、選ばれていれば「使う」は出さない', () => {
		expect(modelButtons(model({ status: 'ready' }), false).use).toBe(true);
		expect(modelButtons(model({ status: 'ready' }), true).use).toBe(false);
		expect(modelButtons(model({ status: 'ready' }), true).remove).toBe(true);
	});
});

describe('推奨のバッジ（この PC のおすすめ を優先する）', () => {
	it('この PC のおすすめは、段階の推奨より優先して出す', () => {
		expect(recommendationKey({ recommended: true, recommendedForThisPc: true })).toBe(
			'ai.recommended_for_pc'
		);
		expect(recommendationKey({ recommended: false, recommendedForThisPc: true })).toBe(
			'ai.recommended_for_pc'
		);
	});

	it('この PC のおすすめでなければ、段階の推奨（既定）を出し、どちらでもなければ出さない', () => {
		expect(recommendationKey({ recommended: true, recommendedForThisPc: false })).toBe(
			'ai.recommended'
		);
		expect(recommendationKey({ recommended: false, recommendedForThisPc: false })).toBeNull();
	});
});
