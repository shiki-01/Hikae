import { describe, it, expect } from 'vitest';
import { droppedFromPaths, toDropAction, toLogicalPoint } from './native-drop';

const position = { x: 0, y: 0 } as never;

describe('アプリ上のドロップの変換', () => {
	it('実パスからファイル名を取り出し、大きさは不明にする', () => {
		expect(droppedFromPaths(['C:\\Users\\a\\資料\\図4.png', '/home/a/memo.txt'])).toEqual([
			{ name: '図4.png', size: null, path: 'C:\\Users\\a\\資料\\図4.png' },
			{ name: 'memo.txt', size: null, path: '/home/a/memo.txt' }
		]);
	});

	it('ドロップでファイルを渡し、ドラッグ中は位置を伝える', () => {
		expect(toDropAction({ type: 'enter', paths: ['/a/b.txt'], position })).toEqual({
			kind: 'enter',
			point: { x: 0, y: 0 }
		});
		expect(toDropAction({ type: 'over', position })).toEqual({
			kind: 'over',
			point: { x: 0, y: 0 }
		});
		expect(toDropAction({ type: 'leave' })).toEqual({ kind: 'leave' });
		expect(toDropAction({ type: 'drop', paths: ['/a/b.txt'], position })).toEqual({
			kind: 'drop',
			files: [{ name: 'b.txt', size: null, path: '/a/b.txt' }],
			point: { x: 0, y: 0 }
		});
	});

	it('パスが空のドロップは何も追加しない', () => {
		expect(toDropAction({ type: 'drop', paths: [], position })).toEqual({ kind: 'leave' });
		expect(toDropAction({ type: 'enter', paths: [], position })).toBeNull();
	});

	it('物理ピクセルの位置を、拡大率で割って CSS ピクセルにする', () => {
		expect(toLogicalPoint({ x: 300, y: 150 }, 1.5)).toEqual({ x: 200, y: 100 });
		expect(toLogicalPoint({ x: 300, y: 150 }, 1)).toEqual({ x: 300, y: 150 });
		// 拡大率が不正なときは、そのまま使う（0 割りや NaN にしない）
		expect(toLogicalPoint({ x: 10, y: 20 }, 0)).toEqual({ x: 10, y: 20 });
		expect(toLogicalPoint({ x: 10, y: 20 }, Number.NaN)).toEqual({ x: 10, y: 20 });
		const over = { type: 'over', position: { x: 250, y: 125 } } as never;
		expect(toDropAction(over, 2.5)).toEqual({ kind: 'over', point: { x: 100, y: 50 } });
	});
});
