// @vitest-environment jsdom
import { waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileDiff, FilePreview, RendererInfo } from '#lib/api/types.js';
import { renderWithClient, setupSmoke } from '../../../test/smoke';
import FileDiffPane from './FileDiffPane.svelte';

// 表示の一覧と差分の結果は、テストの中で差し替える（描き方の振り分けを確かめるため）
const api = vi.hoisted(() => ({
	listRenderers: vi.fn(),
	compare: vi.fn(),
	compareOffice: vi.fn(),
	previewFile: vi.fn()
}));
vi.mock('#lib/api/index.js', () => ({ api }));

setupSmoke();

const BUILTIN_VIEW_AND_COMPARE: RendererInfo = {
	extensionId: 'hikae.builtin.preview',
	entryUrlPath: 'hikae.builtin.preview/ui/index.html',
	fileTypes: ['*'],
	modes: ['view', 'compare']
};
// pdf・pptx は compare を宣言しない（設計書 11.2）。ここでは宣言を分けて置く
const PDF_VIEW_ONLY: RendererInfo = {
	extensionId: 'hikae.builtin.preview',
	entryUrlPath: 'hikae.builtin.preview/ui/index.html',
	fileTypes: ['pdf'],
	modes: ['view']
};
const FALLBACK_VIEW_ONLY: RendererInfo = {
	extensionId: 'hikae.builtin.preview',
	entryUrlPath: 'hikae.builtin.preview/ui/index.html',
	fileTypes: ['*'],
	modes: ['view']
};

const props = {
	projectId: 'proj1',
	from: 'this',
	to: 'current',
	fromLabel: '保存した時点',
	toLabel: '今'
};

const text: FileDiff = { kind: 'text', rows: [{ kind: 'add', oldNo: null, newNo: 1, text: '新' }] };

beforeEach(() => {
	api.listRenderers.mockResolvedValue([BUILTIN_VIEW_AND_COMPARE, PDF_VIEW_ONLY]);
	api.compare.mockResolvedValue(text);
	api.compareOffice.mockResolvedValue({ kind: 'unavailable', reason: 'corrupt' });
	api.previewFile.mockResolvedValue({ kind: 'not_found' } satisfies FilePreview);
});

describe('FileDiffPane の振り分け（ホスト側）', () => {
	it('pdf は compare を宣言しないため、並べて比べる（差分の取得はしない）', async () => {
		const { container } = renderWithClient(FileDiffPane, { ...props, path: 'a.pdf' });
		await waitFor(() =>
			expect(container.querySelector('[data-testid="side-by-side"]')).not.toBeNull()
		);
		expect(container.querySelector('[data-testid="file-compare"]')).toBeNull();
		expect(api.compare).not.toHaveBeenCalled();
	});

	it('compare を宣言する表示があるテキストは、拡張で比べる', async () => {
		const { container } = renderWithClient(FileDiffPane, { ...props, path: 'メモ.txt' });
		await waitFor(() =>
			expect(container.querySelector('[data-testid="file-compare"]')).not.toBeNull()
		);
		expect(container.querySelector('[data-testid="side-by-side"]')).toBeNull();
	});

	it('テキストとして比べられない（情報のみ）は、並べて比べる', async () => {
		api.compare.mockResolvedValue({
			kind: 'info',
			fileKind: 'unknown',
			sizeBytes: 4,
			modifiedAt: null
		});
		const { container } = renderWithClient(FileDiffPane, { ...props, path: 'data.bin' });
		await waitFor(() =>
			expect(container.querySelector('[data-testid="side-by-side"]')).not.toBeNull()
		);
		expect(container.querySelector('[data-testid="file-compare"]')).toBeNull();
	});

	it('compare を宣言する表示が無ければ（`*` が view だけ）、並べて比べる', async () => {
		api.listRenderers.mockResolvedValue([FALLBACK_VIEW_ONLY]);
		const { container } = renderWithClient(FileDiffPane, { ...props, path: 'メモ.txt' });
		await waitFor(() =>
			expect(container.querySelector('[data-testid="side-by-side"]')).not.toBeNull()
		);
		expect(container.querySelector('[data-testid="file-compare"]')).toBeNull();
	});
});
