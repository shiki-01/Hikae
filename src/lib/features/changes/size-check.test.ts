import { describe, it, expect } from 'vitest';
import type { SizeCheck } from '#lib/api/types.js';
import { choiceFor, viewSizeCheck } from './size-check';

const MB = 1024 * 1024;
const warnedOnly: SizeCheck = {
	blocked: [],
	warned: [{ path: '素材\\素材.psd', sizeBytes: 72 * MB }]
};
const mixed: SizeCheck = {
	blocked: [{ path: '動画.mp4', sizeBytes: 250 * MB }],
	warned: [{ path: '素材.psd', sizeBytes: null }]
};

describe('大きいファイルの表示', () => {
	it('保存できないファイルを先に並べ、区切りを / にそろえる', () => {
		const view = viewSizeCheck(mixed);
		expect(view.rows.map((r) => [r.label, r.blocked])).toEqual([
			['動画.mp4', true],
			['素材.psd', false]
		]);
		expect(viewSizeCheck(warnedOnly).rows[0].label).toBe('素材/素材.psd');
	});

	it('警告だけなら「そのまま保存」を出せる', () => {
		expect(viewSizeCheck(warnedOnly)).toMatchObject({ hasBlocked: false, canAccept: true });
	});

	it('保存できないファイルがあれば「そのまま保存」を出さない', () => {
		expect(viewSizeCheck(mixed)).toMatchObject({ hasBlocked: true, canAccept: false });
	});
});

describe('大きいファイルの選択', () => {
	it('そのまま保存は、警告だけのときに警告を受け入れる', () => {
		expect(choiceFor(warnedOnly, 'accept')).toEqual({ acceptWarned: true, exclude: [] });
	});

	it('保存できないファイルがあるときはそのまま保存を作らない', () => {
		expect(choiceFor(mixed, 'accept')).toBeNull();
	});

	it('外す場合は、見つかったファイルのパスを元の形のまま全部外す', () => {
		expect(choiceFor(warnedOnly, 'exclude')).toEqual({
			acceptWarned: false,
			exclude: ['素材\\素材.psd']
		});
		expect(choiceFor(mixed, 'exclude')).toEqual({
			acceptWarned: false,
			exclude: ['動画.mp4', '素材.psd']
		});
	});
});
