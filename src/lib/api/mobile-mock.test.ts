// モバイル版のモック（ブラウザ単体の確認用）の振る舞い。同名の判定と、3 択（置き換える・両方残す・追加しない）を確かめる。
import { describe, expect, it } from 'vitest';
import { t } from '#lib/i18n/index.js';
import { base64ToBytes, textToBase64 } from '#lib/features/mobile/base64.js';
import { mobileMockApi as api } from './mobile-mock';

const OWNER = t('mock.mobile.author');
const FAMILY = t('mock.mobile.repo.family');
const DOCS = t('mock.mobile.dir.docs');

describe('モバイルのモック: 保存先とフォルダ', () => {
	it('保存先は 2 件で、家計メモは非公開', async () => {
		const repos = await api.listRepos();
		expect(repos).toHaveLength(2);
		expect(repos[0]).toMatchObject({ name: FAMILY, private: true, defaultBranch: 'main' });
	});

	it('フォルダの直下だけを返し、フォルダは dir で示す', async () => {
		const root = await api.listDir(OWNER, FAMILY, '');
		const names = root.entries.map((entry) => entry.name);
		expect(names).toEqual(expect.arrayContaining(['README.md', DOCS]));
		const docs = await api.listDir(OWNER, FAMILY, DOCS);
		const minutes = docs.entries.find((entry) => entry.name === t('mock.mobile.file.minutes'));
		expect(minutes).toMatchObject({
			kind: 'file',
			path: `${DOCS}/${t('mock.mobile.file.minutes')}`
		});
		expect(names).not.toContain(t('mock.mobile.file.minutes'));
	});

	it('表示の形式を判定する（テキスト・画像・PDF・docx・xlsx・pptx・未対応・大きすぎる）', async () => {
		const minutes = `${DOCS}/${t('mock.mobile.file.minutes')}`;
		const logo = `${t('mock.mobile.dir.photos')}/${t('mock.mobile.file.logo')}`;
		const manual = `${t('mock.mobile.dir.manual')}/${t('mock.mobile.file.manual')}`;
		const estimate = `${DOCS}/${t('mock.mobile.file.estimate')}`;
		const sheet = `${DOCS}/${t('mock.mobile.file.sheet')}`;
		const deck = `${t('mock.mobile.dir.slides')}/${t('mock.mobile.file.deck')}`;
		const zip = `${t('mock.mobile.dir.archive')}/${t('mock.mobile.file.zip')}`;
		const video = `${t('mock.mobile.dir.archive')}/${t('mock.mobile.file.video')}`;
		expect((await api.preview(OWNER, FAMILY, minutes, 'x')).kind).toBe('text');
		expect((await api.preview(OWNER, FAMILY, logo, 'x')).kind).toBe('image');
		expect((await api.preview(OWNER, FAMILY, manual, 'x')).kind).toBe('pdf');
		expect(await api.preview(OWNER, FAMILY, estimate, 'x')).toMatchObject({
			kind: 'document',
			format: 'docx'
		});
		expect(await api.preview(OWNER, FAMILY, sheet, 'x')).toMatchObject({
			kind: 'document',
			format: 'xlsx'
		});
		expect(await api.preview(OWNER, FAMILY, deck, 'x')).toMatchObject({
			kind: 'document',
			format: 'pptx'
		});
		expect((await api.preview(OWNER, FAMILY, zip, 'x')).kind).toBe('unsupported');
		expect((await api.preview(OWNER, FAMILY, video, 'x')).kind).toBe('tooLarge');
		await expect(api.download(OWNER, FAMILY, video, 'x')).rejects.toMatchObject({
			code: 'backend'
		});
	});

	it('テキストの内容を UTF-8 として返す', async () => {
		const minutes = `${DOCS}/${t('mock.mobile.file.minutes')}`;
		const preview = await api.preview(OWNER, FAMILY, minutes, 'x');
		expect(preview).toMatchObject({ kind: 'text', truncated: false });
		if (preview.kind === 'text')
			expect(preview.text).toContain(t('mock.mobile.body.minutes').slice(0, 5));
	});
});

describe('モバイルのモック: 同名の判定とアップロード', () => {
	it('同名のファイルがあるとき、確認の結果に exists を返す', async () => {
		const plan = await api.planUpload(OWNER, FAMILY, [
			{ targetDir: '', name: 'README.md', size: 1 },
			{ targetDir: '', name: 'new.txt', size: 1 }
		]);
		expect(plan.items).toEqual([
			{ targetPath: 'README.md', exists: true },
			{ targetPath: 'new.txt', exists: false }
		]);
	});

	it('「追加しない」は何も変えず、保存しなかった件数に入れる', async () => {
		const before = await api.fileHistory(OWNER, FAMILY, 'README.md');
		const result = await api.upload(OWNER, FAMILY, null, [
			{ targetDir: '', name: 'README.md', contentBase64: textToBase64('skip'), onExists: 'skip' }
		]);
		expect(result).toEqual({ commitSha: '', saved: [], skipped: ['README.md'], renamed: [] });
		expect(await api.fileHistory(OWNER, FAMILY, 'README.md')).toEqual(before);
	});

	it('「両方残す」は名前に (2) を付けて残し、変えた名前を返す', async () => {
		const result = await api.upload(OWNER, FAMILY, 'テスト', [
			{
				targetDir: '',
				name: 'README.md',
				contentBase64: textToBase64('keep'),
				onExists: 'keepBoth'
			}
		]);
		expect(result.saved).toEqual(['README (2).md']);
		expect(result.renamed).toEqual([{ from: 'README.md', to: 'README (2).md' }]);
		expect(result.commitSha).not.toBe('');
		const root = await api.listDir(OWNER, FAMILY, '');
		expect(root.entries.map((entry) => entry.name)).toEqual(
			expect.arrayContaining(['README (2).md'])
		);
	});

	it('「置き換える」は内容を変え、古い版は過去の版として残る', async () => {
		const before = await api.fileHistory(OWNER, FAMILY, 'README.md');
		const result = await api.upload(OWNER, FAMILY, null, [
			{
				targetDir: '',
				name: 'README.md',
				contentBase64: textToBase64('replaced'),
				onExists: 'replace'
			}
		]);
		expect(result.saved).toEqual(['README.md']);
		expect(result.renamed).toEqual([]);
		const after = await api.fileHistory(OWNER, FAMILY, 'README.md');
		expect(after.length).toBe(before.length + 1);
		const preview = await api.preview(OWNER, FAMILY, 'README.md', 'x');
		expect(preview.kind === 'text' ? preview.text : '').toBe('replaced');
	});

	it('同名のない項目は、そのまま追加する（内容のサイズも返す）', async () => {
		const bytes = textToBase64('新しい');
		await api.upload(OWNER, FAMILY, 'メモ', [
			{ targetDir: DOCS, name: 'new.txt', contentBase64: bytes, onExists: 'keepBoth' }
		]);
		const docs = await api.listDir(OWNER, FAMILY, DOCS);
		const added = docs.entries.find((entry) => entry.name === 'new.txt');
		expect(added?.size).toBe(base64ToBytes(bytes).length);
	});
});
