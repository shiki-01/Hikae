import type { FileEntry } from '#lib/api/types.js';

export interface TreeFile {
	kind: 'file';
	name: string;
	path: string;
	size: number | null;
}

export interface TreeFolder {
	kind: 'folder';
	name: string;
	path: string;
	children: TreeNode[];
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
export function buildFileTree(entries: FileEntry[]): TreeNode[] {
	const root: TreeFolder = { kind: 'folder', name: '', path: '', children: [] };
	const folders = new Map<string, TreeFolder>([['', root]]);

	for (const entry of entries) {
		const segments = entry.path.split(/[\\/]+/).filter((segment) => segment !== '');
		const name = segments.pop();
		if (name === undefined) continue;

		let parent = root;
		let current = '';
		for (const segment of segments) {
			current = current === '' ? segment : `${current}/${segment}`;
			let folder = folders.get(current);
			if (!folder) {
				folder = { kind: 'folder', name: segment, path: current, children: [] };
				folders.set(current, folder);
				parent.children.push(folder);
			}
			parent = folder;
		}
		const path = current === '' ? name : `${current}/${name}`;
		parent.children.push({ kind: 'file', name, path, size: entry.size });
	}

	for (const folder of folders.values()) folder.children = sortNodes(folder.children);
	return root.children;
}
