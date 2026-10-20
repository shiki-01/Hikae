<script lang="ts">
	import '#lib/styles/tokens.css';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';
	import { api } from '#lib/api/index.js';
	import { t } from '#lib/i18n/index.js';
	import AppHealthGate from '#lib/features/app-health/AppHealthGate.svelte';
	import ErrorDialog from '#lib/features/notifications/ErrorDialog.svelte';
	import { pushToast } from '#lib/features/notifications/store.svelte.js';
	import ToastHost from '#lib/features/notifications/ToastHost.svelte';
	import { interruptedOperationName } from '#lib/features/projects/interrupted.js';
	import { useBackendEvents } from '#lib/features/projects/live.svelte.js';
	import RemoteFollowUpDialogs from '#lib/features/projects/RemoteFollowUpDialogs.svelte';
	import LocaleSync from '#lib/features/settings/LocaleSync.svelte';

	let { children } = $props();

	const queryClient = new QueryClient({
		defaultOptions: { queries: { refetchOnWindowFocus: false, retry: false } }
	});

	useBackendEvents(queryClient);

	// モバイル版（docs/design.md 14.3）。アプリは起動時に window.__HIKAE_MOBILE__ を true にする。
	// その場合、デスクトップの画面は使わず /m へ移る。ブラウザ単体（pnpm dev）では /m を直接開ける
	const mobileApp = typeof window !== 'undefined' && window.__HIKAE_MOBILE__ === true;
	const onMobileRoute = $derived(page.url.pathname === '/m' || page.url.pathname.startsWith('/m/'));

	$effect(() => {
		if (mobileApp && !onMobileRoute) void goto(resolve('/m'), { replaceState: true });
	});

	onMount(() => {
		// モバイル版では、デスクトップの起動時の通知（作業フォルダの操作）は行わない
		if (mobileApp) return;
		// 起動時に自動で閉じた、途中で止まった操作は、画面の準備ができてから一度だけ知らせる（設計書 6.3）
		void api
			.takeClosedInterruptedNotices()
			.then((operations) => {
				if (operations.length === 0) return;
				const names = [...new Set(operations.map((op) => interruptedOperationName(op)))];
				pushToast({
					type: 'info',
					message: t('interrupted.auto_closed', { name: names.join('、') })
				});
			})
			.catch(() => undefined);
	});
</script>

<QueryClientProvider client={queryClient}>
	{#if onMobileRoute}
		<!-- モバイル版の画面は、デスクトップの起動時の確認（Git の部品）を通らない -->
		{@render children()}
	{:else if !mobileApp}
		<LocaleSync />
		<AppHealthGate>
			{@render children()}
		</AppHealthGate>
		<RemoteFollowUpDialogs />
	{/if}
	<ErrorDialog />
	<ToastHost />
</QueryClientProvider>
