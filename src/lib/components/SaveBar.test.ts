// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setupSmoke } from '../../test/smoke';
import { t } from '#lib/i18n/index.js';
import type { SuggestState } from '#lib/features/ai/suggest.js';
import SaveBar from './SaveBar.svelte';

setupSmoke();

// jsdom は <dialog> の showModal を持たないため、開閉だけを模した代わりを使う
beforeEach(() => {
	HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
		this.open = true;
	};
	HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
		this.open = false;
	};
});

afterEach(() => cleanup());

function renderBar(overrides: Record<string, unknown> = {}) {
	const onsave = vi.fn();
	const onusesuggestion = vi.fn();
	const onundosuggestion = vi.fn();
	const result = render(SaveBar, {
		props: {
			memo: '税率を変えた',
			detail: '',
			hasChanges: true,
			onsave,
			onusesuggestion,
			onundosuggestion,
			...overrides
		}
	});
	return { ...result, onsave, onusesuggestion, onundosuggestion };
}

describe('SaveBar の詳細（任意）', () => {
	it('詳細は最初は折りたたまれ、「詳細を書く」で開く', async () => {
		renderBar();
		const open = screen.getByRole('button', { name: t('save_bar.detail_open') });
		expect(open.getAttribute('aria-expanded')).toBe('false');
		expect(screen.queryByRole('textbox', { name: t('save_bar.detail') })).toBeNull();

		await fireEvent.click(open);
		expect(screen.getByRole('textbox', { name: t('save_bar.detail') })).toBeTruthy();
		const close = screen.getByRole('button', { name: t('save_bar.detail_close') });
		expect(close.getAttribute('aria-expanded')).toBe('true');
	});

	it('詳細が入っている状態で開くと、詳細の欄が見える', () => {
		renderBar({ detail: '・一行目', detailOpen: true });
		expect(screen.getByRole('textbox', { name: t('save_bar.detail') })).toBeTruthy();
	});

	it('注意書きは常に出る（履歴に残り、アップロードされること）', () => {
		renderBar();
		expect(screen.getByText(t('save_bar.note'))).toBeTruthy();
	});
});

describe('SaveBar の機微情報の警告と確認', () => {
	it('機微に見える文字列があると、警告を role=alert で出す', () => {
		renderBar({ sensitive: ['email'] });
		const alert = screen.getByRole('alert');
		expect(alert.textContent).toContain(t('save_bar.sensitive_warning'));
	});

	it('警告があるときの保存は、確認ダイアログを先に出す（「戻って直す」が既定）', async () => {
		const { onsave } = renderBar({ sensitive: ['phone'] });
		await fireEvent.click(screen.getByRole('button', { name: t('save_bar.button') }));
		expect(onsave).not.toHaveBeenCalled();
		const back = screen.getByRole('button', { name: t('save_bar.sensitive_back') });
		expect(document.activeElement).toBe(back);
	});

	it('確認で「このまま保存」を押すと保存する', async () => {
		const { onsave } = renderBar({ sensitive: ['phone'] });
		await fireEvent.click(screen.getByRole('button', { name: t('save_bar.button') }));
		await fireEvent.click(screen.getByRole('button', { name: t('save_bar.sensitive_confirm') }));
		expect(onsave).toHaveBeenCalledTimes(1);
	});

	it('警告が無いときは、そのまま保存する', async () => {
		const { onsave } = renderBar({ sensitive: [] });
		await fireEvent.click(screen.getByRole('button', { name: t('save_bar.button') }));
		expect(onsave).toHaveBeenCalledTimes(1);
	});
});

describe('SaveBar の AI 提案の「使う」と「元に戻す」', () => {
	const result: SuggestState = {
		kind: 'result',
		title: '税率を8%から10%に変えた',
		body: '・税率を8%から10%に変更',
		notices: ['sensitive-masked', 'body-dropped-copy']
	};

	it('「使う」は要約と本文を渡し、短い注記を出す', async () => {
		const { onusesuggestion } = renderBar({ canSuggest: true, suggestState: result });
		expect(screen.getByText(t('save_bar.notice.sensitive-masked'))).toBeTruthy();
		expect(screen.getByText(t('save_bar.notice.body-dropped-copy'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('save_bar.ai_use') }));
		expect(onusesuggestion).toHaveBeenCalledWith({
			title: '税率を8%から10%に変えた',
			body: '・税率を8%から10%に変更'
		});
	});

	it('使った直後だけ「元に戻す」を出し、押すと前の下書きに戻す操作を呼ぶ', async () => {
		const { onundosuggestion } = renderBar({
			canSuggest: true,
			suggestState: result,
			undoable: true
		});
		await fireEvent.click(screen.getByRole('button', { name: t('save_bar.ai_undo') }));
		expect(onundosuggestion).toHaveBeenCalledTimes(1);
	});

	it('「元に戻す」は、使っていないときは出さない', () => {
		renderBar({ canSuggest: true, suggestState: result, undoable: false });
		expect(screen.queryByRole('button', { name: t('save_bar.ai_undo') })).toBeNull();
	});
});

describe('SaveBar の内容の無い変更と、本文を捨てた理由の注記', () => {
	it('内容の無い変更は、注記だけを出し、再試行を出さない（失敗の表示ではない）', () => {
		renderBar({ canSuggest: true, suggestState: { kind: 'no-content' } });
		expect(screen.getByText(t('save_bar.no_content'))).toBeTruthy();
		expect(screen.queryByRole('button', { name: t('save_bar.ai_retry') })).toBeNull();
		expect(screen.queryByText(t('save_bar.ai_failed.invalid'))).toBeNull();
	});

	it('本文を捨てた理由ごとに、理由を添えた注記を出す', () => {
		for (const notice of [
			'body-dropped-copy',
			'body-dropped-ungrounded',
			'body-dropped-sensitive'
		] as const) {
			cleanup();
			renderBar({
				canSuggest: true,
				suggestState: { kind: 'result', title: '要約', body: null, notices: [notice] }
			});
			expect(screen.getByText(t(`save_bar.notice.${notice}`))).toBeTruthy();
		}
	});
});
