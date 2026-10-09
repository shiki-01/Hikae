import { describe, it, expect } from 'vitest';
import type { AddFilesOutcome, AddedFile } from '#lib/api/types.js';
import { rejectReasonText, summarizeAddFiles } from './add-files-result';

function file(path: string, extra: Partial<AddedFile> = {}): AddedFile {
	return {
		path,
		renamed: false,
		large: false,
		replaced: false,
		replaceRefused: false,
		...extra
	};
}

function outcome(extra: Partial<AddFilesOutcome> = {}): AddFilesOutcome {
	return {
		added: [],
		rejected: [],
		skipped: [],
		needsDecision: [],
		undoToken: null,
		...extra
	};
}

describe('ファイル追加結果のまとめ', () => {
	it('問題なく追加できたときはダイアログを出さない', () => {
		const summary = summarizeAddFiles(outcome({ added: [file('a.txt')] }));
		expect(summary).toMatchObject({ addedCount: 1, needsDialog: false });
	});

	it('別名にしたものと大きいものを分けて数える', () => {
		const summary = summarizeAddFiles(
			outcome({
				added: [file('a (2).txt', { renamed: true }), file('b.psd', { large: true }), file('c.txt')]
			})
		);
		expect(summary.addedCount).toBe(3);
		expect(summary.renamed.map((f) => f.path)).toEqual(['a (2).txt']);
		expect(summary.large.map((f) => f.path)).toEqual(['b.psd']);
		expect(summary.needsDialog).toBe(true);
	});

	it('置き換えたもの・置き換えられず別名にしたもの・別名にしたものを分ける', () => {
		const summary = summarizeAddFiles(
			outcome({
				added: [
					file('r.txt', { replaced: true }),
					file('x (2).psd', { renamed: true, replaceRefused: true }),
					file('k (2).txt', { renamed: true })
				]
			})
		);
		expect(summary.replaced.map((f) => f.path)).toEqual(['r.txt']);
		expect(summary.replaceRefused.map((f) => f.path)).toEqual(['x (2).psd']);
		expect(summary.renamed.map((f) => f.path)).toEqual(['k (2).txt']);
		expect(summary.needsDialog).toBe(true);
	});

	it('大きすぎて追加できなかったファイルがあれば印を付ける', () => {
		const summary = summarizeAddFiles(
			outcome({
				rejected: [
					{ name: 'dir', reason: 'not_a_file', size: null },
					{ name: 'movie.mp4', reason: 'too_large', size: 250 * 1024 * 1024 }
				]
			})
		);
		expect(summary.hasTooLarge).toBe(true);
		expect(summary.addedCount).toBe(0);
		expect(summary.needsDialog).toBe(true);
	});

	it('大きさ以外の理由では警告表示にしない', () => {
		const summary = summarizeAddFiles(
			outcome({ rejected: [{ name: 'dir', reason: 'not_a_file', size: null }] })
		);
		expect(summary.hasTooLarge).toBe(false);
	});

	it('リンクは理由つきで知らせ、隠しファイル・一時ファイルは件数だけにする', () => {
		const links = Array.from({ length: 25 }, (_, i) => ({
			name: `資料/リンク${i}`,
			reason: 'link' as const
		}));
		const summary = summarizeAddFiles(
			outcome({
				added: [file('資料/a.txt')],
				skipped: [
					...links,
					{ name: '資料/.DS_Store', reason: 'os_temp' },
					{ name: '資料/.hidden', reason: 'hidden' }
				]
			})
		);
		expect(summary.skippedLinkCount).toBe(25);
		expect(summary.skippedLinks).toHaveLength(20);
		expect(summary.skippedHiddenCount).toBe(2);
		expect(summary.needsDialog).toBe(true);
	});

	it('隠しファイルを飛ばしただけなら、ダイアログは出さない', () => {
		const summary = summarizeAddFiles(
			outcome({
				added: [file('資料/a.txt')],
				skipped: [{ name: '資料/Thumbs.db', reason: 'os_temp' }]
			})
		);
		expect(summary.skippedHiddenCount).toBe(1);
		expect(summary.needsDialog).toBe(false);
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
		expect(rejectReasonText({ name: 'x', reason: 'in_use', size: null })).not.toBe('');
	});
});
