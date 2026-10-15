// @vitest-environment jsdom
// モバイル版の画面のスモークテスト（モックの API）。M0〜M5 の主な遷移と、同名の 3 択を確かめる。
import { fireEvent, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { t } from '#lib/i18n/index.js';
import { mobileApi } from '#lib/api/mobile-index.js';
import MobileShell from './MobileShell.svelte';
import { resetNav } from './nav.svelte';

setupSmoke();

const OWNER = t('mock.mobile.author');
const FAMILY = t('mock.mobile.repo.family');
const DOCS = t('mock.mobile.dir.docs');
const MINUTES = t('mock.mobile.file.minutes');

beforeEach(() => {
	localStorage.clear();
	localStorage.setItem('hikae.mobile.session', JSON.stringify({ loggedIn: true }));
	resetNav();
	vi.restoreAllMocks();
});

/** 保存先の一覧から家計メモを選び、フォルダの一覧が出るまで待つ */
async function openFamilyRepo(): Promise<void> {
	await fireEvent.click(await screen.findByRole('button', { name: new RegExp(FAMILY) }));
	await screen.findByRole('heading', { name: `${OWNER} / ${FAMILY}` });
}

async function openTab(label: string): Promise<void> {
	await fireEvent.click(screen.getByRole('button', { name: label }));
}

describe('モバイル版のスモークテスト', () => {
	it('未ログインのときは、ログインの画面を出す（M0）', async () => {
		localStorage.clear();
		renderWithClient(MobileShell);
		expect(await screen.findByRole('button', { name: t('mobile.login.button') })).toBeTruthy();
	});

	it('保存先を選び、フォルダを開き、テキストをプレビューで開ける（M1 → M2 → M3）', async () => {
		renderWithClient(MobileShell);
		expect(await screen.findByRole('heading', { name: t('mobile.repos.title') })).toBeTruthy();
		await openFamilyRepo();

		await fireEvent.click(await screen.findByText(DOCS));
		await fireEvent.click(await screen.findByText(MINUTES));
		expect(await screen.findByRole('heading', { name: MINUTES })).toBeTruthy();
		expect(await screen.findByText(/打ち合わせ/)).toBeTruthy();
		expect(screen.getByRole('button', { name: t('mobile.common.download') })).toBeTruthy();

		await fireEvent.click(screen.getByRole('button', { name: t('mobile.common.back') }));
		expect(await screen.findByText(MINUTES)).toBeTruthy();
	});

	it('ファイルの「⋯」から過去の版を一覧し、版を表示できる（M2 → 過去の版）', async () => {
		renderWithClient(MobileShell);
		await openFamilyRepo();
		await fireEvent.click(await screen.findByText(DOCS));

		await fireEvent.click(
			await screen.findByRole('button', { name: t('mobile.files.more', { name: MINUTES }) })
		);
		await fireEvent.click(screen.getByRole('button', { name: t('mobile.common.versions') }));
		const views = await screen.findAllByRole('button', { name: t('mobile.file.view_version') });
		expect(views).toHaveLength(2);
		await fireEvent.click(views[0]);
		expect(await screen.findByRole('heading', { name: MINUTES })).toBeTruthy();
		expect(await screen.findByText(/打ち合わせ/)).toBeTruthy();
	});

	it('過去の版を開くと preview は blob の sha と版の ref 付きで呼ばれ、現在の版は ref なし', async () => {
		const preview = vi.spyOn(mobileApi, 'preview');
		renderWithClient(MobileShell);
		await openFamilyRepo();
		await fireEvent.click(await screen.findByText(DOCS));
		const path = `${DOCS}/${MINUTES}`;
		const listing = await mobileApi.listDir(OWNER, FAMILY, DOCS);
		const blobSha = listing.entries.find((entry) => entry.name === MINUTES)?.sha;
		const versions = await mobileApi.fileHistory(OWNER, FAMILY, path);

		// 現在の版は、ref を渡さない
		await fireEvent.click(await screen.findByText(MINUTES));
		await screen.findByRole('heading', { name: MINUTES });
		expect(preview).toHaveBeenLastCalledWith(OWNER, FAMILY, path, blobSha, undefined);

		// 過去の版は、sha に blob の sha、ref に版のコミットの sha を渡す
		await fireEvent.click(screen.getByRole('button', { name: t('mobile.common.back') }));
		await fireEvent.click(
			await screen.findByRole('button', { name: t('mobile.files.more', { name: MINUTES }) })
		);
		await fireEvent.click(screen.getByRole('button', { name: t('mobile.common.versions') }));
		await fireEvent.click(
			(await screen.findAllByRole('button', { name: t('mobile.file.view_version') }))[0]
		);
		await screen.findByRole('heading', { name: MINUTES });
		await vi.waitFor(() =>
			expect(preview).toHaveBeenLastCalledWith(OWNER, FAMILY, path, blobSha, versions[0].sha)
		);
		expect(versions[0].sha).not.toBe(blobSha);
	});

	it('テキストを追加して「追加して保存」を押すと、同名がなければそのまま保存される（M4）', async () => {
		const upload = vi.spyOn(mobileApi, 'upload');
		renderWithClient(MobileShell);
		await openFamilyRepo();
		await openTab(t('mobile.tab.upload'));

		await fireEvent.click(screen.getByRole('button', { name: t('mobile.upload.add_text') }));
		const area = screen.getByRole('textbox', { name: t('mobile.upload.text_placeholder') });
		await fireEvent.input(area, { target: { value: 'メモの本文' } });
		await fireEvent.click(screen.getByRole('button', { name: t('mobile.upload.text_add') }));
		await fireEvent.click(await screen.findByRole('button', { name: t('mobile.upload.submit') }));

		await vi.waitFor(() => expect(upload).toHaveBeenCalledTimes(1));
		expect(upload).toHaveBeenCalledWith(
			OWNER,
			FAMILY,
			t('mobile.upload.default_message', { n: 1 }),
			[
				expect.objectContaining({
					targetDir: '',
					name: expect.stringMatching(/^メモ_/),
					onExists: 'keepBoth',
					contentBase64: expect.any(String)
				})
			]
		);
		expect(await screen.findByText(t('mobile.upload.result_title'))).toBeTruthy();
	});

	it('同名があるときは 3 択を出し、もう一度押すまで保存しない（M4）', async () => {
		const upload = vi.spyOn(mobileApi, 'upload');
		renderWithClient(MobileShell);
		await openFamilyRepo();
		await openTab(t('mobile.tab.upload'));

		await fireEvent.click(screen.getByRole('button', { name: t('mobile.upload.add_text') }));
		await fireEvent.input(
			screen.getByRole('textbox', { name: t('mobile.upload.text_placeholder') }),
			{
				target: { value: '上書きの確認' }
			}
		);
		await fireEvent.click(screen.getByRole('button', { name: t('mobile.upload.text_add') }));
		await fireEvent.input(screen.getByRole('textbox', { name: t('mobile.upload.item_name') }), {
			target: { value: 'README.md' }
		});
		await fireEvent.click(screen.getByRole('button', { name: t('mobile.upload.submit') }));

		expect(
			await screen.findByRole('radio', { name: t('mobile.upload.conflict_replace') })
		).toBeTruthy();
		expect(screen.getByRole('radio', { name: t('mobile.upload.conflict_keep_both') })).toBeTruthy();
		expect(screen.getByRole('radio', { name: t('mobile.upload.conflict_skip') })).toBeTruthy();
		expect(screen.getByText(t('mobile.upload.conflict_confirm_hint'))).toBeTruthy();
		expect(upload).not.toHaveBeenCalled();

		await fireEvent.click(screen.getByRole('radio', { name: t('mobile.upload.conflict_replace') }));
		await fireEvent.click(screen.getByRole('button', { name: t('mobile.upload.submit') }));
		await vi.waitFor(() => expect(upload).toHaveBeenCalledTimes(1));
		expect(upload.mock.calls[0][3][0]).toMatchObject({ name: 'README.md', onExists: 'replace' });
	});

	it('設定からログアウトすると、ログインの画面へ戻る（M5）', async () => {
		renderWithClient(MobileShell);
		await openFamilyRepo();
		await openTab(t('mobile.tab.settings'));
		await fireEvent.click(await screen.findByRole('button', { name: t('mobile.settings.logout') }));
		expect(await screen.findByRole('button', { name: t('mobile.login.button') })).toBeTruthy();
	});
});
