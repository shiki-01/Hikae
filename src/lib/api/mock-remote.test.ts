import { describe, it, expect } from 'vitest';
import { AppError } from './errors';
import { MOCK_EXISTING_EMPTY, MOCK_TAKEN, mockApi } from './mock';
import { followUpFor } from '#lib/features/projects/remote-follow-up.js';
import { describeError } from '#lib/features/notifications/error-view.js';

const OWNER = 'shiki-01';
const base = {
	mode: 'new' as const,
	folder: 'C:/Users/student/Documents',
	ownerId: OWNER,
	visibility: 'private' as const,
	connectCloud: true
};

describe('モックの保存先の作成と続き', () => {
	it('同名の空の保存先があると、作成も接続もせずに確認を返し、承認すると接続できる', async () => {
		const added = await mockApi.addProject({ ...base, name: MOCK_EXISTING_EMPTY });
		expect(added.project.remoteConnected).toBe(false);
		expect(added.remote).toMatchObject({
			connected: false,
			existingEmptyRepository: `${OWNER}/${MOCK_EXISTING_EMPTY}`,
			error: null
		});

		const asked = await mockApi.connectRemote(added.project.id, {
			ownerId: OWNER,
			visibility: 'private',
			publicConfirmed: false,
			name: MOCK_EXISTING_EMPTY
		});
		expect(asked.existingEmptyRepository).not.toBeNull();
		expect(asked.connected).toBe(false);

		const adopted = await mockApi.connectRemote(added.project.id, {
			ownerId: OWNER,
			visibility: 'private',
			publicConfirmed: false,
			name: MOCK_EXISTING_EMPTY,
			adoptExisting: true
		});
		expect(adopted).toMatchObject({
			connected: true,
			repository: `${OWNER}/${MOCK_EXISTING_EMPTY}`,
			existingEmptyRepository: null
		});
		expect((await mockApi.getProject(added.project.id)).remoteConnected).toBe(true);
	});

	it('「別の名前にする」と、名前を変えて新しく作って接続できる', async () => {
		const added = await mockApi.addProject({ ...base, name: MOCK_EXISTING_EMPTY });
		const renamed = await mockApi.connectRemote(added.project.id, {
			ownerId: OWNER,
			visibility: 'private',
			publicConfirmed: false,
			name: 'thesis-2',
			adoptExisting: false
		});
		expect(renamed).toMatchObject({ connected: true, repository: `${OWNER}/thesis-2` });
	});

	it('空でない保存先には、承認されても接続せず、名前の衝突のエラーにする', async () => {
		const added = await mockApi.addProject({ ...base, name: MOCK_TAKEN });
		// 追加は成功し、ローカルの登録だけが残る。エラーは結果に入る
		expect(added.project.remoteConnected).toBe(false);
		expect(added.remote?.connected).toBe(false);
		expect(added.remote?.error?.backend?.code).toBe('remote_name_taken');

		const input = { ownerId: OWNER, visibility: 'private' as const, publicConfirmed: false };
		await expect(
			mockApi.connectRemote(added.project.id, { ...input, name: MOCK_TAKEN })
		).rejects.toBeInstanceOf(AppError);
		await expect(
			mockApi.connectRemote(added.project.id, {
				...input,
				name: MOCK_EXISTING_EMPTY + '-x',
				adoptExisting: true
			})
		).rejects.toMatchObject({ backend: { code: 'remote_name_taken' } });
		expect((await mockApi.getProject(added.project.id)).remoteConnected).toBe(false);
	});

	it('最初の保存に大きいファイルがあると、作成の前に確認を返し、選択を反映してから接続できる', async () => {
		const added = await mockApi.addProject({ ...base, name: 'large-project' });
		expect(added.project.remoteConnected).toBe(false);
		expect(added.remote).toMatchObject({ connected: false, repository: null, error: null });
		expect(added.remote?.sizeCheck?.warned).toHaveLength(1);

		const followUp = followUpFor(added.remote, {
			projectId: added.project.id,
			projectName: added.project.name,
			input: { ownerId: OWNER, visibility: 'private', publicConfirmed: false }
		});
		expect(followUp?.kind).toBe('size');

		const path = added.remote?.sizeCheck?.warned[0].path ?? '';
		const saved = await mockApi.saveWithSizeChoice(added.project.id, '最初の保存', {
			acceptWarned: false,
			exclude: [path]
		});
		expect(saved).toEqual({ kind: 'saved' });
		const connected = await mockApi.connectRemote(added.project.id, {
			ownerId: OWNER,
			visibility: 'private',
			publicConfirmed: false
		});
		expect(connected).toMatchObject({ connected: true, existingEmptyRepository: null });
	});
});

describe('保存先の作成のエラー文言', () => {
	it('失敗の文言は 3 要素がそろい、Git 用語を使わない', () => {
		const taken = describeError(
			new AppError(
				'backend',
				'x',
				{},
				{
					code: 'remote_orphaned',
					params: { name: 'me/thesis' },
					whatHappened: 'rust',
					dataIsSafe: 'rust',
					nextAction: 'rust'
				}
			)
		);
		expect(taken.title).toContain('me/thesis');
		expect(taken.title).not.toBe('rust');
		expect(taken.message).toContain('この PC に安全に残っています');
		expect(taken.message).toContain('もう一度「接続する」');
	});
});
