import { describe, it, expect } from 'vitest';
import type { RendererInfo } from '#lib/api/types.js';
import { compareRendererFor, extOf, extUrl, fileNameOf, rendererFor } from './renderer-select';

const renderers: RendererInfo[] = [
	{
		extensionId: 'hikae.builtin.preview',
		entryUrlPath: 'hikae.builtin.preview/ui/index.html',
		fileTypes: ['png', 'pdf'],
		modes: ['view']
	},
	{
		extensionId: 'hikae.builtin.preview',
		entryUrlPath: 'hikae.builtin.preview/ui/index.html',
		fileTypes: ['*'],
		modes: ['view']
	},
	{
		extensionId: 'test.compare-only',
		entryUrlPath: 'test.compare-only/index.html',
		fileTypes: ['doc'],
		modes: ['compare']
	}
];

describe('fileNameOf / extOf', () => {
	it('区切りは / と \\ の両方。拡張子は英小文字で返す', () => {
		expect(fileNameOf('dir/sub\\夏.PNG')).toBe('夏.PNG');
		expect(extOf('dir/sub\\夏.PNG')).toBe('png');
		expect(extOf('a.tar.gz')).toBe('gz');
	});

	it('拡張子が無いもの、先頭の . だけの名前は拡張子なし', () => {
		expect(extOf('README')).toBe('');
		expect(extOf('.gitignore')).toBe('');
		expect(extOf('dir.d/file')).toBe('');
	});
});

describe('rendererFor', () => {
	it('同じ拡張子の宣言を、`*` より優先する', () => {
		expect(rendererFor(renderers, 'png')?.fileTypes).toEqual(['png', 'pdf']);
		expect(rendererFor(renderers, 'pdf')?.fileTypes).toEqual(['png', 'pdf']);
	});

	it('宣言の無い拡張子は `*` に落ちる', () => {
		expect(rendererFor(renderers, 'txt')?.fileTypes).toEqual(['*']);
		expect(rendererFor(renderers, '')?.fileTypes).toEqual(['*']);
	});

	it('表示の方法（view）の宣言だけを対象にする。compare だけの宣言は選ばず、`*` にも落とさない', () => {
		// doc は compare だけを宣言している。宣言のある拡張子は `*` に落とさない（Rust の renderer_for と同じ規則）
		expect(rendererFor(renderers, 'doc')).toBeNull();
		expect(rendererFor([renderers[2]], 'doc')).toBeNull();
	});

	it('宣言が無ければ null', () => {
		expect(rendererFor([], 'png')).toBeNull();
	});
});

describe('compareRendererFor', () => {
	const viewOnlyPdf: RendererInfo = {
		extensionId: 'hikae.builtin.preview',
		entryUrlPath: 'hikae.builtin.preview/ui/index.html',
		fileTypes: ['pdf'],
		modes: ['view']
	};
	const withCompare: RendererInfo = {
		extensionId: 'hikae.builtin.preview',
		entryUrlPath: 'hikae.builtin.preview/ui/index.html',
		fileTypes: ['png'],
		modes: ['view', 'compare']
	};
	const fallback: RendererInfo = {
		extensionId: 'hikae.builtin.preview',
		entryUrlPath: 'hikae.builtin.preview/ui/index.html',
		fileTypes: ['*'],
		modes: ['view', 'compare']
	};
	const all = [viewOnlyPdf, withCompare, fallback];

	it('compare を宣言する拡張子の表示を選ぶ', () => {
		expect(compareRendererFor(all, 'png')).toBe(withCompare);
	});

	it('拡張子の宣言が compare を持たないときは、`*` に落とさず null（pdf は並べて比べる）', () => {
		expect(compareRendererFor(all, 'pdf')).toBeNull();
	});

	it('宣言の無い拡張子は `*` の compare を使う。`*` が view だけなら null', () => {
		expect(compareRendererFor(all, 'txt')).toBe(fallback);
		expect(compareRendererFor([viewOnlyPdf], 'txt')).toBeNull();
		expect(compareRendererFor([], 'txt')).toBeNull();
	});
});

describe('extUrl', () => {
	it('Windows の WebView は http://ext.localhost、それ以外は ext://localhost', () => {
		expect(extUrl('hikae.builtin.preview/ui/index.html', true)).toBe(
			'http://ext.localhost/hikae.builtin.preview/ui/index.html'
		);
		expect(extUrl('hikae.builtin.preview/ui/index.html', false)).toBe(
			'ext://localhost/hikae.builtin.preview/ui/index.html'
		);
	});

	it('入口のパスは URL として安全な形にする', () => {
		expect(extUrl('a.b/ui/my file.html', false)).toBe('ext://localhost/a.b/ui/my%20file.html');
	});
});
