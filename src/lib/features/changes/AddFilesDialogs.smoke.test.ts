// @vitest-environment jsdom
import { fireEvent, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { setupSmoke, renderWithClient } from '../../../test/smoke';
import { t } from '#lib/i18n/index.js';
import type { AddFilesOutcome } from '#lib/api/types.js';
import AddConflictDialog from './AddConflictDialog.svelte';
import AddFilesResultDialog from './AddFilesResultDialog.svelte';
import { summarizeAddFiles } from './add-files-result';

setupSmoke();

const noop = () => {};

describe('AddConflictDialog のスモークテスト', () => {
	const conflicts = [
		{ path: 'a.txt', canReplace: true },
		{ path: '資料/b.psd', canReplace: false },
		{ path: '資料/c.docx', canReplace: true }
	];

	it('同名のファイルが一覧に出て、最初は「両方残す」が選ばれている', async () => {
		renderWithClient(AddConflictDialog, {
			conflicts,
			pending: false,
			onconfirm: noop,
			oncancel: noop
		});
		expect(await screen.findByText(t('add_conflict.title'))).toBeTruthy();
		expect(screen.getByText('a.txt')).toBeTruthy();
		expect(screen.getByText('b.psd')).toBeTruthy();
		const keepBoth = screen.getAllByRole('radio', { name: t('add_conflict.keep_both') });
		expect(keepBoth).toHaveLength(3);
		expect(keepBoth.every((radio) => (radio as HTMLInputElement).checked)).toBe(true);
	});

	it('置き換えられないファイルでは、置き換えを選べない', async () => {
		renderWithClient(AddConflictDialog, {
			conflicts,
			pending: false,
			onconfirm: noop,
			oncancel: noop
		});
		await screen.findByText(t('add_conflict.title'));
		const replace = screen.getAllByRole('radio', { name: t('add_conflict.replace') });
		expect((replace[0] as HTMLInputElement).disabled).toBe(false);
		expect((replace[1] as HTMLInputElement).disabled).toBe(true);
		expect(screen.getByText(t('add_conflict.replace_unavailable'))).toBeTruthy();
	});

	it('ファイルごとに選んだ内容を、確認で送る', async () => {
		const onconfirm = vi.fn();
		renderWithClient(AddConflictDialog, { conflicts, pending: false, onconfirm, oncancel: noop });
		await screen.findByText(t('add_conflict.title'));
		await fireEvent.click(screen.getAllByRole('radio', { name: t('add_conflict.replace') })[0]);
		await fireEvent.click(screen.getAllByRole('radio', { name: t('add_conflict.skip') })[2]);
		await fireEvent.click(screen.getByRole('button', { name: t('add_conflict.confirm') }));
		expect(onconfirm).toHaveBeenCalledWith([
			{ path: 'a.txt', action: 'replace' },
			{ path: '資料/b.psd', action: 'keep_both' },
			{ path: '資料/c.docx', action: 'skip' }
		]);
	});

	it('複数あるときは「すべてに同じ選択を使う」で一括して選べる', async () => {
		const onconfirm = vi.fn();
		renderWithClient(AddConflictDialog, { conflicts, pending: false, onconfirm, oncancel: noop });
		await screen.findByText(t('add_conflict.title'));
		await fireEvent.click(screen.getByRole('button', { name: t('add_conflict.apply_all_label') }));
		await fireEvent.click(await screen.findByRole('option', { name: t('add_conflict.replace') }));
		await fireEvent.click(screen.getByRole('button', { name: t('add_conflict.confirm') }));
		// 置き換えられないファイルは、両方残すになる
		expect(onconfirm).toHaveBeenCalledWith([
			{ path: 'a.txt', action: 'replace' },
			{ path: '資料/b.psd', action: 'keep_both' },
			{ path: '資料/c.docx', action: 'replace' }
		]);
	});

	it('1 件のときは一括の選択を出さず、キャンセルできる', async () => {
		const oncancel = vi.fn();
		renderWithClient(AddConflictDialog, {
			conflicts: [conflicts[0]],
			pending: false,
			onconfirm: noop,
			oncancel
		});
		await screen.findByText(t('add_conflict.title'));
		expect(screen.queryByRole('button', { name: t('add_conflict.apply_all_label') })).toBeNull();
		await fireEvent.click(screen.getByRole('button', { name: t('add_conflict.cancel') }));
		expect(oncancel).toHaveBeenCalled();
	});
});

describe('AddFilesResultDialog のスモークテスト', () => {
	it('置き換え・別名・追加できなかった理由・飛ばしたリンクを分けて出す', async () => {
		const outcome: AddFilesOutcome = {
			added: [
				{ path: 'r.txt', renamed: false, large: false, replaced: true, replaceRefused: false },
				{ path: 'x (2).psd', renamed: true, large: false, replaced: false, replaceRefused: true },
				{ path: 'k (2).txt', renamed: true, large: false, replaced: false, replaceRefused: false }
			],
			rejected: [
				{ name: '資料/巨大.mov', reason: 'too_large', size: 200 * 1024 * 1024 },
				{ name: 'busy.docx', reason: 'in_use', size: null }
			],
			skipped: [
				{ name: '資料/ショートカット', reason: 'link' },
				{ name: '資料/.DS_Store', reason: 'os_temp' }
			],
			needsDecision: [],
			undoToken: 'refs/hikae/snapshots/main/1'
		};
		renderWithClient(AddFilesResultDialog, {
			summary: summarizeAddFiles(outcome),
			onclose: noop
		});
		expect(await screen.findByText(t('add_files.title'))).toBeTruthy();
		expect(screen.getByText(t('add_files.replaced_heading'))).toBeTruthy();
		expect(screen.getByText(t('add_files.replace_refused_heading'))).toBeTruthy();
		expect(screen.getByText(t('add_files.renamed_heading'))).toBeTruthy();
		expect(screen.getByText(t('add_files.rejected_heading'))).toBeTruthy();
		expect(screen.getByText(t('add_files.reason_in_use'))).toBeTruthy();
		expect(screen.getByText(t('add_files.skipped_links_heading'))).toBeTruthy();
		expect(screen.getByText('資料/ショートカット')).toBeTruthy();
		expect(screen.getByText(t('add_files.skipped_hidden', { count: 1 }))).toBeTruthy();
	});
});
