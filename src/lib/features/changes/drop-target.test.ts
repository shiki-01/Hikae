// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { DROP_FOLDER_ATTR, folderAtPoint, folderLabel, type HitTest } from './drop-target';

/** 位置ごとに決めた要素を返す、位置の引き方の代役 */
function hitTest(elementAt: (x: number, y: number) => Element | null): HitTest {
	return { elementFromPoint: elementAt };
}

describe('ドロップ位置のフォルダ行の判定', () => {
	function tree() {
		const root = document.createElement('div');
		root.innerHTML = `
			<ul>
				<li>
					<div id="folder" ${DROP_FOLDER_ATTR}="資料/図">
						<span id="folder-name">図</span>
					</div>
				</li>
				<li><button id="file">a.txt</button></li>
			</ul>
			<div id="blank"></div>`;
		document.body.append(root);
		const get = (id: string) => root.querySelector(`#${id}`);
		return { root, get };
	}

	it('ツリー表示でフォルダ行の上なら、そのフォルダのパスを返す（行の中の要素でも同じ）', () => {
		const { root, get } = tree();
		const at = hitTest((x) => (x === 1 ? get('folder') : get('folder-name')));
		expect(folderAtPoint({ x: 1, y: 0 }, at, true)).toBe('資料/図');
		expect(folderAtPoint({ x: 2, y: 0 }, at, true)).toBe('資料/図');
		root.remove();
	});

	it('ファイルの行・余白・何も無い場所はプロジェクト直下（null）', () => {
		const { root, get } = tree();
		expect(
			folderAtPoint(
				{ x: 0, y: 0 },
				hitTest(() => get('file')),
				true
			)
		).toBeNull();
		expect(
			folderAtPoint(
				{ x: 0, y: 0 },
				hitTest(() => get('blank')),
				true
			)
		).toBeNull();
		expect(
			folderAtPoint(
				{ x: 0, y: 0 },
				hitTest(() => null),
				true
			)
		).toBeNull();
		root.remove();
	});

	it('ツリー表示でないとき（変更のみ表示）は、常にプロジェクト直下', () => {
		const { root, get } = tree();
		expect(
			folderAtPoint(
				{ x: 0, y: 0 },
				hitTest(() => get('folder')),
				false
			)
		).toBeNull();
		expect(folderAtPoint({ x: 0, y: 0 }, null, true)).toBeNull();
		root.remove();
	});

	it('空の属性は無視し、画面に出す名前は最後の成分にする', () => {
		const empty = document.createElement('div');
		empty.setAttribute(DROP_FOLDER_ATTR, '');
		expect(
			folderAtPoint(
				{ x: 0, y: 0 },
				hitTest(() => empty),
				true
			)
		).toBeNull();
		expect(folderLabel('資料/図/下書き')).toBe('下書き');
		expect(folderLabel('資料')).toBe('資料');
	});
});
