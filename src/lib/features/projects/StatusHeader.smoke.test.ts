// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { t } from '#lib/i18n/index.js';
import StatusHeader from '#lib/components/StatusHeader.svelte';

setupSmoke();

describe('StatusHeader のスモークテスト（再ログイン）', () => {
	it('再ログインが必要なときは、ログインし直すボタンが出て押せる', async () => {
		const onrelogin = vi.fn();
		renderWithClient(StatusHeader, {
			projectName: 'sample',
			attention: 'auth' as const,
			onrelogin
		});
		expect(screen.getByText(t('header.status_attention_auth'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('header.relogin') }));
		expect(onrelogin).toHaveBeenCalledTimes(1);
	});

	it('未保存の変更を理由に見送ったときは、ログインし直すボタンを出さない', () => {
		renderWithClient(StatusHeader, {
			projectName: 'sample',
			attention: 'unsaved-changes' as const
		});
		expect(screen.getByText(t('header.status_attention_unsaved'))).toBeTruthy();
		expect(screen.queryByRole('button', { name: t('header.relogin') })).toBeNull();
	});
});
