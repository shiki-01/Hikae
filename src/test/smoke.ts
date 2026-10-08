// コンポーネントのスモークテスト用の共通ヘルパ。
// 初期化時の例外（宣言順の誤りによる TDZ など）と、握りつぶされたエラーを検出する
import { QueryClient } from '@tanstack/svelte-query';
import { render } from '@testing-library/svelte';
import type { Component } from 'svelte';
import { afterEach, beforeEach, expect, vi } from 'vitest';
import TestHost from './TestHost.svelte';

/**
 * 失敗にしない console.error の許可リスト。無害と確認できたものだけを、理由とともに書く。
 * 現時点で許可するものはない
 */
const ALLOWED_ERRORS: RegExp[] = [];

let errors: string[] = [];
let clients: QueryClient[] = [];
let consoleSpy: ReturnType<typeof vi.spyOn> | undefined;

function record(source: string, value: unknown): void {
	const text = value instanceof Error ? `${value.name}: ${value.message}` : String(value);
	if (ALLOWED_ERRORS.some((pattern) => pattern.test(text))) return;
	errors.push(`${source}: ${text}`);
}

function onUnhandledRejection(reason: unknown): void {
	record('unhandledRejection', reason);
}

function onWindowError(event: ErrorEvent): void {
	record('error', event.error ?? event.message);
}

/** スモークテストのファイルから 1 度呼ぶ。各テストの前後でエラーの収集と後始末を行う */
export function setupSmoke(): void {
	beforeEach(() => {
		errors = [];
		consoleSpy = vi.spyOn(console, 'error').mockImplementation((...args: unknown[]) => {
			record(
				'console.error',
				args.map((a) => (a instanceof Error ? a.message : String(a))).join(' ')
			);
		});
		process.on('unhandledRejection', onUnhandledRejection);
		window.addEventListener('error', onWindowError);
	});

	afterEach(async () => {
		// 非同期の後続処理（クエリの完了など）が落ち着くのを 1 周だけ待ってから判定する
		await Promise.resolve();
		process.off('unhandledRejection', onUnhandledRejection);
		window.removeEventListener('error', onWindowError);
		consoleSpy?.mockRestore();
		// 残ったタイマー（gcTime など）でテストが終わらなくならないよう、クライアントを空にする
		for (const client of clients) client.clear();
		clients = [];
		const found = errors;
		errors = [];
		expect(found, '描画中に console.error または未処理の例外が発生しました').toEqual([]);
	});
}

/** 再試行なしのテスト用 QueryClient で包んで描画する */
export function renderWithClient<P extends Record<string, unknown>>(
	component: Component<P>,
	props: P = {} as P
) {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } }
	});
	clients.push(client);
	const host = TestHost as Component<{ client: QueryClient; component: Component<P>; props: P }>;
	const result = render(host, { props: { client, component, props } });
	return { ...result, client };
}
