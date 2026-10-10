// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FileDiff, RendererInfo } from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';
import { renderWithClient, setupSmoke } from '../../../test/smoke';
import CompareFrame from './CompareFrame.svelte';

// 差分の結果は、テストの中で差し替える（ホストの通信の流れを確かめるため）
const api = vi.hoisted(() => ({ compare: vi.fn(), compareOffice: vi.fn(), previewFile: vi.fn() }));
vi.mock('#lib/api/index.js', () => ({ api }));

setupSmoke();

const RENDERER: RendererInfo = {
	extensionId: 'hikae.builtin.preview',
	entryUrlPath: 'hikae.builtin.preview/ui/index.html',
	fileTypes: ['*'],
	modes: ['view', 'compare']
};

const props = {
	projectId: 'proj1',
	path: 'メモ.txt',
	from: 'this',
	to: 'current',
	fromLabel: '保存した時点',
	toLabel: '今',
	renderer: RENDERER,
	mode: 'split' as const,
	activeBlock: 0 as number | null
};

const diff: FileDiff = {
	kind: 'text',
	rows: [
		{ kind: 'del', oldNo: 1, newNo: null, text: '旧' },
		{ kind: 'add', oldNo: null, newNo: 1, text: '新' }
	]
};

beforeEach(() => {
	api.compare.mockResolvedValue(diff);
	api.compareOffice.mockResolvedValue({ kind: 'unavailable', reason: 'corrupt' });
	api.previewFile.mockResolvedValue({ kind: 'not_found' });
});

afterEach(() => vi.useRealTimers());

/** 作られた iframe（拡張の表示の窓）を待つ */
async function frameOf(container: HTMLElement): Promise<HTMLIFrameElement> {
	return waitFor(() => {
		const frame = container.querySelector('iframe');
		if (!frame) throw new Error('iframe がまだ無い');
		return frame;
	});
}

describe('CompareFrame（ホスト側）', () => {
	it('iframe は sandbox を allow-scripts だけにし、ext:// の入口を使う', async () => {
		const { container } = renderWithClient(CompareFrame, props);
		const frame = await frameOf(container);
		expect(frame.getAttribute('sandbox')).toBe('allow-scripts');
		expect(frame.getAttribute('referrerpolicy')).toBe('no-referrer');
		expect(frame.getAttribute('src')).toBe('ext://localhost/hikae.builtin.preview/ui/index.html');
	});

	it('ready を受けてから比較の表示を送り、差分の行と表示の選択を含み、プロジェクトの ID は含めない', async () => {
		const { container } = renderWithClient(CompareFrame, props);
		const frame = await frameOf(container);
		const win = frame.contentWindow as Window;
		const post = vi.spyOn(win, 'postMessage').mockImplementation(() => {});
		window.dispatchEvent(
			new MessageEvent('message', { data: { v: 1, type: 'ready' }, source: win })
		);
		await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
		const [message, origin] = post.mock.calls[0];
		expect(origin).toBe('*');
		expect(message).toMatchObject({
			v: 1,
			type: 'render',
			mode: 'compare',
			left: { label: '保存した時点', file: { name: 'メモ.txt', ext: 'txt', state: 'ok' } },
			right: { label: '今', file: { name: 'メモ.txt', ext: 'txt', state: 'ok' } },
			diff: { kind: 'lines', rows: diff.kind === 'text' ? diff.rows : [] },
			options: { viewMode: 'split', activeBlock: 0 }
		});
		expect(JSON.stringify(message)).not.toContain('proj1');
		expect(JSON.stringify(message)).not.toContain('current');
		expect(JSON.stringify(message)).not.toContain('this');
	});

	it('行差分の箇所の数を親へ知らせる（前へ・次へに使う）', async () => {
		const onstatus = vi.fn();
		renderWithClient(CompareFrame, { ...props, onstatus });
		await waitFor(() =>
			expect(onstatus).toHaveBeenLastCalledWith({ textual: true, ready: true, total: 1 })
		);
	});

	it('5 秒以内に ready が来なければ、拡張の失敗（E20）を出し、並べて比べる表示へは切り替えない', async () => {
		vi.useFakeTimers();
		const onopen = vi.fn();
		const { container } = renderWithClient(CompareFrame, { ...props, onopen });
		await vi.advanceTimersByTimeAsync(10);
		expect(container.querySelector('iframe')).not.toBeNull();
		await vi.advanceTimersByTimeAsync(5200);
		expect(container.querySelector('iframe')).toBeNull();
		expect(screen.getByText(t('preview.extension.failed'))).toBeTruthy();
		expect(container.querySelector('[data-testid="side-by-side"]')).toBeNull();
		await fireEvent.click(screen.getByRole('button', { name: t('preview.extension.open') }));
		expect(onopen).toHaveBeenCalledTimes(1);
	});

	it('読み取りに失敗したら、拡張を作らずに差分の失敗の文言を出す', async () => {
		api.compare.mockRejectedValue(new Error('boom'));
		const { container } = renderWithClient(CompareFrame, props);
		expect(await screen.findByText(t('compare.office.failed'))).toBeTruthy();
		expect(container.querySelector('iframe')).toBeNull();
	});
});
