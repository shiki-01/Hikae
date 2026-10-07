<script lang="ts">
	import '#lib/styles/tokens.css';
	import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';
	import ErrorDialog from '#lib/features/notifications/ErrorDialog.svelte';
	import ToastHost from '#lib/features/notifications/ToastHost.svelte';

	let { children } = $props();

	const queryClient = new QueryClient({
		defaultOptions: { queries: { refetchOnWindowFocus: false, retry: false } }
	});
</script>

<QueryClientProvider client={queryClient}>
	{@render children()}
	<ErrorDialog />
	<ToastHost />
</QueryClientProvider>
