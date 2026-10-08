import { describe, it, expect } from 'vitest';
import { buildFileTree, type TreeFolder, type TreeNode } from './file-tree';

describe('buildFileTree', () => {
	it('フォルダ階層にまとめ、フォルダを先に並べる', () => {
		const tree = buildFileTree([
			{ path: 'b.txt', size: 1 },
			{ path: 'docs/sub/z.md', size: 2 },
			{ path: 'docs/a.md', size: null },
			{ path: 'a.txt', size: 3 }
		]);
		expect(tree.map((n) => n.name)).toEqual(['docs', 'a.txt', 'b.txt']);
		const docs = tree[0];
		expect(docs.kind).toBe('folder');
		if (docs.kind !== 'folder') return;
		expect(docs.children.map((n) => n.name)).toEqual(['sub', 'a.md']);
		const sub = docs.children[0];
		expect(sub.kind === 'folder' && sub.path).toBe('docs/sub');
		const leaf = sub.kind === 'folder' ? sub.children[0] : null;
		expect(leaf).toEqual({ kind: 'file', name: 'z.md', path: 'docs/sub/z.md', size: 2 });
	});

	it('バックスラッシュ区切りでも同じフォルダにまとめる', () => {
		const tree = buildFileTree([
			{ path: 'dir\\a.txt', size: 1 },
			{ path: 'dir/b.txt', size: 1 }
		]);
		expect(tree).toHaveLength(1);
		expect(tree[0].kind === 'folder' && tree[0].children).toHaveLength(2);
	});

	it('空の入力は空の木になる', () => {
		expect(buildFileTree([])).toEqual([]);
	});
});

describe('buildFileTree（変更の種類つき）', () => {
	const entries = [
		{ path: '資料/第3章.docx', size: 10, change: 'modified' as const },
		{ path: '資料/図/図4.png', size: 20, change: null },
		{ path: '資料/図/図5.png', size: 30, change: 'added' as const },
		{ path: '他/メモ.txt', size: 5, change: null },
		{ path: '競合/a.txt', size: 5, change: null, isConflict: true },
		{ path: 'README.md', size: 1 }
	];

	function folderNamed(nodes: TreeNode[], name: string): TreeFolder {
		const found = nodes.find((n): n is TreeFolder => n.kind === 'folder' && n.name === name);
		if (!found) throw new Error(`folder not found: ${name}`);
		return found;
	}

	it('変更のあるファイルを含むフォルダだけに、変更ありの印が付く（上位のフォルダにも伝わる）', () => {
		const tree = buildFileTree(entries);
		const docs = folderNamed(tree, '資料');
		expect(docs.hasChange).toBe(true);
		expect(folderNamed(docs.children, '図').hasChange).toBe(true);
		expect(folderNamed(tree, '競合').hasChange).toBe(true);
		expect(folderNamed(tree, '他').hasChange).toBe(false);
	});

	it('ファイルに変更の種類とぶつかりの印が引き継がれる', () => {
		const tree = buildFileTree(entries);
		const chapter = folderNamed(tree, '資料').children.find((n) => n.name === '第3章.docx');
		expect(chapter).toMatchObject({ kind: 'file', change: 'modified' });
		expect(folderNamed(tree, '競合').children[0]).toMatchObject({ kind: 'file', isConflict: true });
	});

	it('変更の無いファイルだけのツリーには変更ありの印が付かない', () => {
		const tree = buildFileTree([{ path: 'a/b.txt', size: 1, change: null }]);
		expect(folderNamed(tree, 'a').hasChange).toBe(false);
	});

	it('日本語名・深い階層の変更の印が最上位まで伝わる', () => {
		const tree = buildFileTree([{ path: 'a/b/c/d/e/f/g/深い 階層.txt', size: 1, change: 'added' }]);
		let node: TreeNode = tree[0];
		let depth = 0;
		while (node.kind === 'folder') {
			expect(node.hasChange).toBe(true);
			node = node.children[0];
			depth += 1;
		}
		expect(depth).toBe(7);
		expect(node.name).toBe('深い 階層.txt');
	});
});
