import { describe, it, expect } from 'vitest';
import { resolveStatus as calculateStatusPriority, type StatusState } from './status-header';

describe('StatusHeader priority calculation', () => {
	it('should return conflict status with highest priority', () => {
		const result = calculateStatusPriority(
			true, // hasConflict
			false,
			true,
			5,
			2,
			1
		);
		expect(result).toBe('conflict');
	});

	it('should return syncing before offline', () => {
		const result = calculateStatusPriority(
			false,
			true, // isSyncing
			false, // !isOnline
			5,
			2,
			1
		);
		expect(result).toBe('syncing');
	});

	it('should return offline before unsaved', () => {
		const result = calculateStatusPriority(false, false, false, 5, 2, 1);
		expect(result).toBe('offline');
	});

	it('should return unsaved before fetch_pending', () => {
		const result = calculateStatusPriority(false, false, true, 5, 2, 0);
		expect(result).toBe('unsaved');
	});

	it('should return fetch_pending before push_pending', () => {
		const result = calculateStatusPriority(false, false, true, 0, 2, 1);
		expect(result).toBe('fetch_pending');
	});

	it('should return push_pending before saved', () => {
		const result = calculateStatusPriority(false, false, true, 0, 0, 1);
		expect(result).toBe('push_pending');
	});

	it('should return saved when all conditions are clear', () => {
		const result = calculateStatusPriority(false, false, true, 0, 0, 0);
		expect(result).toBe('saved');
	});

	it('should respect priority order: conflict > syncing > offline > unsaved > fetch > push > saved', () => {
		const priorities = {
			conflict: 0,
			syncing: 1,
			offline: 2,
			unsaved: 3,
			fetch_pending: 4,
			push_pending: 5,
			saved: 6
		} as Record<StatusState, number>;

		const testCases = [
			{
				state: 'conflict' as StatusState,
				hasConflict: true,
				isSyncing: true,
				isOnline: false,
				unsavedCount: 5,
				fetchPendingCount: 2,
				uploadPendingCount: 1
			},
			{
				state: 'syncing' as StatusState,
				hasConflict: false,
				isSyncing: true,
				isOnline: false,
				unsavedCount: 5,
				fetchPendingCount: 2,
				uploadPendingCount: 1
			},
			{
				state: 'offline' as StatusState,
				hasConflict: false,
				isSyncing: false,
				isOnline: false,
				unsavedCount: 5,
				fetchPendingCount: 2,
				uploadPendingCount: 1
			},
			{
				state: 'unsaved' as StatusState,
				hasConflict: false,
				isSyncing: false,
				isOnline: true,
				unsavedCount: 5,
				fetchPendingCount: 2,
				uploadPendingCount: 1
			},
			{
				state: 'fetch_pending' as StatusState,
				hasConflict: false,
				isSyncing: false,
				isOnline: true,
				unsavedCount: 0,
				fetchPendingCount: 2,
				uploadPendingCount: 1
			},
			{
				state: 'push_pending' as StatusState,
				hasConflict: false,
				isSyncing: false,
				isOnline: true,
				unsavedCount: 0,
				fetchPendingCount: 0,
				uploadPendingCount: 1
			},
			{
				state: 'saved' as StatusState,
				hasConflict: false,
				isSyncing: false,
				isOnline: true,
				unsavedCount: 0,
				fetchPendingCount: 0,
				uploadPendingCount: 0
			}
		];

		for (let i = 0; i < testCases.length - 1; i++) {
			const current = testCases[i];
			const next = testCases[i + 1];

			const currentPriority = priorities[current.state];
			const nextPriority = priorities[next.state];

			expect(currentPriority).toBeLessThan(nextPriority);

			const result = calculateStatusPriority(
				current.hasConflict,
				current.isSyncing,
				current.isOnline,
				current.unsavedCount,
				current.fetchPendingCount,
				current.uploadPendingCount
			);

			expect(result).toBe(current.state);
		}
	});
});

describe('途中で止まった操作', () => {
	it('ぶつかりより後で、処理中・オフライン・未保存より先に表示する', () => {
		expect(calculateStatusPriority(true, false, true, 0, 0, 0, false, true)).toBe('conflict');
		expect(calculateStatusPriority(false, true, false, 5, 2, 1, true, true)).toBe('interrupted');
		expect(calculateStatusPriority(false, false, false, 5, 2, 1, false, true)).toBe('interrupted');
		expect(calculateStatusPriority(false, false, true, 5, 2, 1, false, true)).toBe('interrupted');
	});

	it('残っていなければ従来の順序のまま', () => {
		expect(calculateStatusPriority(false, false, true, 5, 2, 1, false, false)).toBe('unsaved');
	});
});

describe('大きいファイルによる見送り', () => {
	it('ぶつかり・途中で止まった操作より後、処理中・オフライン・未保存より先に表示する', () => {
		const large = (
			...args: [boolean, boolean, boolean, number, number, number, boolean, boolean]
		) => calculateStatusPriority(...args, true);
		expect(large(true, false, true, 0, 0, 0, false, false)).toBe('conflict');
		expect(large(false, false, true, 0, 0, 0, false, true)).toBe('interrupted');
		expect(large(false, true, false, 5, 2, 1, true, false)).toBe('large_files');
		expect(large(false, false, false, 5, 2, 1, false, false)).toBe('large_files');
		expect(large(false, false, true, 5, 2, 1, false, false)).toBe('large_files');
	});

	it('なければ従来の順序のまま', () => {
		expect(calculateStatusPriority(false, false, true, 5, 2, 1, false, false, false)).toBe(
			'unsaved'
		);
	});
});

describe('GitHub に接続していない場合', () => {
	const unconnected = (
		...args: [boolean, boolean, boolean, number, number, number, boolean, boolean, boolean]
	) => calculateStatusPriority(...args, true);

	it('すべて保存済みの代わりに、接続を促す', () => {
		expect(unconnected(false, false, true, 0, 0, 0, false, false, false)).toBe('not_connected');
	});

	it('未保存・取り込み待ち・アップロード待ちなどの、ほかの状態が優先される', () => {
		expect(unconnected(true, false, true, 0, 0, 0, false, false, false)).toBe('conflict');
		expect(unconnected(false, true, true, 0, 0, 0, false, false, false)).toBe('syncing');
		expect(unconnected(false, false, true, 3, 0, 0, false, false, false)).toBe('unsaved');
		expect(unconnected(false, false, true, 0, 2, 0, false, false, false)).toBe('fetch_pending');
		expect(unconnected(false, false, true, 0, 0, 1, false, false, false)).toBe('push_pending');
	});

	it('接続していれば従来どおり保存済みになる', () => {
		expect(calculateStatusPriority(false, false, true, 0, 0, 0, false, false, false, false)).toBe(
			'saved'
		);
	});
});
