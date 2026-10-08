import type { MessageKey } from '#lib/i18n/index.js';
import type { OpenTarget } from '#lib/api/types.js';

/** 「開く」メニューの項目。区切り線は `separator` */
export type OpenMenuEntry =
	{ type: 'item'; target: OpenTarget; label: MessageKey } | { type: 'separator' };

/**
 * 「開く」メニューの項目。バックエンドで安全に実行できるものだけを並べる。
 * 外部コマンドの実行になる「VS Code で開く」は出さない（設計書 4.7）。
 */
export const OPEN_MENU: readonly OpenMenuEntry[] = [
	{ type: 'item', target: 'default', label: 'file_action.open_default' },
	{ type: 'item', target: 'folder', label: 'file_action.show_folder' },
	{ type: 'separator' },
	{ type: 'item', target: 'copy_path', label: 'file_action.copy_path' }
];

/**
 * クリップボードへ渡す、プロジェクト内のファイルのパス。
 * プロジェクトの場所の区切り文字（Windows は `\`、macOS は `/`）に合わせて結ぶ。
 */
export function projectFilePath(root: string, relative: string): string {
	const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/';
	const trimmed = root.replace(/[\\/]+$/, '');
	const parts = relative.split(/[\\/]+/).filter((part) => part.length > 0);
	return [trimmed, ...parts].join(separator);
}
