import { describe, it, expect } from 'vitest';
import type { QueryClient } from '@tanstack/svelte-query';
import { keys } from '#lib/api/keys.js';
import { CHANGES_FALLBACK_INTERVAL_MS, changesRefetchInterval } from './refetch';
import { invalidateChanges } from './invalidate';

describe('変更一覧の取り直しの方針', () => {
	it('ファイル監視が動いている間は、定期的に取り直さない', () => {
		expect(changesRefetchInterval(true, true)).toBe(false);
	});

	it('監視が動いていない Tauri 実行時は、30 秒間隔のフォールバックにする', () => {
		expect(CHANGES_FALLBACK_INTERVAL_MS).toBe(30_000);
		expect(changesRefetchInterval(true, false)).toBe(30_000);
	});

	it('ブラウザ単体（モック）では取り直さない', () => {
		expect(changesRefetchInterval(false, false)).toBe(false);
		expect(changesRefetchInterval(false, true)).toBe(false);
	});
});

describe('ファイル変更の通知による無効化', () => {
	it('変更一覧とメモの案だけを無効化し、履歴などの重い取得は触らない', async () => {
		const invalidated: unknown[] = [];
		const client = {
			invalidateQueries: ({ queryKey }: { queryKey: unknown }) => {
				invalidated.push(queryKey);
				return Promise.resolve();
			}
		} as unknown as QueryClient;

		await invalidateChanges(client, 'p1');

		expect(invalidated).toEqual([keys.changes('p1'), keys.memoSuggestion('p1')]);
	});
});
