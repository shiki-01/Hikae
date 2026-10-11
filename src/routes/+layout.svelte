<script lang="ts">
	import '#lib/styles/tokens.css';
	import { onMount } from 'svelte';
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

	let { children } = $props();

	const queryClient = new QueryClient({
		defaultOptions: { queries: { refetchOnWindowFocus: false, retry: false } }
	});

	useBackendEvents(queryClient);

	onMount(() => {
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
	<AppHealthGate>
		{@render children()}
	</AppHealthGate>
	<RemoteFollowUpDialogs />
	<ErrorDialog />
	<ToastHost />
</QueryClientProvider>
