import type { AddProjectMode, Owner, Visibility } from '#lib/api/types.js';
import type { MessageKey } from '#lib/i18n/index.js';

export interface AddProjectForm {
	mode: AddProjectMode;
	name: string;
	folder: string;
	ownerId: string;
	visibility: Visibility;
	/** 公開にすることの警告を確認した（非公開のままなら不要） */
	publicConfirmed: boolean;
	remoteId: string | null;
}

export type FormErrors = Partial<
	Record<'name' | 'folder' | 'owner' | 'remote' | 'confirm', MessageKey>
>;

const INVALID_NAME = /[\\/:*?"<>|]/;

export function emptyForm(mode: AddProjectMode, ownerId: string): AddProjectForm {
	return {
		mode,
		name: '',
		folder: '',
		ownerId,
		visibility: 'private',
		publicConfirmed: false,
		remoteId: null
	};
}

export function nameFromFolder(folder: string): string {
	const trimmed = folder.replace(/[\\/]+$/, '');
	return trimmed.slice(Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\')) + 1);
}

/**
 * フォルダや名前などの入力を検証する。
 * `cloudAvailable` が false（未ログインなどで保存先を選べない）のときは、ローカルだけで登録するため
 * 保存先と公開範囲は検証しない。
 */
export function validateAddProject(
	form: AddProjectForm,
	owners: Owner[],
	cloudAvailable = true
): FormErrors {
	const errors: FormErrors = {};
	if (!form.folder.trim()) errors.folder = 'add_project.error_folder';

	if (form.mode === 'github') {
		if (!form.remoteId) errors.remote = 'add_project.error_remote';
		return errors;
	}

	const name = form.name.trim();
	if (!name) errors.name = 'add_project.error_name';
	else if (INVALID_NAME.test(name)) errors.name = 'add_project.error_name_chars';

	if (!cloudAvailable) return errors;
	const owner = owners.find((o) => o.id === form.ownerId);
	if (!owner || !owner.canCreate) errors.owner = 'add_project.error_owner';
	if (form.visibility === 'public' && !form.publicConfirmed) {
		errors.confirm = 'add_project.error_public_confirm';
	}
	return errors;
}

export function canSubmit(form: AddProjectForm, owners: Owner[], cloudAvailable = true): boolean {
	return Object.keys(validateAddProject(form, owners, cloudAvailable)).length === 0;
}
