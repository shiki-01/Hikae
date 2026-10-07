<script lang="ts">
	import { t } from '#lib/i18n/index.js';
	import { formatRelative } from '#lib/i18n/format.js';
	import type { Change } from '#lib/api/types.js';
	import ChangeListItem from '#lib/components/ChangeListItem.svelte';
	import DropZone from '#lib/components/DropZone.svelte';
	import Skeleton from '#lib/components/Skeleton.svelte';

	interface Props {
		changes: Change[];
		loading: boolean;
		selectedPath: string | null;
		lastSavedAt: Date | null;
		dragging: boolean;
		tooLarge: boolean;
		onselect: (path: string) => void;
		onfiles: (files: File[]) => void;
	}

	let {
		changes,
		loading,
		selectedPath,
		lastSavedAt,
		dragging,
		tooLarge,
		onselect,
		onfiles
	}: Props = $props();
</script>

<div class="flex flex-direction:column flex:1 min-h:0">
	<div class="flex:1 min-h:0 overflow-y:auto">
		{#if loading}
			<div class="flex flex-direction:column gap:3 p:3" aria-busy="true">
				{#each [0, 1, 2, 3] as row (row)}
					<Skeleton class="h:40px" />
				{/each}
			</div>
		{:else if changes.length === 0}
			<div class="flex flex-direction:column align-items:center gap:1 p:6 text-align:center">
				<p class="m:0 type-body font-weight:700">{t('changes.empty')}</p>
				{#if lastSavedAt}
					<p class="m:0 type-small fg:fg-muted">
						{t('changes.last_saved', { when: formatRelative(lastSavedAt) })}
					</p>
				{/if}
			</div>
		{:else}
			<ul class="m:0 p:0 list-style:none" aria-label={t('changes.list_label')}>
				{#each changes as change (change.id)}
					<li>
						<ChangeListItem
							{change}
							selected={selectedPath === change.path}
							onclick={() => onselect(change.path)}
						/>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
	<div class="p:3 flex-shrink:0 bt:1px|solid|border">
		<DropZone {dragging} {tooLarge} {onfiles} />
	</div>
</div>
