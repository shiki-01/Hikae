import { t, type MessageKey } from '#lib/i18n/index.js';
import { formatDateTime } from '#lib/i18n/format.js';
import type { Choice, ConflictFile, ConflictKind } from '#lib/api/types.js';

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

/**
 * 各版の説明文。保存日時と PC 名のどちらも無ければ undefined（説明を出さない）。
 * PC 名だけがある場合も、どの PC の版かが分かるように出す。
 */
export function conflictDetail(side: Choice, conflict: ConflictFile): string | undefined {
	const savedAt = side === 'this' ? conflict.thisPcSavedAt : conflict.cloudSavedAt;
	const pc = side === 'this' ? conflict.thisPcName : conflict.cloudPcName;
	const time = savedAt ? formatDateTime(savedAt) : null;
	if (side === 'this') {
		if (time && pc) return t('conflict.this_pc_detail_pc', { time, pc });
		if (time) return t('conflict.this_pc_detail', { time });
		return pc ? t('conflict.this_pc_only_pc', { pc }) : undefined;
	}
	if (time && pc) return t('conflict.cloud_detail', { time, pc });
	if (time) return t('conflict.cloud_detail_no_pc', { time });
	return pc ? t('conflict.cloud_only_pc', { pc }) : undefined;
}

/** 日時が不明（null）のときは日付部分を付けない */
export function alternateName(path: string, unused: Choice, date: Date | null): string {
	const slash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
	const folder = path.slice(0, slash + 1);
	const file = path.slice(slash + 1);
	const dot = file.lastIndexOf('.');
	const stem = dot > 0 ? file.slice(0, dot) : file;
	const extension = dot > 0 ? file.slice(dot) : '';
	const side = unused === 'cloud' ? t('conflict.alt_cloud') : t('conflict.alt_this');
	if (date === null) return `${folder}${stem} (${side})${extension}`;
	const month = String(date.getMonth() + 1).padStart(2, '0');
	const day = String(date.getDate()).padStart(2, '0');
	return `${folder}${stem} (${side} ${month}-${day})${extension}`;
}
