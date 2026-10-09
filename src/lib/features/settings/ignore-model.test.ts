import { describe, it, expect } from 'vitest';
import {
	canSubmitIgnoreInput,
	ignoreRuleStatus,
	normalizeIgnoreInput,
	summarizeTracked
} from './ignore-model';

describe('保存しないファイルの入力', () => {
	it('拡張子は先頭の . と空白を外す', () => {
		expect(normalizeIgnoreInput('extension', ' .psd ')).toBe('psd');
		expect(normalizeIgnoreInput('extension', '..tar.gz')).toBe('tar.gz');
	});

	it('フォルダとファイルは区切りを / にそろえ、前後の / を外す', () => {
		expect(normalizeIgnoreInput('folder', '\\node_modules\\')).toBe('node_modules');
		expect(normalizeIgnoreInput('file', ' 素材\\動画 1.mp4 ')).toBe('素材/動画 1.mp4');
		expect(normalizeIgnoreInput('folder', 'a/b/')).toBe('a/b');
	});

	it('空の入力は追加できない', () => {
		expect(canSubmitIgnoreInput('extension', '  ')).toBe(false);
		expect(canSubmitIgnoreInput('extension', '.')).toBe(false);
		expect(canSubmitIgnoreInput('folder', '/')).toBe(false);
		expect(canSubmitIgnoreInput('file', 'a.txt')).toBe(true);
	});
});

describe('一覧の 1 行の状態', () => {
	it('件数の有無で分ける。解釈できない書き方は unknown', () => {
		expect(ignoreRuleStatus({ pattern: '*.psd', trackedCount: 3 })).toBe('tracked');
		expect(ignoreRuleStatus({ pattern: '*.psd', trackedCount: 0 })).toBe('none');
		expect(ignoreRuleStatus({ pattern: '!a', trackedCount: null })).toBe('unknown');
	});
});

describe('追跡中のファイルの要約', () => {
	it('区切りをそろえ、件数が多いときは残りを数える', () => {
		const paths = Array.from({ length: 23 }, (_, i) => `dir\\f${i}.psd`);
		const summary = summarizeTracked(paths, 20);
		expect(summary.shown).toHaveLength(20);
		expect(summary.shown[0]).toBe('dir/f0.psd');
		expect(summary.rest).toBe(3);
	});

	it('件数が上限以下なら残りは 0', () => {
		expect(summarizeTracked(['a.psd'])).toEqual({ shown: ['a.psd'], rest: 0 });
		expect(summarizeTracked([])).toEqual({ shown: [], rest: 0 });
	});
});
