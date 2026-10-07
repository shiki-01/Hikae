<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import ProjectScreen from '#lib/features/projects/ProjectScreen.svelte';

	const projectId = $derived(page.url.searchParams.get('id') ?? '');

	$effect(() => {
		if (!projectId) void goto(resolve('/'));
	});
</script>

{#if projectId}
	{#key projectId}
		<ProjectScreen {projectId} />
	{/key}
{/if}
