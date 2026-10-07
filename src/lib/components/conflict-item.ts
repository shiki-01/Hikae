import { t, type MessageKey } from '#lib/i18n/index.js';
import type { Choice, ConflictKind } from '#lib/api/types.js';

export interface ConflictOption {
	choice: Choice;
	label: MessageKey;
}

export function conflictOptions(kind: ConflictKind): ConflictOption[] {
	switch (kind) {
		case 'both':
			return [
				{ choice: 'this', label: 'conflict.use_this' },
				{ choice: 'cloud', label: 'conflict.use_cloud' }
			];
		case 'deleted_in_cloud':
			return [
				{ choice: 'this', label: 'conflict.keep_file' },
				{ choice: 'cloud', label: 'conflict.accept_delete' }
			];
		case 'deleted_on_this_pc':
			return [
				{ choice: 'cloud', label: 'conflict.keep_file' },
				{ choice: 'this', label: 'conflict.accept_delete' }
			];
	}
}

export function alternateName(path: string, unused: Choice, date: Date): string {
	const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
	const folder = path.slice(0, slash + 1);
	const file = path.slice(slash + 1);
	const dot = file.lastIndexOf('.');
	const stem = dot > 0 ? file.slice(0, dot) : file;
	const extension = dot > 0 ? file.slice(dot) : '';
	const month = String(date.getMonth() + 1).padStart(2, '0');
	const day = String(date.getDate()).padStart(2, '0');
	const side = unused === 'cloud' ? t('conflict.alt_cloud') : t('conflict.alt_this');
	return `${folder}${stem} (${side} ${month}-${day})${extension}`;
}
