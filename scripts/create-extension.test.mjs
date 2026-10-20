// scripts/create-extension.mjs の単体テスト（vitest）。生成は一時フォルダに行い、リポジトリには書かない。
// 生成物は core-preview の fixtures（Rust のテストが登録の検証に使う）と一致しなければならない。

import { existsSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
	buildExtensionFiles,
	createExtension,
	CreateExtensionError,
	isValidExtensionId
} from './create-extension.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
const script = join(here, 'create-extension.mjs');
const fixtureDir = join(
	here,
	'..',
	'src-tauri',
	'crates',
	'core-preview',
	'tests',
	'fixtures',
	'extension-template'
);

/** テスト用の空の一時フォルダ（OS の一時領域。削除はしない） */
function freshDir() {
	return mkdtempSync(join(tmpdir(), 'hikae-create-extension-'));
}

describe('isValidExtensionId（設計書 11.2 の id の形式）', () => {
	it.each(['a', 'a.b', 'a-b.c-d1', 'hikae.builtin.office', 'example.sample-preview'])(
		'%s は受け付ける',
		(id) => {
			expect(isValidExtensionId(id)).toBe(true);
		}
	);

	it.each(['', 'Bad', 'a..b', 'a.-b', '-a', 'a-', 'a_b', 'a b', 'a/b', '.a', 'a.', 'é'])(
		'%j は拒否する',
		(id) => {
			expect(isValidExtensionId(id)).toBe(false);
		}
	);

	it('文字列以外は拒否する', () => {
		expect(isValidExtensionId(undefined)).toBe(false);
		expect(isValidExtensionId(42)).toBe(false);
	});
});

describe('createExtension', () => {
	it('拡張の必須のキーを持つ extension.json を作る', () => {
		const outDir = freshDir();
		createExtension({ id: 'example.my-preview', name: '自作プレビュー', outDir });

		const manifest = JSON.parse(readFileSync(join(outDir, 'extension.json'), 'utf8'));
		expect(manifest.id).toBe('example.my-preview');
		expect(manifest.name).toBe('自作プレビュー');
		expect(typeof manifest.version).toBe('string');
		expect(manifest.apiVersion).toBe('1');
		expect(manifest.permissions).toEqual([]);
		expect(manifest.contributes.renderers).toHaveLength(1);
		const [renderer] = manifest.contributes.renderers;
		expect(renderer.entry).toBe('ui/index.html');
		expect(renderer.modes).toEqual(['view']);
		expect(renderer.fileTypes.every((t) => /^\.[a-z0-9]{1,16}$/.test(t))).toBe(true);
	});

	it('extension.json の entry の実体を作る', () => {
		const outDir = freshDir();
		createExtension({ id: 'example.my-preview', name: '自作プレビュー', outDir });
		expect(existsSync(join(outDir, 'ui', 'index.html'))).toBe(true);
		expect(existsSync(join(outDir, 'ui', 'main.js'))).toBe(true);
		expect(existsSync(join(outDir, 'README.md'))).toBe(true);
	});

	it('README は開発者モードでの追加の手順を日本語で書く', () => {
		const outDir = freshDir();
		createExtension({ id: 'example.my-preview', name: '自作プレビュー', outDir });
		const readme = readFileSync(join(outDir, 'README.md'), 'utf8');
		expect(readme).toContain('開発者モード');
		expect(readme).toContain('フォルダから追加');
	});

	it('出力先に既にファイルがあれば、何も書かずに失敗する', () => {
		const outDir = freshDir();
		writeFileSync(join(outDir, 'keep.txt'), 'そのまま');

		expect(() => createExtension({ id: 'example.my-preview', name: 'x', outDir })).toThrow(
			CreateExtensionError
		);
		expect(readdirSync(outDir)).toEqual(['keep.txt']);
		expect(readFileSync(join(outDir, 'keep.txt'), 'utf8')).toBe('そのまま');
	});

	it('id が不正なら、終了の失敗として何も書かない', () => {
		const outDir = freshDir();
		expect(() => createExtension({ id: 'Bad_ID', name: 'x', outDir })).toThrow(
			CreateExtensionError
		);
		expect(readdirSync(outDir)).toEqual([]);
	});

	it('表示名が空なら失敗する', () => {
		const outDir = freshDir();
		expect(() => createExtension({ id: 'example.my-preview', name: '  ', outDir })).toThrow(
			CreateExtensionError
		);
	});

	it('HTML の表示名は、タイトルの中で文字として退避される', () => {
		const files = buildExtensionFiles({ id: 'example.my-preview', name: '<b>x</b> & "y"' });
		const html = files.find((f) => f.path === 'ui/index.html').content;
		expect(html).toContain('<title>&lt;b&gt;x&lt;/b&gt; &amp; &quot;y&quot;</title>');
	});

	it('生成物は core-preview の fixtures と一致する（Rust の登録テストの入力）', () => {
		const files = buildExtensionFiles({
			id: 'example.sample-preview',
			name: 'サンプル プレビュー'
		});
		for (const file of files) {
			const expected = readFileSync(join(fixtureDir, ...file.path.split('/')), 'utf8');
			expect(file.content, file.path).toBe(expected);
		}
	});
});

describe('コマンドライン', () => {
	it('id が不正なら終了コード 1 とメッセージを返す', () => {
		const outDir = freshDir();
		const result = spawnSync(process.execPath, [script, 'Bad', '--name', 'x', '--out', outDir], {
			encoding: 'utf8'
		});
		expect(result.status).toBe(1);
		expect(result.stderr).toContain('id の形式');
		expect(readdirSync(outDir)).toEqual([]);
	});

	it('引数が足りなければ終了コード 1', () => {
		const result = spawnSync(process.execPath, [script], { encoding: 'utf8' });
		expect(result.status).toBe(1);
	});
});
