<script lang="ts" generics="P extends Record<string, unknown>">
	import type { Component } from 'svelte';
	import { QueryClientProvider, type QueryClient } from '@tanstack/svelte-query';

	interface Props {
		client: QueryClient;
		component: Component<P>;
		props: P;
	}

	let { client, component: Target, props }: Props = $props();
</script>

<QueryClientProvider {client}>
	<Target {...props} />
</QueryClientProvider>
