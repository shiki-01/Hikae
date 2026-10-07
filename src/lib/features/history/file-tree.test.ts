import { describe, it, expect } from 'vitest';
import { buildFileTree } from './file-tree';

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
