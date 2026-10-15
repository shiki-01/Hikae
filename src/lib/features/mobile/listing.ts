// フォルダの一覧の並べ方。フォルダを先に、その中は名前順（日本語の並び）。

import type { MobileDirEntry } from '#lib/api/mobile-types.js';

const collator = new Intl.Collator('ja', { numeric: true, sensitivity: 'base' });

export function sortEntries(entries: readonly MobileDirEntry[]): MobileDirEntry[] {
	return [...entries].sort((a, b) => {
		if (a.kind !== b.kind) return a.kind === 'dir' ? -1 : 1;
		return collator.compare(a.name, b.name);
	});
}
