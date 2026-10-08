import { describe, it, expect } from 'vitest';
import { mapPull, mapPush } from '#lib/api/mappers.js';
import { syncSizeRequest } from './sync-size';

const large = { blocked: [{ path: '動画.mp4', size: 250 * 1024 * 1024 }], warned: [] };

describe('取り込み・アップロードの見送り', () => {
	it('取り込み結果の size_check が、確認が必要な内容になる', () => {
		const result = mapPull({
			outcome: 'skipped',
			conflicts: [],
			size_check: large,
			unsaved_count: null
		});
		expect(result.mergedCount).toBe(0);
		expect(syncSizeRequest('fetch', result)).toEqual({
			kind: 'fetch',
			check: { blocked: [{ path: '動画.mp4', sizeBytes: 250 * 1024 * 1024 }], warned: [] }
		});
	});

	it('アップロード結果の size_check も同様に扱う', () => {
		const request = syncSizeRequest('push', mapPush({ outcome: 'skipped', size_check: large }));
		expect(request?.kind).toBe('push');
	});

	it('size_check が無い・該当ファイルが空なら見送りではない', () => {
		const pulled = mapPull({
			outcome: 'merged',
			conflicts: [],
			size_check: null,
			unsaved_count: null
		});
		expect(syncSizeRequest('fetch', pulled)).toBeNull();
		const empty = mapPush({ outcome: 'pushed', size_check: { blocked: [], warned: [] } });
		expect(syncSizeRequest('push', empty)).toBeNull();
	});
});
