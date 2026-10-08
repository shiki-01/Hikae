import { describe, it, expect } from 'vitest';
import { droppedFromPaths, toDropAction } from './native-drop';

const position = { x: 0, y: 0 } as never;

describe('アプリ上のドロップの変換', () => {
	it('実パスからファイル名を取り出し、大きさは不明にする', () => {
		expect(droppedFromPaths(['C:\\Users\\a\\資料\\図4.png', '/home/a/memo.txt'])).toEqual([
			{ name: '図4.png', size: null, path: 'C:\\Users\\a\\資料\\図4.png' },
			{ name: 'memo.txt', size: null, path: '/home/a/memo.txt' }
		]);
	});

	it('ドロップでファイルを渡し、ドラッグ中は強調表示の開始と終了だけを伝える', () => {
		expect(toDropAction({ type: 'enter', paths: ['/a/b.txt'], position })).toEqual({
			kind: 'enter'
		});
		expect(toDropAction({ type: 'over', position })).toBeNull();
		expect(toDropAction({ type: 'leave' })).toEqual({ kind: 'leave' });
		expect(toDropAction({ type: 'drop', paths: ['/a/b.txt'], position })).toEqual({
			kind: 'drop',
			files: [{ name: 'b.txt', size: null, path: '/a/b.txt' }]
		});
	});

	it('パスが空のドロップは何も追加しない', () => {
		expect(toDropAction({ type: 'drop', paths: [], position })).toEqual({ kind: 'leave' });
		expect(toDropAction({ type: 'enter', paths: [], position })).toBeNull();
	});
});
