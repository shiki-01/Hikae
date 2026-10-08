import { describe, it, expect } from 'vitest';
import { mockApi } from './mock';

// 卒業論文のモックには、新規（未追跡）の「新しい案.txt」と、大きい「ロゴ原案.psd」がある
const PROJECT = 'proj1';

describe('モックの「元に戻す（作成しない）」', () => {
	it('大きいファイルと保存済みのファイルは、削除せずに断る', async () => {
		await expect(mockApi.discardNewFile(PROJECT, 'ロゴ原案.psd')).rejects.toMatchObject({
			backend: { code: 'discard_file_too_large' }
		});
		await expect(mockApi.discardNewFile(PROJECT, '第3章.docx')).rejects.toMatchObject({
			backend: { code: 'discard_not_untracked' }
		});
		const paths = (await mockApi.listChanges(PROJECT)).map((c) => c.path);
		expect(paths).toContain('ロゴ原案.psd');
		expect(paths).toContain('第3章.docx');
	});

	it('新規ファイルを一覧から外し、取り消しで元に戻せる', async () => {
		const before = await mockApi.listChanges(PROJECT);
		const result = await mockApi.discardNewFile(PROJECT, '新しい案.txt');
		expect(result.undoToken).not.toBeNull();
		const after = await mockApi.listChanges(PROJECT);
		expect(after.map((c) => c.path)).not.toContain('新しい案.txt');
		expect(after).toHaveLength(before.length - 1);

		await mockApi.undoRestore(PROJECT, result.undoToken ?? '');
		expect((await mockApi.listChanges(PROJECT)).map((c) => c.path)).toContain('新しい案.txt');
	});

	it('新規ファイルを「いま」と比べると、全行が追加の差分になる', async () => {
		const diff = await mockApi.compare(PROJECT, '新しい案.txt', 'sp1', 'current');
		expect(diff.kind).toBe('text');
		if (diff.kind === 'text') {
			expect(diff.rows.length).toBeGreaterThan(0);
			expect(diff.rows.every((row) => row.kind === 'add')).toBe(true);
		}
	});
});

describe('モックの「すべてのファイル」', () => {
	it('変更のあるファイルに種類が付き、変更の無いファイルも含む', async () => {
		const tree = await mockApi.listProjectTree(PROJECT);
		expect(tree.truncated).toBe(false);
		const byPath = new Map(tree.files.map((f) => [f.path, f]));
		expect(byPath.get('第3章.docx')?.change).toBe('modified');
		expect(byPath.get('はじめに.txt')?.change).toBeNull();
		expect(byPath.get('古い案.txt')).toMatchObject({ change: 'deleted', sizeBytes: null });
	});
});
