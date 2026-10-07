<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import Skeleton from '#lib/components/Skeleton.svelte';
	import TimelineItem from '#lib/components/TimelineItem.svelte';
	import type { TimelineEntry } from '#lib/components/timeline.js';

	interface Props {
		entries: TimelineEntry[];
		loading: boolean;
		selectedId: string | null;
		expanded: string[];
		onselect: (id: string) => void;
		ontoggle: (id: string) => void;
	}

	let { entries, loading, selectedId, expanded, onselect, ontoggle }: Props = $props();

	const saveCount = $derived(entries.filter((e) => e.kind === 'save').length);
</script>

<div class="flex:1 min-h:0 overflow-y:auto p:3">
	{#if loading}
		<div class="flex flex-direction:column gap:3" aria-busy="true">
			{#each [0, 1, 2, 3] as row (row)}
				<Skeleton class="h:48px" />
			{/each}
		</div>
	{:else}
		<ol class="m:0 p:0 list-style:none" aria-label={t('history.list_label')}>
			{#each entries as entry, index (entry.id)}
				<TimelineItem
					{entry}
					{selectedId}
					expanded={expanded.includes(entry.id)}
					isLast={index === entries.length - 1}
					{onselect}
					{ontoggle}
				/>
			{/each}
		</ol>
		{#if saveCount <= 1}
			<p class="m:0 mt:3 type-small fg:fg-muted text-align:center">{t('timeline.empty_message')}</p>
		{/if}
	{/if}
</div>
