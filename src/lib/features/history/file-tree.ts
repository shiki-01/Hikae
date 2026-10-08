import type { ChangeType, FileEntry } from '#lib/api/types.js';

/** ツリーにするファイルの入力。履歴の時点（サイズのみ）と、プロジェクトフォルダ（変更の種類つき）で共通 */
export interface TreeInput extends FileEntry {
	/** 未保存の変更の種類。変更が無ければ null または未指定 */
	change?: ChangeType | null;
	/** 変更のぶつかり中か */
	isConflict?: boolean;
}

export interface TreeFile {
	kind: 'file';
	name: string;
	path: string;
	size: number | null;
	change?: ChangeType | null;
	isConflict?: boolean;
}

export interface TreeFolder {
	kind: 'folder';
	name: string;
	path: string;
	children: TreeNode[];
	/** 配下に、変更のあるファイル（ぶつかり中を含む）がある */
	hasChange: boolean;
}

export type TreeNode = TreeFile | TreeFolder;

function byName(a: TreeNode, b: TreeNode): number {
	return a.name.localeCompare(b.name);
}

function sortNodes(nodes: TreeNode[]): TreeNode[] {
	// フォルダを先、ファイルを後に並べる
	const folders = nodes.filter((n) => n.kind === 'folder').sort(byName);
	const files = nodes.filter((n) => n.kind === 'file').sort(byName);
	return [...folders, ...files];
}

/** パスの一覧をフォルダ階層に変換する。パス区切りは `/` と `\` の両方を受け付ける */
export function buildFileTree(entries: TreeInput[]): TreeNode[] {
	const root: TreeFolder = { kind: 'folder', name: '', path: '', children: [], hasChange: false };
	const folders = new Map<string, TreeFolder>([['', root]]);

	for (const entry of entries) {
		const segments = entry.path.split(/[\\/]+/).filter((segment) => segment !== '');
		const name = segments.pop();
		if (name === undefined) continue;

		const changed = (entry.change ?? null) !== null || entry.isConflict === true;
		let parent = root;
		let current = '';
		for (const segment of segments) {
			current = current === '' ? segment : `${current}/${segment}`;
			let folder = folders.get(current);
			if (!folder) {
				folder = { kind: 'folder', name: segment, path: current, children: [], hasChange: false };
				folders.set(current, folder);
				parent.children.push(folder);
			}
			if (changed) folder.hasChange = true;
			parent = folder;
		}
		const path = current === '' ? name : `${current}/${name}`;
		const file: TreeFile = { kind: 'file', name, path, size: entry.size };
		if (entry.change !== undefined) file.change = entry.change;
		if (entry.isConflict !== undefined) file.isConflict = entry.isConflict;
		parent.children.push(file);
	}

	for (const folder of folders.values()) folder.children = sortNodes(folder.children);
	return root.children;
}
