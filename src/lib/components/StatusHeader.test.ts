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
