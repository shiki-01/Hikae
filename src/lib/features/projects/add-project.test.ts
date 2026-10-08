import { describe, it, expect } from 'vitest';
import type { Owner } from '#lib/api/types.js';
import { canSubmit, emptyForm, nameFromFolder, validateAddProject } from './add-project';

const owners: Owner[] = [
	{ id: 'personal', name: 'me', kind: 'personal', canCreate: true },
	{ id: 'org', name: 'team', kind: 'org', canCreate: true },
	{ id: 'locked', name: 'locked', kind: 'org', canCreate: false }
];

describe('プロジェクト追加フォームの検証', () => {
	it('既存フォルダ: フォルダと名前が揃えば追加できる', () => {
		const form = {
			...emptyForm('existing', 'personal'),
			folder: 'C:\\Docs\\Thesis',
			name: 'Thesis'
		};
		expect(canSubmit(form, owners)).toBe(true);
	});

	it('既存フォルダ: 空のままでは追加できない', () => {
		const errors = validateAddProject(emptyForm('existing', 'personal'), owners);
		expect(errors.folder).toBe('add_project.error_folder');
		expect(errors.name).toBe('add_project.error_name');
	});

	it('名前に使えない文字を含むとエラーになる', () => {
		const form = { ...emptyForm('new', 'personal'), folder: 'C:\\Docs', name: 'a/b' };
		expect(validateAddProject(form, owners).name).toBe('add_project.error_name_chars');
	});

	it('空白だけの名前は未入力として扱う', () => {
		const form = { ...emptyForm('new', 'personal'), folder: 'C:\\Docs', name: '   ' };
		expect(validateAddProject(form, owners).name).toBe('add_project.error_name');
	});

	it('作成権限のない保存先は選べない', () => {
		const form = { ...emptyForm('new', 'locked'), folder: 'C:\\Docs', name: 'x' };
		expect(validateAddProject(form, owners).owner).toBe('add_project.error_owner');
	});

	it('GitHub から取得: プロジェクトとフォルダの選択が必要で、名前は求めない', () => {
		const empty = validateAddProject(emptyForm('github', 'personal'), owners);
		expect(empty.remote).toBe('add_project.error_remote');
		expect(empty.name).toBeUndefined();
		const filled = { ...emptyForm('github', 'personal'), folder: 'C:\\Docs', remoteId: 'r1' };
		expect(canSubmit(filled, owners)).toBe(true);
	});

	it('公開範囲の初期値は非公開', () => {
		expect(emptyForm('new', 'personal').visibility).toBe('private');
	});

	it('フォルダ名からプロジェクト名を作れる', () => {
		expect(nameFromFolder('C:\\Users\\me\\Thesis')).toBe('Thesis');
		expect(nameFromFolder('/Users/me/Thesis/')).toBe('Thesis');
	});
});

describe('GitHub への保存先の作成に関する検証', () => {
	const base = { ...emptyForm('new', 'personal'), folder: 'C:\\Docs', name: 'x' };

	it('公開にするときは、警告を確認するまで追加できない', () => {
		const form = { ...base, visibility: 'public' as const };
		expect(validateAddProject(form, owners).confirm).toBe('add_project.error_public_confirm');
		expect(canSubmit({ ...form, publicConfirmed: true }, owners)).toBe(true);
	});

	it('非公開なら確認は要らない', () => {
		expect(validateAddProject(base, owners).confirm).toBeUndefined();
		expect(canSubmit(base, owners)).toBe(true);
	});

	it('保存先を選べない（未ログイン）ときは、ローカルだけで登録できる', () => {
		const form = { ...base, ownerId: 'personal', visibility: 'public' as const };
		expect(canSubmit(form, [], false)).toBe(true);
		// 名前とフォルダの検証は残る
		expect(validateAddProject({ ...form, name: '' }, [], false).name).toBe(
			'add_project.error_name'
		);
	});
});
