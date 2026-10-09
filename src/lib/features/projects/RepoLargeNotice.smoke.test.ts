// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { t } from '#lib/i18n/index.js';
import RepoLargeNotice from './RepoLargeNotice.svelte';
import CloudSyncNotice from './CloudSyncNotice.svelte';

setupSmoke();

describe('RepoLargeNotice のスモークテスト（E09）', () => {
	it('容量の注意が出て、確認のボタンで通知される', async () => {
		const onreview = vi.fn();
		renderWithClient(RepoLargeNotice, { onreview });
		expect(screen.getByText(t('repo_large.message'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('repo_large.review') }));
		expect(onreview).toHaveBeenCalledTimes(1);
	});
});

describe('CloudSyncNotice のスモークテスト', () => {
	it('注意と 2 つの選択が出る', async () => {
		const onkeep = vi.fn();
		const onchange = vi.fn();
		renderWithClient(CloudSyncNotice, { accepted: false, onkeep, onchange });
		expect(screen.getByText(t('cloud_sync.message'))).toBeTruthy();
		await fireEvent.click(screen.getByRole('button', { name: t('cloud_sync.keep') }));
		await fireEvent.click(screen.getByRole('button', { name: t('cloud_sync.change') }));
		expect(onkeep).toHaveBeenCalledTimes(1);
		expect(onchange).toHaveBeenCalledTimes(1);
	});

	it('選択したあとは「このまま追加する」を出さず、その旨を示す', () => {
		renderWithClient(CloudSyncNotice, { accepted: true, onkeep: () => {}, onchange: () => {} });
		expect(screen.queryByRole('button', { name: t('cloud_sync.keep') })).toBeNull();
		expect(screen.getByText(t('cloud_sync.kept'))).toBeTruthy();
	});
});
