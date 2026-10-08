import { describe, it, expect } from 'vitest';
import { mockApi } from './mock';

// 卒業論文のモックには、保存前の検査で警告になる .psd の未保存変更がある
const PROJECT = 'proj1';

describe('モックの保存と大きいファイル', () => {
	it('大きいファイルがあると何も保存せず確認を返す', async () => {
		const before = (await mockApi.listSavePoints(PROJECT)).length;
		const outcome = await mockApi.save(PROJECT, 'メモ');
		expect(outcome.kind).toBe('size_check');
		if (outcome.kind === 'size_check') {
			expect(outcome.check.blocked).toEqual([]);
			expect(outcome.check.warned.map((f) => f.path)).toEqual(['ロゴ原案.psd']);
		}
		expect((await mockApi.listSavePoints(PROJECT)).length).toBe(before);
	});

	it('外す選択をすると、そのファイルを除いて保存し、未保存から消える', async () => {
		const outcome = await mockApi.saveWithSizeChoice(PROJECT, 'メモ', {
			acceptWarned: false,
			exclude: ['ロゴ原案.psd']
		});
		expect(outcome).toEqual({ kind: 'saved' });
		expect(await mockApi.listChanges(PROJECT)).toEqual([]);
	});
});
