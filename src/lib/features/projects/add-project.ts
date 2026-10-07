import type { AddProjectMode, Owner, Visibility } from '#lib/api/types.js';
import type { MessageKey } from '#lib/i18n/index.js';

export interface AddProjectForm {
	mode: AddProjectMode;
	name: string;
	folder: string;
	ownerId: string;
	visibility: Visibility;
	remoteId: string | null;
}

export type FormErrors = Partial<Record<'name' | 'folder' | 'owner' | 'remote', MessageKey>>;

const INVALID_NAME = /[\\/:*?"<>|]/;

export function emptyForm(mode: AddProjectMode, ownerId: string): AddProjectForm {
	return { mode, name: '', folder: '', ownerId, visibility: 'private', remoteId: null };
}

export function nameFromFolder(folder: string): string {
	const trimmed = folder.replace(/[\\/]+$/, '');
	return trimmed.slice(Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\')) + 1);
}

export function validateAddProject(form: AddProjectForm, owners: Owner[]): FormErrors {
	const errors: FormErrors = {};
	if (!form.folder.trim()) errors.folder = 'add_project.error_folder';

	if (form.mode === 'github') {
		if (!form.remoteId) errors.remote = 'add_project.error_remote';
		return errors;
	}

	const name = form.name.trim();
	if (!name) errors.name = 'add_project.error_name';
	else if (INVALID_NAME.test(name)) errors.name = 'add_project.error_name_chars';

	const owner = owners.find((o) => o.id === form.ownerId);
	if (!owner || !owner.canCreate) errors.owner = 'add_project.error_owner';
	return errors;
}

export function canSubmit(form: AddProjectForm, owners: Owner[]): boolean {
	return Object.keys(validateAddProject(form, owners)).length === 0;
}
