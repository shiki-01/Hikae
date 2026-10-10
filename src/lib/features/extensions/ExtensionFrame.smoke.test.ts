// @vitest-environment jsdom
import { fireEvent, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { FilePreview, RendererInfo } from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';
import { renderWithClient, setupSmoke } from '../../../test/smoke';
import ExtensionFrame from './ExtensionFrame.svelte';

// 表示拡張の一覧と内容は、テストの中で差し替える（ホストの通信の流れを確かめるため）
const api = vi.hoisted(() => ({ listRenderers: vi.fn(), previewFile: vi.fn() }));
vi.mock('#lib/api/index.js', () => ({ api }));

setupSmoke();

const BUILTIN: RendererInfo = {
	extensionId: 'hikae.builtin.preview',
	entryUrlPath: 'hikae.builtin.preview/ui/index.html',
	fileTypes: ['*'],
	modes: ['view']
};

const props = { projectId: 'proj1', rev: 'current', path: 'メモ.txt', label: '今' };

beforeEach(() => {
	api.listRenderers.mockResolvedValue([BUILTIN]);
	api.previewFile.mockResolvedValue({
		kind: 'text',
		content: '本文',
		truncated: false
	} satisfies FilePreview);
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

describe('ExtensionFrame（ホスト側）', () => {
	it('拡張が無いときは iframe を作らず、拡張の失敗の文言と「ファイルを開く」を出す', async () => {
		api.listRenderers.mockResolvedValue([]);
		const onopen = vi.fn();
		const { container } = renderWithClient(ExtensionFrame, { ...props, onopen });
		expect(await screen.findByText(t('preview.extension.failed'))).toBeTruthy();
		expect(container.querySelector('iframe')).toBeNull();
		await fireEvent.click(screen.getByRole('button', { name: t('preview.extension.open') }));
		expect(onopen).toHaveBeenCalledTimes(1);
	});

	it('iframe は sandbox を allow-scripts だけにし、ext:// の入口を使う', async () => {
		const { container } = renderWithClient(ExtensionFrame, props);
		const frame = await frameOf(container);
		expect(frame.getAttribute('sandbox')).toBe('allow-scripts');
		expect(frame.getAttribute('referrerpolicy')).toBe('no-referrer');
		expect(frame.getAttribute('src')).toBe('ext://localhost/hikae.builtin.preview/ui/index.html');
	});

	it('ready を受けてから内容を送り、送るのは名前と内容だけ', async () => {
		const { container } = renderWithClient(ExtensionFrame, props);
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
			mode: 'view',
			file: { name: 'メモ.txt', ext: 'txt', state: 'ok' },
			content: { kind: 'text', text: '本文', truncated: false }
		});
		// 送らないもの（プロジェクトの ID・パス・設定）
		expect(JSON.stringify(message)).not.toContain('proj1');
		expect(JSON.stringify(message)).not.toContain('current');
	});

	it('別の窓からのメッセージは無視する', async () => {
		const { container } = renderWithClient(ExtensionFrame, props);
		const frame = await frameOf(container);
		const post = vi
			.spyOn(frame.contentWindow as Window, 'postMessage')
			.mockImplementation(() => {});
		window.dispatchEvent(
			new MessageEvent('message', { data: { v: 1, type: 'ready' }, source: window })
		);
		await new Promise((resolve) => setTimeout(resolve, 20));
		expect(post).not.toHaveBeenCalled();
	});

	it('5 秒以内に ready が来なければ、拡張の失敗の表示に切り替える', async () => {
		vi.useFakeTimers();
		const { container } = renderWithClient(ExtensionFrame, props);
		await vi.advanceTimersByTimeAsync(10);
		expect(container.querySelector('iframe')).not.toBeNull();
		await vi.advanceTimersByTimeAsync(4900);
		expect(container.querySelector('iframe')).not.toBeNull();
		await vi.advanceTimersByTimeAsync(200);
		expect(container.querySelector('iframe')).toBeNull();
		expect(screen.getByText(t('preview.extension.failed'))).toBeTruthy();
	});

	it('拡張の失敗は、送った表示の番号と同じときだけ切り替える', async () => {
		const { container } = renderWithClient(ExtensionFrame, props);
		const frame = await frameOf(container);
		const win = frame.contentWindow as Window;
		const post = vi.spyOn(win, 'postMessage').mockImplementation(() => {});
		window.dispatchEvent(
			new MessageEvent('message', { data: { v: 1, type: 'ready' }, source: win })
		);
		await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
		const id = (post.mock.calls[0][0] as { id: number }).id;

		window.dispatchEvent(
			new MessageEvent('message', {
				data: { v: 1, type: 'error', id: id + 100, code: 'render-failed' },
				source: win
			})
		);
		await new Promise((resolve) => setTimeout(resolve, 20));
		expect(container.querySelector('iframe')).not.toBeNull();

		window.dispatchEvent(
			new MessageEvent('message', {
				data: { v: 1, type: 'error', id, code: 'render-failed' },
				source: win
			})
		);
		await waitFor(() => expect(container.querySelector('iframe')).toBeNull());
		expect(screen.getByText(t('preview.extension.failed'))).toBeTruthy();
	});
});
