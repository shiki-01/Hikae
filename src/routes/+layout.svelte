<script lang="ts">
	import '#lib/styles/tokens.css';
	import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';
	import AppHealthGate from '#lib/features/app-health/AppHealthGate.svelte';
	import ErrorDialog from '#lib/features/notifications/ErrorDialog.svelte';
	import ToastHost from '#lib/features/notifications/ToastHost.svelte';
	import { useBackendEvents } from '#lib/features/projects/live.svelte.js';
	import RemoteFollowUpDialogs from '#lib/features/projects/RemoteFollowUpDialogs.svelte';

	let { children } = $props();

	const queryClient = new QueryClient({
		defaultOptions: { queries: { refetchOnWindowFocus: false, retry: false } }
	});

	useBackendEvents(queryClient);
</script>

<QueryClientProvider client={queryClient}>
	<AppHealthGate>
		{@render children()}
	</AppHealthGate>
	<RemoteFollowUpDialogs />
	<ErrorDialog />
	<ToastHost />
</QueryClientProvider>
