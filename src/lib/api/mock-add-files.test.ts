// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { mockApi } from './mock';

// 卒業論文のモックには、保存済みの「第3章.docx」「メモ.txt」と、未保存の変更がある
const PROJECT = 'proj1';

const file = (name: string, size: number | null = 1024) => ({
	name,
	size,
	path: `C:\\外\\${name}`
});

describe('モックのファイル追加（同名の扱い）', () => {
	it('同名があるとき、確認では何も追加せずに一覧を返す', async () => {
		const before = await mockApi.listChanges(PROJECT);
		const outcome = await mockApi.addFiles(PROJECT, [file('第3章.docx'), file('新しい.txt')]);
		expect(outcome.added).toEqual([]);
		expect(outcome.needsDecision).toEqual([{ path: '第3章.docx', canReplace: true }]);
		expect(await mockApi.listChanges(PROJECT)).toHaveLength(before.length);
	});

	it('大きいファイルは置き換えられないと知らせる', async () => {
		const outcome = await mockApi.addFiles(PROJECT, [file('第3章.docx', 60 * 1024 * 1024)]);
		expect(outcome.needsDecision).toEqual([{ path: '第3章.docx', canReplace: false }]);
	});

	it('両方残す・置き換える・追加しないを、ファイルごとに選べる', async () => {
		const outcome = await mockApi.addFiles(
			PROJECT,
			[file('第3章.docx'), file('メモ.txt'), file('新規A.txt')],
			{
				policy: 'keep_both',
				decisions: [
					{ path: '第3章.docx', action: 'replace' },
					{ path: 'メモ.txt', action: 'skip' }
				]
			}
		);
		const byPath = new Map(outcome.added.map((f) => [f.path, f]));
		expect(byPath.get('第3章.docx')).toMatchObject({ replaced: true, renamed: false });
		expect(byPath.has('メモ.txt')).toBe(false);
		expect(byPath.get('新規A.txt')).toMatchObject({ replaced: false, renamed: false });
		// 置き換えたときだけ、取り消し用の復元点が付く
		expect(outcome.undoToken).not.toBeNull();
		await mockApi.undoRestore(PROJECT, outcome.undoToken ?? '');
	});

	it('別名にするときは (2) を付け、追加先のフォルダを含めたパスで返す', async () => {
		await mockApi.addFiles(PROJECT, [file('図.png')], { destSubdir: '資料', policy: 'keep_both' });
		const second = await mockApi.addFiles(PROJECT, [file('図.png')], {
			destSubdir: '資料',
			policy: 'keep_both'
		});
		expect(second.added[0]).toMatchObject({ path: '資料/図 (2).png', renamed: true });
		expect(second.undoToken).toBeNull();
	});

	it('置き換えを選ばれても置き換えられないものは、別名にして印を付ける', async () => {
		const outcome = await mockApi.addFiles(PROJECT, [file('メモ.txt', 60 * 1024 * 1024)], {
			policy: 'replace'
		});
		expect(outcome.added[0]).toMatchObject({
			replaced: false,
			renamed: true,
			replaceRefused: true
		});
	});
});

describe('モックの元に戻した後の保存', () => {
	const ADDED = 'sp2';

	it('メモを渡すと、設定がオンなら続けて保存する', async () => {
		const result = await mockApi.restore(
			PROJECT,
			ADDED,
			{ kind: 'all' },
			'10/4 の状態に戻しました'
		);
		expect(result.save).toBe('saved');
		const points = await mockApi.listSavePoints(PROJECT, 0, 5);
		expect(points[0].message).toBe('10/4 の状態に戻しました');
		expect((await mockApi.listChanges(PROJECT)).length).toBe(0);
	});

	it('メモを渡さないと保存せず、戻した内容が未保存の変更として残る', async () => {
		const result = await mockApi.restore(PROJECT, 'sp1', { kind: 'all' });
		expect(result.save).toBe('not_requested');
	});

	it('設定がオフのときは、メモがあっても保存しない', async () => {
		await mockApi.updateSettings(null, { autoSaveAfterRestore: false });
		const result = await mockApi.restore(PROJECT, 'sp1', { kind: 'all' }, 'メモ');
		expect(result.save).toBe('not_requested');
		await mockApi.updateSettings(null, { autoSaveAfterRestore: true });
	});
});

describe('モックの広すぎるフォルダの確認', () => {
	it('ドライブのルートとホームフォルダは広すぎ、作業用のフォルダは問題ない', async () => {
		expect((await mockApi.checkProjectFolder('C:\\')).broad).toBe('drive_root');
		expect((await mockApi.checkProjectFolder('C:\\Users\\taro')).broad).toBe('home');
		const ok = await mockApi.checkProjectFolder('C:\\Users\\taro\\卒業論文');
		expect(ok.broad).toBeNull();
		expect(ok.warnings).toEqual([]);
	});

	it('同期フォルダの配下は、登録できるが注意を返す', async () => {
		for (const path of [
			'C:\\Users\\taro\\OneDrive\\卒業論文',
			'C:\\Users\\taro\\OneDrive - 研究室\\x',
			'/Users/taro/Dropbox/x',
			'/Users/taro/iCloud Drive/x'
		]) {
			const check = await mockApi.checkProjectFolder(path);
			expect(check.broad, path).toBeNull();
			expect(check.warnings, path).toEqual(['cloud_sync']);
		}
		const plain = await mockApi.checkProjectFolder('C:\\Users\\taro\\OneDriveBackup\\x');
		expect(plain.warnings).toEqual([]);
	});
});
