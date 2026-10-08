import { describe, it, expect } from 'vitest';
import type { AddFilesOutcome } from '#lib/api/types.js';
import { rejectReasonText, summarizeAddFiles } from './add-files-result';

describe('ファイル追加結果のまとめ', () => {
	it('問題なく追加できたときはダイアログを出さない', () => {
		const outcome: AddFilesOutcome = {
			added: [{ path: 'a.txt', renamed: false, large: false }],
			rejected: []
		};
		expect(summarizeAddFiles(outcome)).toMatchObject({ addedCount: 1, needsDialog: false });
	});

	it('別名にしたものと大きいものを分けて数える', () => {
		const summary = summarizeAddFiles({
			added: [
				{ path: 'a (2).txt', renamed: true, large: false },
				{ path: 'b.psd', renamed: false, large: true },
				{ path: 'c.txt', renamed: false, large: false }
			],
			rejected: []
		});
		expect(summary.addedCount).toBe(3);
		expect(summary.renamed.map((f) => f.path)).toEqual(['a (2).txt']);
		expect(summary.large.map((f) => f.path)).toEqual(['b.psd']);
		expect(summary.needsDialog).toBe(true);
	});

	it('大きすぎて追加できなかったファイルがあれば印を付ける', () => {
		const summary = summarizeAddFiles({
			added: [],
			rejected: [
				{ name: 'dir', reason: 'not_a_file', size: null },
				{ name: 'movie.mp4', reason: 'too_large', size: 250 * 1024 * 1024 }
			]
		});
		expect(summary.hasTooLarge).toBe(true);
		expect(summary.addedCount).toBe(0);
		expect(summary.needsDialog).toBe(true);
	});

	it('フォルダなど大きさ以外の理由では警告表示にしない', () => {
		const summary = summarizeAddFiles({
			added: [],
			rejected: [{ name: 'dir', reason: 'not_a_file', size: null }]
		});
		expect(summary.hasTooLarge).toBe(false);
	});
});

describe('追加できなかった理由の文言', () => {
	it('大きさが分かるときは大きさを添える', () => {
		expect(rejectReasonText({ name: 'm', reason: 'too_large', size: 250 * 1024 * 1024 })).toContain(
			'250.0 MB'
		);
	});

	it('大きさが不明でも文言を返す', () => {
		expect(rejectReasonText({ name: 'm', reason: 'too_large', size: null })).toContain('100MB');
		expect(rejectReasonText({ name: 'x', reason: 'unreadable', size: null })).not.toBe('');
	});
});
