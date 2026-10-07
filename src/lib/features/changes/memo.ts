import type { Change } from '#lib/api/types.js';
import { t } from '#lib/i18n/index.js';

const MAX_NAMED = 3;

function baseName(path: string): string {
	return path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);
}

export function suggestMemo(changes: Change[]): string {
	if (changes.length === 0) return '';
	const named = changes
		.slice(0, MAX_NAMED)
		.map((change) => t(`memo.${change.type}`, { name: baseName(change.path) }));
	const rest = changes.length - named.length;
	if (rest > 0) named.push(t('memo.others', { count: rest }));
	return named.join(t('memo.separator'));
}
