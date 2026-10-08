import { describe, it, expect } from 'vitest';
import { t } from '#lib/i18n/index.js';
import { OPEN_MENU, projectFilePath } from './file-actions';

describe('「開く」メニューの項目', () => {
	const targets = OPEN_MENU.flatMap((entry) => (entry.type === 'item' ? [entry.target] : []));

	it('既定のアプリ・フォルダで表示・パスをコピーだけを出す', () => {
		expect(targets).toEqual(['default', 'folder', 'copy_path']);
	});

	it('外部コマンドを実行する「VS Code で開く」は出さない', () => {
		expect(targets).not.toContain('vscode');
		for (const entry of OPEN_MENU) {
			if (entry.type === 'item') expect(t(entry.label)).not.toMatch(/VS Code/);
		}
	});

	it('すべての項目に文言がある', () => {
		for (const entry of OPEN_MENU) {
			if (entry.type === 'item') expect(t(entry.label)).not.toBe('');
		}
	});
});

describe('コピーするパス', () => {
	it('Windows のフォルダには区切り文字 \\ で結ぶ', () => {
		expect(projectFilePath('C:\\Users\\a\\卒論', 'docs/第1章.docx')).toBe(
			'C:\\Users\\a\\卒論\\docs\\第1章.docx'
		);
		expect(projectFilePath('C:\\Users\\a\\卒論\\', 'a.txt')).toBe('C:\\Users\\a\\卒論\\a.txt');
	});

	it('macOS のフォルダには区切り文字 / で結ぶ', () => {
		expect(projectFilePath('/Users/a/卒論', 'docs/a.txt')).toBe('/Users/a/卒論/docs/a.txt');
	});
});
